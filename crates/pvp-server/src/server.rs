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
    /// Diagnostics only: client build and the URL fragment it was opened with.
    entry: Option<Entry>,
    /// Outside VK: a random key the browser keeps, so a guest who reloads
    /// gets the same seat back. Guests may only join rooms, never host.
    guest_key: Option<String>,
}

/// Guest sign-ins allowed per minute, server-wide: guests cost nothing to
/// make, so this bounds sessions an anonymous flood can create.
const GUEST_AUTH_PER_MINUTE: u32 = 60;

fn guest_auth_allowed(now: u64) -> bool {
    static WINDOW: std::sync::Mutex<(u64, u32)> = std::sync::Mutex::new((0, 0));
    let mut w = WINDOW.lock().unwrap_or_else(|e| e.into_inner());
    if now.saturating_sub(w.0) >= 60_000 {
        *w = (now, 0);
    }
    w.1 += 1;
    w.1 <= GUEST_AUTH_PER_MINUTE
}

/// A stable guest id from the browser's key; None for a malformed key.
fn guest_id(key: &str) -> Option<u64> {
    use sha2::{Digest, Sha256};
    let ok = (32..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !ok {
        return None;
    }
    let digest = Sha256::digest(format!("blitz-guest:{key}"));
    let mut bytes = [0u8; 8];
    bytes[2..].copy_from_slice(&digest[..6]);
    Some(crate::room::GUEST_BIT | u64::from_be_bytes(bytes))
}

#[derive(Deserialize, Default)]
struct Entry {
    build: Option<String>,
    hash: Option<String>,
    hash_now: Option<String>,
}

/// A log-safe copy of client-supplied text: short, printable, and with long
/// tokens (room ids) cut to their first four characters.
fn log_safe(text: Option<&str>) -> String {
    let text = text.unwrap_or("");
    let mut out = String::new();
    let mut run = 0;
    for c in text.chars().take(120) {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            run += 1;
            if run <= 4 {
                out.push(c);
            } else if run == 5 {
                out.push('…');
            }
        } else {
            run = 0;
            out.push(if c.is_ascii_graphic() { c } else { '?' });
        }
    }
    out
}

/// One launch parameter, for logs (`vk_platform`, `vk_ref`): not secret.
fn launch_param<'a>(params: &'a str, key: &str) -> &'a str {
    params
        .trim_start_matches('?')
        .split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
        .unwrap_or("")
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
        (None, None) if cfg.guest_auth && req.guest_key.is_some() => {
            let Some(id) = req.guest_key.as_deref().and_then(guest_id) else {
                hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
                return reject(
                    StatusCode::BAD_REQUEST,
                    "bad_guest_key",
                    "Не удалось войти как гость",
                );
            };
            if !guest_auth_allowed(now_ms()) {
                return reject(
                    StatusCode::TOO_MANY_REQUESTS,
                    "busy",
                    "Слишком много входов, попробуйте через минуту",
                );
            }
            id
        }
        _ => {
            hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
            return reject(
                StatusCode::UNAUTHORIZED,
                "bad_launch_params",
                "Не удалось подтвердить вход через VK",
            );
        }
    };
    let guest = crate::room::is_guest(vk_id);
    let name = clean_name(req.name.as_deref().unwrap_or(""));
    let profile = Profile {
        vk_id,
        // The host should know who is not signed in through VK.
        name: if guest {
            format!("{name} (гость)")
        } else {
            name
        },
        photo: if guest {
            None
        } else {
            clean_photo(req.photo.as_deref())
        },
    };
    let entry = req.entry.unwrap_or_default();
    let params = req.launch_params.as_deref().unwrap_or("");
    tracing::info!(
        user = %user_tag(vk_id),
        guest,
        build = %log_safe(entry.build.as_deref()),
        platform = %log_safe(Some(launch_param(params, "vk_platform"))),
        vk_ref = %log_safe(Some(launch_param(params, "vk_ref"))),
        hash = %log_safe(entry.hash.as_deref()),
        hash_now = %log_safe(entry.hash_now.as_deref()),
        "signed in"
    );
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

/// Tells users apart in logs without logging VK ids.
fn user_tag(vk_id: u64) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("blitz-log:{vk_id}"));
    digest[..3].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod log_tests {
    use super::*;

    #[test]
    fn log_safe_cuts_room_ids() {
        assert_eq!(
            log_safe(Some("#room=O6ceAbCdEfGhIjKlMn0_-q")),
            "#room=O6ce…"
        );
        assert_eq!(log_safe(Some("#room%3DO6ceAbCdEfGh")), "#room%3DO6…");
        assert_eq!(log_safe(None), "");
        assert_eq!(
            launch_param(
                "vk_app_id=1&vk_platform=mobile_iphone&sign=x",
                "vk_platform"
            ),
            "mobile_iphone"
        );
        assert_ne!(user_tag(1), user_tag(2));
    }

    #[test]
    fn guest_ids_are_stable_and_never_vk_ids() {
        let key = "k".repeat(43);
        let id = guest_id(&key).unwrap();
        assert_eq!(guest_id(&key), Some(id));
        assert!(crate::room::is_guest(id));
        assert!(!crate::room::is_guest(9_999_999_999));
        assert_ne!(guest_id(&"j".repeat(43)), Some(id));
        assert_eq!(guest_id("short"), None);
        assert_eq!(guest_id(&format!("{}!", "k".repeat(40))), None);
    }
}
