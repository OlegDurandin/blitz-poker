//! Server configuration from the environment (a local `.env` fills gaps).

use crate::room::Timings;
use std::net::SocketAddr;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    pub app_id: u64,
    pub app_secret: String,
    pub db_path: String,
    /// Allowed `Origin`s: exact values, or suffixes starting with a dot
    /// (".vk-apps.ru" allows every VK hosting subdomain over https).
    pub allowed_origins: Vec<String>,
    pub launch_max_age_secs: u64,
    /// Accept `{"dev_user_id": …}` instead of VK parameters. Only honoured
    /// when bound to a loopback address.
    pub dev_auth: bool,
    /// Players outside VK may join a room by its code (`PVP_GUEST_AUTH=1`).
    pub guest_auth: bool,
    pub timings: Timings,
}

impl Config {
    pub fn from_env() -> Result<Config, String> {
        load_dotenv(".env");
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let num = |k: &str, default: u64| -> Result<u64, String> {
            var(k).map_or(Ok(default), |v| {
                v.trim()
                    .parse()
                    .map_err(|_| format!("{k} must be a number"))
            })
        };
        let bind: SocketAddr = var("PVP_BIND")
            .unwrap_or_else(|| "127.0.0.1:8787".into())
            .parse()
            .map_err(|_| "PVP_BIND must be host:port")?;
        let dev_auth = var("PVP_DEV_AUTH").is_some_and(|v| v == "1") && bind.ip().is_loopback();
        let app_secret = var("VK_APP_SECRET").unwrap_or_default();
        if app_secret.is_empty() && !dev_auth {
            return Err("VK_APP_SECRET is required".into());
        }
        let defaults = Timings::default();
        Ok(Config {
            bind,
            app_id: num("VK_APP_ID", 0)?,
            app_secret,
            db_path: var("PVP_DB").unwrap_or_else(|| "pvp.sqlite".into()),
            allowed_origins: var("PVP_ALLOWED_ORIGINS")
                .unwrap_or_else(|| ".vk-apps.ru,.vk-apps.com,http://localhost:5173".into())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            launch_max_age_secs: num("PVP_LAUNCH_MAX_AGE_HOURS", 24)? * 3600,
            dev_auth,
            guest_auth: var("PVP_GUEST_AUTH").is_some_and(|v| v.trim() == "1"),
            timings: Timings {
                turn_ms: num("PVP_TURN_SECONDS", defaults.turn_ms / 1000)? * 1000,
                bank_ms: num("PVP_BANK_SECONDS", defaults.bank_ms / 1000)? * 1000,
                grace_ms: num("PVP_GRACE_SECONDS", defaults.grace_ms / 1000)? * 1000,
                room_ttl_ms: num("PVP_ROOM_TTL_MINUTES", defaults.room_ttl_ms / 60_000)? * 60_000,
                ..defaults
            },
        })
    }

    pub fn origin_allowed(&self, origin: &str) -> bool {
        origin_allowed(&self.allowed_origins, origin)
    }
}

pub fn origin_allowed(allowed: &[String], origin: &str) -> bool {
    allowed.iter().any(|a| {
        if let Some(suffix) = a.strip_prefix('.') {
            origin.strip_prefix("https://").is_some_and(|host| {
                host.ends_with(&format!(".{suffix}")) && !host.contains(['/', '@'])
            })
        } else {
            a == origin
        }
    })
}

/// Minimal `.env` reader: `KEY=VALUE` lines; existing variables win.
fn load_dotenv(path: &str) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let (k, v) = (k.trim(), v.trim().trim_matches('"'));
            if !k.is_empty() && std::env::var_os(k).is_none() {
                // SAFETY: called once at startup, before any threads exist.
                unsafe { std::env::set_var(k, v) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        let allowed: Vec<String> = [".vk-apps.ru", "http://localhost:5173"]
            .map(String::from)
            .to_vec();
        assert!(origin_allowed(
            &allowed,
            "https://prod-app54872714-abc.pages-ac.vk-apps.ru"
        ));
        assert!(origin_allowed(&allowed, "http://localhost:5173"));
        assert!(
            !origin_allowed(&allowed, "http://prod-app1.pages.vk-apps.ru"),
            "https only"
        );
        assert!(!origin_allowed(&allowed, "https://evil-vk-apps.ru"));
        assert!(!origin_allowed(&allowed, "https://vk-apps.ru.evil.com"));
        assert!(!origin_allowed(&allowed, "http://localhost:5174"));
    }
}
