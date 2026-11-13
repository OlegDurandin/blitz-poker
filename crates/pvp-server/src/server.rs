//! HTTP and WebSocket endpoints.
//!
//! - `POST /api/auth` — VK launch parameters → session + one-time ticket
//! - `POST /api/ticket` — `Authorization: Bearer <session>` → new ticket
//! - `GET /ws?ticket=…` — the game socket (checks `Origin`)
//! - `GET /health` — liveness and counters, no personal data

use crate::auth::{clean_name, clean_photo};
use crate::hub::{Hub, Outgoing, now_ms};
use crate::player::PlayerId;
use crate::room::Profile;
use crate::{telegram, vk};
use axum::extract::ws::{CloseFrame, Message, WebSocket, WebSocketUpgrade};
use axum::extract::{DefaultBodyLimit, Path, Query, State};
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
        .route("/api/me/profile", get(personal_profile))
        .route("/api/me/matches", get(personal_matches))
        .route("/api/me/matches/{event}", get(personal_detail))
        .route(
            "/api/me/local-results",
            post(local_result).layer(DefaultBodyLimit::max(4096)),
        )
        .layer(cors)
        .with_state(hub)
}

/// How a player proves who they are; exactly one proof per request.
#[derive(Deserialize)]
struct AuthRequest {
    /// VK: `window.location.search` of the Mini App, as VK signed it.
    launch_params: Option<String>,
    /// Telegram: `Telegram.WebApp.initData`, signed for the bot.
    tg_init_data: Option<String>,
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

/// The player's network for rate limits: the address Caddy put last in
/// X-Forwarded-For (it overwrites what untrusted clients send), with IPv6
/// cut to its /64 so one home network counts once. Direct hits share "direct".
pub fn client_key(headers: &HeaderMap) -> String {
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|v| v.trim().parse::<std::net::IpAddr>().ok());
    match ip {
        Some(std::net::IpAddr::V4(v4)) => v4.to_string(),
        Some(std::net::IpAddr::V6(v6)) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
        }
        None => "direct".into(),
    }
}

