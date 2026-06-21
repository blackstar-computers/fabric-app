//! Portal session auth types (desktop SSO + break-glass service token).

use serde::{Deserialize, Serialize};

pub const SSO_PORTAL_URL: &str = "https://fabric.blackstar.inc";
pub const AUTH_CALLBACK_SCHEME: &str = "fabric";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TokenExchangeRequest {
    pub code: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub exp: Option<i64>,
    #[serde(default)]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionLoginRequest {
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionLoginResponse {
    pub ok: bool,
    #[serde(default)]
    pub exp: Option<i64>,
    #[serde(default)]
    pub ttl: Option<i64>,
    #[serde(default)]
    pub access_token: Option<String>,
}
