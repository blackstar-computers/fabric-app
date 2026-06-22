//! Portal auth — session exchange and break-glass service-token login.

use crate::session::SessionBundle;
use crate::ClientError;
use fabric_types::{SessionLoginRequest, SessionLoginResponse, TokenExchangeRequest, TokenResponse};
use reqwest::StatusCode;
use serde_json::Value;

pub async fn exchange_desktop_code(
    base_url: &str,
    req: &TokenExchangeRequest,
) -> Result<TokenResponse, ClientError> {
    post_json(base_url, "/api/auth/token", req).await
}

pub async fn login_with_service_token(
    base_url: &str,
    token: &str,
) -> Result<SessionLoginResponse, ClientError> {
    post_json(
        base_url,
        "/api/auth/session",
        &SessionLoginRequest {
            token: token.to_string(),
        },
    )
    .await
}

async fn post_json<T: serde::de::DeserializeOwned>(
    base_url: &str,
    path: &str,
    body: &impl serde::Serialize,
) -> Result<T, ClientError> {
    let url = format!("{}{}", base_url.trim_end_matches('/'), path);
    let http = reqwest::Client::builder()
        .user_agent("fabric-app/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(ClientError::Http)?;
    let response = http
        .post(&url)
        .header("Accept", "application/json")
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or_else(|_| {
        serde_json::json!({ "error": "non-JSON response from portal" })
    });
    if !status.is_success() {
        let message = body
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("request failed")
            .to_string();
        if status == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        return Err(ClientError::Api { status, message });
    }
    Ok(serde_json::from_value(body)?)
}

/// Parsed `fabric://auth/callback` query (direct token or legacy one-time code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopCallback {
    pub state: String,
    pub access_token: Option<String>,
    pub code: Option<String>,
    pub email: Option<String>,
    pub exp: Option<i64>,
}

pub fn parse_desktop_callback(url: &str) -> Option<DesktopCallback> {
    let rest = url.strip_prefix("fabric://auth/callback?")?;
    let mut state = None;
    let mut access_token = None;
    let mut code = None;
    let mut email = None;
    let mut exp = None;
    for part in rest.split('&') {
        if part.is_empty() {
            continue;
        }
        let (key, val) = part.split_once('=')?;
        let val = decode_query_component(val);
        match key {
            "state" => state = Some(val),
            "access_token" => access_token = Some(val),
            "code" => code = Some(val),
            "email" => email = Some(val).filter(|s| !s.is_empty()),
            "exp" => exp = val.parse().ok(),
            _ => {}
        }
    }
    Some(DesktopCallback {
        state: state?,
        access_token,
        code,
        email,
        exp,
    })
}

pub fn parse_callback_url(url: &str) -> Option<(String, String)> {
    parse_desktop_callback(url).and_then(|cb| Some((cb.code?, cb.state)))
}

/// SSO entry URL — uses deployed `/login` (IAP-exempt) rather than `/auth/desktop`.
pub fn desktop_auth_url(portal_url: &str, state: &str) -> String {
    format!(
        "{}/login?desktop=1&state={}",
        portal_url.trim_end_matches('/'),
        url_encode(state)
    )
}

fn random_oauth_state() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{}-{nanos}", std::process::id())
}

/// In-app Google SSO: ASWebAuthenticationSession → fabric:// callback with bearer token.
pub async fn sign_in_with_google(auth_portal: &str) -> Result<TokenResponse, ClientError> {
    let state = random_oauth_state();
    let auth_url = desktop_auth_url(auth_portal, &state);

    #[cfg(target_os = "macos")]
    {
        let callback = tokio::task::spawn_blocking(move || {
            crate::macos_oauth::authenticate_in_app(&auth_url)
        })
        .await
        .map_err(|e| ClientError::auth(e.to_string()))?;

        let url = callback.map_err(|e| ClientError::auth(e.to_string()))?;
        let cb = parse_desktop_callback(&url)
            .ok_or_else(|| ClientError::bad_request("invalid auth callback"))?;
        if cb.state != state {
            return Err(ClientError::bad_request("auth state mismatch"));
        }
        if let Some(access_token) = cb.access_token {
            return Ok(TokenResponse {
                access_token,
                token_type: Some("session".into()),
                exp: cb.exp,
                email: cb.email,
            });
        }
        if let Some(code) = cb.code {
            let exchange_url = SessionBundle::api_portal_url_for_sso_host(auth_portal);
            return exchange_desktop_code(
                &exchange_url,
                &TokenExchangeRequest {
                    code,
                    state: cb.state,
                },
            )
            .await;
        }
        Err(ClientError::bad_request("auth callback missing token"))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = (auth_portal, auth_url, state);
        Err(ClientError::auth("Google SSO requires macOS"))
    }
}

fn decode_query_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(v) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(v as char);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_code_callback() {
        let cb = parse_desktop_callback("fabric://auth/callback?code=abc&state=xyz").unwrap();
        assert_eq!(cb.code.as_deref(), Some("abc"));
        assert_eq!(cb.state, "xyz");
    }

    #[test]
    fn parses_direct_token_callback() {
        let cb = parse_desktop_callback(
            "fabric://auth/callback?access_token=tok123&state=xyz&email=a%40b.co&exp=99",
        )
        .unwrap();
        assert_eq!(cb.access_token.as_deref(), Some("tok123"));
        assert_eq!(cb.state, "xyz");
        assert_eq!(cb.email.as_deref(), Some("a@b.co"));
        assert_eq!(cb.exp, Some(99));
    }

    #[test]
    fn rejects_callback_without_state() {
        assert!(parse_desktop_callback("fabric://auth/callback?code=abc").is_none());
    }

    #[test]
    fn rejects_wrong_scheme() {
        assert!(parse_desktop_callback("https://example.com?state=x").is_none());
    }

    #[test]
    fn desktop_auth_url_uses_login() {
        assert!(desktop_auth_url("https://fabric.blackstar.inc", "s1").contains("/login?desktop=1"));
    }
}