/// A stable guest id from the browser's key; None for a malformed key.
fn guest_id(key: &str) -> Option<PlayerId> {
    use sha2::{Digest, Sha256};
    let ok = (32..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_');
    if !ok {
        return None;
    }
    let digest = Sha256::digest(format!("blitz-guest:{key}"));
    let mut bytes = [0u8; 6];
    bytes.copy_from_slice(&digest[..6]);
    Some(PlayerId::guest(bytes))
}

/// A verified player, with what their platform vouches for.
struct Identity {
    id: PlayerId,
    /// Signed by the platform (Telegram); otherwise the client's text is used.
    name: Option<String>,
    photo: Option<String>,
}

fn unauthorized(hub: &Hub, message: &str) -> Box<Response> {
    hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
    Box::new(reject(
        StatusCode::UNAUTHORIZED,
        "bad_launch_params",
        message,
    ))
}

/// Checks the one proof in `req` against its platform.
/// The error is the response to send (boxed: responses are large).
fn identify(hub: &Hub, headers: &HeaderMap, req: &AuthRequest) -> Result<Identity, Box<Response>> {
    let cfg = &hub.config;
    let now_s = now_ms() / 1000;
    let plain = |id| Identity {
        id,
        name: None,
        photo: None,
    };
    if let Some(params) = &req.launch_params {
        if cfg.app_secret.is_empty() {
            return Err(unauthorized(hub, "Вход через VK отключён"));
        }
        let launch = vk::verify(
            params,
            cfg.app_secret.as_bytes(),
            cfg.app_id,
            cfg.launch_max_age_secs,
            now_s,
        )
        .map_err(|e| {
            tracing::warn!("launch params rejected: {e:?}");
            unauthorized(hub, "Не удалось подтвердить вход через VK")
        })?;
        let id = PlayerId::vk(launch.user_id)
            .ok_or_else(|| unauthorized(hub, "Не удалось подтвердить вход через VK"))?;
        return Ok(plain(id));
    }
    if let Some(init_data) = &req.tg_init_data {
        let Some(token) = &cfg.tg_bot_token else {
            return Err(unauthorized(hub, "Вход через Telegram отключён"));
        };
        let launch =
            telegram::verify(init_data, token, cfg.launch_max_age_secs, now_s).map_err(|e| {
                tracing::warn!("telegram init data rejected: {e:?}");
                unauthorized(hub, "Не удалось подтвердить вход через Telegram")
            })?;
        let id = PlayerId::telegram(launch.user_id)
            .ok_or_else(|| unauthorized(hub, "Не удалось подтвердить вход через Telegram"))?;
        return Ok(Identity {
            id,
            name: Some(launch.name),
            photo: launch.photo_url,
        });
    }
    if let Some(dev) = req.dev_user_id {
        return match PlayerId::vk(dev) {
            Some(id) if cfg.dev_auth => Ok(plain(id)),
            _ => Err(unauthorized(hub, "Не удалось подтвердить вход")),
        };
    }
    if let Some(key) = &req.guest_key
        && cfg.guest_auth
    {
        let Some(id) = guest_id(key) else {
            hub.metrics.auth_failures.fetch_add(1, Ordering::Relaxed);
            return Err(Box::new(reject(
                StatusCode::BAD_REQUEST,
                "bad_guest_key",
                "Не удалось войти как гость",
            )));
        };
        if !hub.guest_auth_allowed(&client_key(headers)) {
            return Err(Box::new(reject(
                StatusCode::TOO_MANY_REQUESTS,
                "busy",
                "Слишком много входов, попробуйте через минуту",
            )));
        }
        return Ok(plain(id));
    }
    Err(unauthorized(hub, "Откройте игру в VK или Telegram"))
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

async fn auth(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Json(req): Json<AuthRequest>,
) -> Response {
    let who = match identify(&hub, &headers, &req) {
        Ok(who) => who,
        Err(response) => return *response,
    };
    let guest = who.id.is_guest();
    let name = clean_name(who.name.as_deref().or(req.name.as_deref()).unwrap_or(""));
    let profile = Profile {
        id: who.id,
        // The host should know who is not signed in through a messenger.
        name: if guest {
            format!("{name} (гость)")
        } else {
            name
        },
        photo: if guest {
            None
        } else {
            clean_photo(who.photo.as_deref().or(req.photo.as_deref()))
        },
    };
    let entry = req.entry.unwrap_or_default();
    let params = req.launch_params.as_deref().unwrap_or("");
    tracing::info!(
        user = %user_tag(who.id),
        via = who.id.platform().as_str(),
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
        "account_key": who.id.to_string(),
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
        .on_upgrade(move |socket| session(hub, profile, client_key(&headers), socket))
}

async fn session(hub: Arc<Hub>, profile: Profile, client: String, socket: WebSocket) {
    let (conn_id, mut outgoing) = hub.connect(profile, client);
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
fn user_tag(id: PlayerId) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(format!("blitz-log:{}", id.raw()));
    digest[..3].iter().map(|b| format!("{b:02x}")).collect()
}

fn personal_response(
    hub: &Hub,
    headers: &HeaderMap,
    query: crate::progress::HistoryQuery,
    event: Option<&str>,
    report: Option<&crate::progress::LocalReport>,
    profile: bool,
) -> Response {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    match hub.personal(token, query, event, report, profile) {
        Ok(value) => Json(value).into_response(),
        Err(code) => reject(
            StatusCode::from_u16(code).unwrap(),
            "profile_error",
            match code {
                401 => "Откройте профиль через VK или Telegram",
                400 => "Некорректные данные",
                404 => "Матч не найден",
                429 => "Повторите через минуту",
                _ => "Не удалось сохранить или загрузить профиль. Повторите",
            },
        ),
    }
}
async fn personal_profile(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Query(q): Query<crate::progress::HistoryQuery>,
) -> Response {
    personal_response(&hub, &headers, q, None, None, true)
}
async fn personal_matches(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Query(q): Query<crate::progress::HistoryQuery>,
) -> Response {
    personal_response(&hub, &headers, q, None, None, false)
}
async fn personal_detail(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Path(event): Path<String>,
) -> Response {
    personal_response(
        &hub,
        &headers,
        Default::default(),
        Some(&event),
        None,
        false,
    )
}
async fn local_result(
    State(hub): State<Arc<Hub>>,
    headers: HeaderMap,
    Json(r): Json<crate::progress::LocalReport>,
) -> Response {
    personal_response(&hub, &headers, Default::default(), None, Some(&r), false)
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
        assert_ne!(
            user_tag(PlayerId::vk(1).unwrap()),
            user_tag(PlayerId::vk(2).unwrap())
        );
    }

    #[test]
    fn client_key_uses_the_proxy_address() {
        let mut h = HeaderMap::new();
        assert_eq!(client_key(&h), "direct");
        h.insert("x-forwarded-for", "1.2.3.4, 5.6.7.8".parse().unwrap());
        assert_eq!(client_key(&h), "5.6.7.8");
        h.insert("x-forwarded-for", "2a00:1450:4010:c05::8a".parse().unwrap());
        assert_eq!(client_key(&h), "2a00:1450:4010:c05::/64");
        h.insert("x-forwarded-for", "garbage".parse().unwrap());
        assert_eq!(client_key(&h), "direct");
    }

    #[test]
    fn guest_ids_are_stable_and_never_vk_ids() {
        let key = "k".repeat(43);
        let id = guest_id(&key).unwrap();
        assert_eq!(guest_id(&key), Some(id));
        assert!(id.is_guest());
        assert_ne!(guest_id(&"j".repeat(43)), Some(id));
        assert_eq!(guest_id("short"), None);
        assert_eq!(guest_id(&format!("{}!", "k".repeat(40))), None);
    }
}
