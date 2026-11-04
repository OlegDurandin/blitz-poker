//! Telegram Mini Apps launch data (`initData`) verification.
//!
//! Telegram hands the Mini App `initData`: a query string with `user` (JSON),
//! `auth_date`, optional `start_param`, and `hash`. The hash is
//! HMAC-SHA256 over the other fields sorted by name and joined as
//! `key=value` lines, keyed with HMAC-SHA256("WebAppData", bot token); hex.
//! Only the server knows the bot token, so a verified `user.id` is trusted.

use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgLaunch {
    pub user_id: u64,
    /// "First Last", as Telegram shows it; signed, so better than client text.
    pub name: String,
    pub photo_url: Option<String>,
    /// When Telegram signed the data (unix seconds).
    pub auth_date: u64,
    /// `startapp` of the link that opened the app (e.g. an invite).
    pub start_param: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum TgError {
    MissingHash,
    BadHash,
    Expired,
    Malformed(&'static str),
}

#[derive(Deserialize)]
struct TgUser {
    id: u64,
    first_name: Option<String>,
    last_name: Option<String>,
    photo_url: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Hex hash over decoded fields (everything except `hash`).
pub fn sign(fields: &[(String, String)], bot_token: &str) -> String {
    let mut lines: Vec<String> = fields
        .iter()
        .filter(|(k, _)| k != "hash")
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    lines.sort();
    let mut secret = Hmac::<Sha256>::new_from_slice(b"WebAppData").expect("any key length");
    secret.update(bot_token.as_bytes());
    let key = secret.finalize().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("any key length");
    mac.update(lines.join("\n").as_bytes());
    hex(&mac.finalize().into_bytes())
}

/// Verifies `initData` (`Telegram.WebApp.initData`, a query string).
pub fn verify(
    init_data: &str,
    bot_token: &str,
    max_age_secs: u64,
    now_unix: u64,
) -> Result<TgLaunch, TgError> {
    let fields: Vec<(String, String)> = form_urlencoded::parse(init_data.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let get = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let given = get("hash").ok_or(TgError::MissingHash)?;
    if !constant_time_eq(sign(&fields, bot_token).as_bytes(), given.as_bytes()) {
        return Err(TgError::BadHash);
    }
    let auth_date: u64 = get("auth_date")
        .and_then(|v| v.parse().ok())
        .ok_or(TgError::Malformed("auth_date"))?;
    if now_unix.saturating_sub(auth_date) > max_age_secs {
        return Err(TgError::Expired);
    }
    let user: TgUser = get("user")
        .and_then(|u| serde_json::from_str(u).ok())
        .ok_or(TgError::Malformed("user"))?;
    if user.id == 0 {
        return Err(TgError::Malformed("user.id"));
    }
    let name = [user.first_name, user.last_name]
        .into_iter()
        .flatten()
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(TgLaunch {
        user_id: user.id,
        name,
        photo_url: user.photo_url,
        auth_date,
        start_param: get("start_param").map(str::to_string),
    })
}

/// Builds signed `initData`; used by tests and local development.
pub fn signed_init_data(fields: &[(&str, &str)], bot_token: &str) -> String {
    let owned: Vec<(String, String)> = fields
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let mut q = form_urlencoded::Serializer::new(String::new());
    for (k, v) in &owned {
        q.append_pair(k, v);
    }
    q.append_pair("hash", &sign(&owned, bot_token));
    q.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "123456:test-token";
    const USER: &str = r#"{"id":777000,"first_name":"Маша","last_name":"К","photo_url":"https://t.me/i/userpic/320/x.jpg"}"#;

    fn data(auth_date: &str) -> String {
        signed_init_data(
            &[
                ("query_id", "AAE"),
                ("user", USER),
                ("auth_date", auth_date),
                ("start_param", "room_abc"),
            ],
            TOKEN,
        )
    }

    #[test]
    fn accepts_signed_data_and_reads_the_user() {
        let l = verify(&data("1000"), TOKEN, 3600, 1500).unwrap();
        assert_eq!(l.user_id, 777000);
        assert_eq!(l.name, "Маша К");
        assert_eq!(
            l.photo_url.as_deref(),
            Some("https://t.me/i/userpic/320/x.jpg")
        );
        assert_eq!(l.start_param.as_deref(), Some("room_abc"));
    }

    #[test]
    fn rejects_forgery_other_bots_and_old_data() {
        let good = data("1000");
        let forged = good.replace("777000", "777001");
        assert_eq!(verify(&forged, TOKEN, 3600, 1500), Err(TgError::BadHash));
        assert_eq!(
            verify(&good, "999:other", 3600, 1500),
            Err(TgError::BadHash)
        );
        assert_eq!(verify(&good, TOKEN, 3600, 10_000), Err(TgError::Expired));
        let no_hash = good.split("&hash=").next().unwrap().to_string();
        assert_eq!(
            verify(&no_hash, TOKEN, 3600, 1500),
            Err(TgError::MissingHash)
        );
    }

    /// The data-check string Telegram documents: fields sorted by key, one
    /// `key=value` per line, `hash` excluded; extra fields (e.g. `signature`)
    /// are part of it.
    #[test]
    fn hash_covers_every_field_but_hash() {
        let a = sign(&[("b".into(), "2".into()), ("a".into(), "1".into())], TOKEN);
        let b = sign(
            &[
                ("a".into(), "1".into()),
                ("hash".into(), "x".into()),
                ("b".into(), "2".into()),
            ],
            TOKEN,
        );
        assert_eq!(a, b);
        let c = sign(
            &[
                ("a".into(), "1".into()),
                ("b".into(), "2".into()),
                ("signature".into(), "s".into()),
            ],
            TOKEN,
        );
        assert_ne!(a, c);
    }
}
