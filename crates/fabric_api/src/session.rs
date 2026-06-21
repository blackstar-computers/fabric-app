use fabric_types::SSO_PORTAL_URL;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const KEYCHAIN_SESSION: &str = "session-bundle";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SessionBundle {
    pub access_token: String,
    pub portal_url: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub expires_at: Option<i64>,
}

impl SessionBundle {
    pub fn sso_portal_url() -> String {
        let url = std::env::var("FABRIC_SSO_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| SSO_PORTAL_URL.to_string())
            .trim_end_matches('/')
            .to_string();
        // SSO must hit the IAP-gated operator host, never the pod-facing agents host.
        if url.contains("agents.fabric.blackstar.inc") {
            return SSO_PORTAL_URL.trim_end_matches('/').to_string();
        }
        url
    }

    /// Portal base URL for authenticated API calls (IAP-off data plane when saved URL is SSO host).
    pub fn api_portal_url(&self) -> String {
        use crate::credentials::default_portal_url;
        let saved = self.portal_url.trim_end_matches('/');
        if saved == Self::sso_portal_url() {
            default_portal_url()
        } else {
            saved.to_string()
        }
    }
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("no saved session")]
    NotFound,
    #[cfg(target_os = "macos")]
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid session json")]
    Json,
}

/// Load a saved SSO session from Keychain (macOS).
pub fn load_session() -> Result<SessionBundle, SessionError> {
    #[cfg(target_os = "macos")]
    {
        use security_framework::passwords::get_generic_password;
        const KEYCHAIN_SERVICE: &str = "inc.blackstar.fabric";
        match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_SESSION) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| SessionError::Json),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("not found") || msg.contains("could not be found") {
                    Err(SessionError::NotFound)
                } else {
                    Err(SessionError::Keychain(msg))
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = SessionError::NotFound;
        Err(SessionError::NotFound)
    }
}

pub fn save_session(bundle: &SessionBundle) -> Result<(), SessionError> {
    let json = serde_json::to_vec(bundle).map_err(|_| SessionError::Json)?;
    #[cfg(target_os = "macos")]
    {
        use security_framework::passwords::{delete_generic_password, set_generic_password};
        const KEYCHAIN_SERVICE: &str = "inc.blackstar.fabric";
        let _ = delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_SESSION);
        set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_SESSION, &json)
            .map_err(|e| SessionError::Keychain(e.to_string()))?;
    }
    Ok(())
}

pub fn clear_session() -> Result<(), SessionError> {
    #[cfg(target_os = "macos")]
    {
        use security_framework::passwords::delete_generic_password;
        const KEYCHAIN_SERVICE: &str = "inc.blackstar.fabric";
        let _ = delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_SESSION);
    }
    Ok(())
}
