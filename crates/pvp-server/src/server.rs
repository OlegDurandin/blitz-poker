//! HTTP and WebSocket endpoints.
//!
//! - `POST /api/auth` — VK launch parameters → session + one-time ticket
//! - `POST /api/ticket` — `Authorization: Bearer <session>` → new ticket
//! - `GET /ws?ticket=…` — the game socket (checks `Origin`)
//! - `GET /health` — liveness and counters, no personal data

use crate::auth::{clean_name, clean_photo};
use crate::hub::{Hub, Outgoing, now_ms};
use crate::room::Profile;
use crate::vk;
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tower_http::cors::{AllowOrigin, CorsLayer};

/// Largest frame a client may send.
const MAX_FRAME: usize = 8 * 1024;
/// A socket with no traffic for this long is dead (clients ping every 15 s).
const IDLE: Duration = Duration::from_secs(45);
/// Messages per window before a client is disconnected.
const RATE_LIMIT: u32 = 60;
const RATE_WINDOW: Duration = Duration::from_secs(10);

pub fn router(hub: Arc<Hub>) -> Router {
    let allowed = hub.config.allowed_origins.clone();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            origin
                .to_str()
                .is_ok_and(|o| crate::config::origin_allowed(&allowed, o))
        }))
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION]);
    Router::new()
        .route("/api/auth", post(auth))
        .route("/api/ticket", post(ticket))
        .route("/ws", get(ws))
        .route("/health", get(health))
        .layer(cors)
        .with_state(hub)
}

#[derive(Deserialize)]
struct AuthRequest {
    /// `window.location.search` of the Mini App, as VK signed it.
    launch_params: Option<String>,
    /// Local development only (`PVP_DEV_AUTH=1` on a loopback address).
    dev_user_id: Option<u64>,
    name: Option<String>,
    photo: Option<String>,
}

fn reject(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(serde_json::json!({ "ok": false, "error": { "code": code, "message": message } })),
    )
        .into_response()
}

async fn auth(State(hub): State<Arc<Hub>>, Json(req): Json<AuthRequest>) -> Response {
    let cfg = &hub.config;
    let vk_id = match (&req.launch_params, req.dev_user_id) {
        (Some(params), _) => {
            match vk::verify(
                params,
                cfg.app_secret.as_bytes(),
                cfg.app_id,
                cfg.launch_max_age_secs,
                now_ms() / 1000,
            ) {
                Ok(launch) => launch.user_id,
                Err(e) => {
                    hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
                    tracing::warn!("launch params rejected: {e:?}");
                    return reject(
                        StatusCode::UNAUTHORIZED,
                        "bad_launch_params",
                        "Не удалось подтвердить вход через VK",
                    );
                }
            }
        }
        (None, Some(id)) if cfg.dev_auth && id > 0 => id,
        _ => {
            hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
            return reject(
                StatusCode::UNAUTHORIZED,
                "bad_launch_params",
                "Не удалось подтвердить вход через VK",
            );
        }
    };
    let profile = Profile {
        vk_id,
        name: clean_name(req.name.as_deref().unwrap_or("")),
        photo: clean_photo(req.photo.as_deref()),
    };
    let (session, ticket) = hub.issue_session(profile.clone());
    Json(serde_json::json!({
        "ok": true,
        "session": session,
        "ticket": ticket,
        "protocol": crate::protocol::PROTOCOL_VERSION,
        "user": { "name": profile.name, "photo": profile.photo },
    }))
    .into_response()
}

async fn ticket(State(hub): State<Arc<Hub>>, headers: HeaderMap) -> Response {
    let session = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match session.and_then(|s| hub.ticket_for(s)) {
        Some(ticket) => Json(serde_json::json!({ "ok": true, "ticket": ticket })).into_response(),
        None => reject(
            StatusCode::UNAUTHORIZED,
            "session_expired",
            "Сессия истекла, войдите снова",
        ),
    }
}

async fn health(State(hub): State<Arc<Hub>>) -> Response {
    Json(hub.health()).into_response()
}

#[derive(Deserialize)]
struct WsQuery {
    ticket: String,
}

async fn ws(
    State(hub): State<Arc<Hub>>,
    Query(q): Query<WsQuery>,
    headers: HeaderMap,
    upgrade: WebSocketUpgrade,
) -> Response {
    // Browsers always send Origin on WebSocket handshakes; other clients
    // still need a valid ticket, so a missing Origin is not a bypass.
    if let Some(origin) = headers.get(header::ORIGIN).and_then(|o| o.to_str().ok())
        && !hub.config.origin_allowed(origin)
    {
        hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
        return reject(
            StatusCode::FORBIDDEN,
            "origin_not_allowed",
            "Недопустимый источник",
        );
    }
    let Some(profile) = hub.redeem_ticket(&q.ticket) else {
        hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
        return reject(
            StatusCode::UNAUTHORIZED,
            "bad_ticket",
            "Сессия истекла, войдите снова",
        );
    };
    upgrade
        .max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| session(hub, profile, socket))
}

async fn session(hub: Arc<Hub>, profile: Profile, socket: WebSocket) {
    let (conn_id, mut outgoing) = hub.connect(profile);
    let (mut sink, mut stream) = socket.split();

    let writer = tokio::spawn(async move {
        while let Some(out) = outgoing.recv().await {
            let done = matches!(out, Outgoing::Close);
            let msg = match out {
                Outgoing::Text(t) => Message::Text(t.into()),
                Outgoing::Close => Message::Close(Some(CloseFrame {
                    code: 4001,
                    reason: "replaced".into(),
                })),
            };
            if sink.send(msg).await.is_err() || done {
                break;
            }
        }
    });

    let mut window_start = tokio::time::Instant::now();
    let mut count = 0u32;
    loop {
        let next = tokio::time::timeout(IDLE, stream.next()).await;
        let Ok(Some(Ok(msg))) = next else { break };
        if window_start.elapsed() > RATE_WINDOW {
            window_start = tokio::time::Instant::now();
            count = 0;
        }
        count += 1;
        if count > RATE_LIMIT {
            tracing::warn!("rate limit exceeded; closing socket");
            break;
        }
        match msg {
            Message::Text(text) => hub.handle(conn_id, &text),
            Message::Close(_) => break,
            _ => {}
        }
    }
    hub.disconnect(conn_id);
    writer.abort();
}
