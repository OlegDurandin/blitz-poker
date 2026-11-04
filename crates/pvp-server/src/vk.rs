//! VK Mini Apps launch parameter verification.
//!
//! VK appends `vk_*` parameters and a `sign` to the app URL. The signature is
//! HMAC-SHA256 (key: the app's protected key, kept on the server only) over
//! the `vk_*` parameters sorted by name and joined as `key=value&...` with
//! values percent-encoded like JavaScript's `encodeURIComponent`, then
//! base64url without padding. A `vk_user_id` is trusted only after this check.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VkLaunch {
    pub user_id: u64,
    pub app_id: u64,
    /// `vk_ts`: when VK signed the parameters (unix seconds).
    pub ts: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LaunchError {
    MissingSign,
    BadSign,
    WrongApp,
    Expired,
    Malformed(&'static str),
}

/// Same output as JavaScript's `encodeURIComponent`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Signature over the `vk_*` parameters of an already-decoded parameter list.
pub fn sign(params: &[(String, String)], secret: &[u8]) -> String {
    let mut vk: Vec<&(String, String)> = params
        .iter()
        .filter(|(k, _)| k.starts_with("vk_"))
        .collect();
    vk.sort_by(|a, b| a.0.cmp(&b.0));
    let payload = vk
        .iter()
        .map(|(k, v)| format!("{k}={}", encode_uri_component(v)))
        .collect::<Vec<_>>()
        .join("&");
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC takes any key length");
    mac.update(payload.as_bytes());
    URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Verifies a launch query string (with or without a leading `?`).
pub fn verify(
    query: &str,
    secret: &[u8],
    expected_app_id: u64,
    max_age_secs: u64,
    now_unix: u64,
) -> Result<VkLaunch, LaunchError> {
    let params: Vec<(String, String)> =
        form_urlencoded::parse(query.trim_start_matches('?').as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
    let given = params
        .iter()
        .find(|(k, _)| k == "sign")
        .map(|(_, v)| v.as_str())
        .ok_or(LaunchError::MissingSign)?;
    if !constant_time_eq(sign(&params, secret).as_bytes(), given.as_bytes()) {
        return Err(LaunchError::BadSign);
    }
    let get = |name: &'static str| -> Result<u64, LaunchError> {
        params
            .iter()
            .find(|(k, _)| k == name)
            .and_then(|(_, v)| v.parse().ok())
            .ok_or(LaunchError::Malformed(name))
    };
    let launch = VkLaunch {
        user_id: get("vk_user_id")?,
        app_id: get("vk_app_id")?,
        ts: get("vk_ts")?,
    };
    if launch.app_id != expected_app_id {
        return Err(LaunchError::WrongApp);
    }
    if launch.user_id == 0 {
        return Err(LaunchError::Malformed("vk_user_id"));
    }
    if now_unix.saturating_sub(launch.ts) > max_age_secs {
        return Err(LaunchError::Expired);
    }
    Ok(launch)
}

/// Builds a signed query string; used by tests and the local dev client.
pub fn signed_query(params: &[(&str, &str)], secret: &[u8]) -> String {
    let owned: Vec<(String, String)> = params
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let mut q = form_urlencoded::Serializer::new(String::new());
    for (k, v) in &owned {
        q.append_pair(k, v);
    }
    q.append_pair("sign", &sign(&owned, secret));
    q.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret";

    fn launch(user: &str, ts: &str) -> String {
        signed_query(
            &[
                ("vk_user_id", user),
                ("vk_app_id", "54872714"),
                ("vk_ts", ts),
                ("vk_platform", "mobile_iphone"),
                ("vk_language", "ru"),
                // Non-ASCII and reserved characters must be encoded like encodeURIComponent.
                ("vk_ref", "other/с пробелом&x=1"),
                ("utm_source", "not-signed"),
            ],
            SECRET,
        )
    }

    #[test]
    fn accepts_valid_and_rejects_tampered() {
        let q = launch("777", "1000");
        assert_eq!(
            verify(&q, SECRET, 54872714, 3600, 1500),
            Ok(VkLaunch {
                user_id: 777,
                app_id: 54872714,
                ts: 1000
            })
        );
        let forged = q.replace("vk_user_id=777", "vk_user_id=778");
        assert_eq!(
            verify(&forged, SECRET, 54872714, 3600, 1500),
            Err(LaunchError::BadSign)
        );
        assert_eq!(
            verify(&q, b"other", 54872714, 3600, 1500),
            Err(LaunchError::BadSign)
        );
        let unsigned = "vk_user_id=777&vk_app_id=54872714&vk_ts=1000";
        assert_eq!(
            verify(unsigned, SECRET, 54872714, 3600, 1500),
            Err(LaunchError::MissingSign)
        );
        // Parameters outside vk_* are not signed and may change freely.
        let extra = q.replace("utm_source=not-signed", "utm_source=changed");
        assert!(verify(&extra, SECRET, 54872714, 3600, 1500).is_ok());
    }

    #[test]
    fn checks_app_and_age() {
        let q = launch("777", "1000");
        assert_eq!(
            verify(&q, SECRET, 1, 3600, 1500),
            Err(LaunchError::WrongApp)
        );
        assert_eq!(
            verify(&q, SECRET, 54872714, 3600, 9000),
            Err(LaunchError::Expired)
        );
        assert!(verify(&format!("?{q}"), SECRET, 54872714, 3600, 1500).is_ok());
    }

    #[test]
    fn encodes_like_javascript() {
        assert_eq!(encode_uri_component("a b/ц!~*'()"), "a%20b%2F%D1%86!~*'()");
    }
}
