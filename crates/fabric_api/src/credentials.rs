use fabric_types::DEFAULT_PORTAL_URL;
use std::path::{Path, PathBuf};
use thiserror::Error;

const KEYCHAIN_SERVICE: &str = "inc.blackstar.fabric";
const KEYCHAIN_ACCOUNT: &str = "service-token";
const USER_TOKEN_FILE: &str = ".config/fabric/service_token";
const VM_TOKEN_FILE: &str = "/opt/fabric/.fleet_service_token";

#[derive(Debug, Error)]
pub enum CredentialsError {
    #[error("no service token configured — run `fabric auth <token>` or set FABRIC_SERVICE_TOKEN")]
    NotConfigured,
    #[cfg(target_os = "macos")]
    #[error("keychain error: {0}")]
    Keychain(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Resolve the portal service token (same order as `fleet/service_token.py` + Keychain on macOS).
pub fn load_service_token() -> Result<String, CredentialsError> {
    if let Ok(tok) = std::env::var("FABRIC_SERVICE_TOKEN") {
        let tok = tok.trim().to_string();
        if !tok.is_empty() {
            return Ok(tok);
        }
    }

    if let Some((tok, source)) = read_first_token_file() {
        let canonical = canonical_user_token_path();
        if canonical.as_ref() != source.as_ref() {
            let _ = save_service_token(&tok);
        }
        return Ok(tok);
    }

    #[cfg(target_os = "macos")]
    if let Ok(tok) = read_keychain() {
        if !tok.is_empty() {
            return Ok(tok);
        }
    }

    Err(CredentialsError::NotConfigured)
}

/// Persist token to macOS Keychain (preferred) and the CLI-compatible user file.
pub fn save_service_token(token: &str) -> Result<(), CredentialsError> {
    let token = token.trim();
    if token.is_empty() {
        return Err(CredentialsError::NotConfigured);
    }

    #[cfg(target_os = "macos")]
    write_keychain(token)?;

    if let Some(path) = user_token_path() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, format!("{token}\n"))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
    }

    Ok(())
}

pub fn default_portal_url() -> String {
    std::env::var("FABRIC_PORTAL_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_PORTAL_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

fn user_token_path() -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(USER_TOKEN_FILE))
}

fn canonical_user_token_path() -> Option<PathBuf> {
    user_token_path()
}

/// Same read order as `fleet/service_token.py::_candidate_files`, plus common alternates.
fn candidate_token_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(f) = std::env::var("FABRIC_SERVICE_TOKEN_FILE") {
        if !f.trim().is_empty() {
            paths.push(PathBuf::from(f));
        }
    }
    if let Some(home) = std::env::var("HOME").ok().map(PathBuf::from) {
        paths.push(home.join(USER_TOKEN_FILE));
        paths.push(home.join(".config/fabric/service_token.bak"));
    }
    paths.push(PathBuf::from(VM_TOKEN_FILE));
    paths
}

fn read_first_token_file() -> Option<(String, Option<PathBuf>)> {
    for path in candidate_token_paths() {
        if let Ok(tok) = read_token_file(&path) {
            if !tok.is_empty() {
                return Some((tok, Some(path)));
            }
        }
    }
    None
}

fn read_token_file(path: &Path) -> Result<String, std::io::Error> {
    if !path.exists() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "token file missing",
        ));
    }
    let tok = std::fs::read_to_string(path)?.trim().to_string();
    Ok(tok)
}

#[cfg(target_os = "macos")]
fn read_keychain() -> Result<String, CredentialsError> {
    use security_framework::passwords::get_generic_password;
    match get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT) {
        Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).trim().to_string()),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not found") || msg.contains("could not be found") {
                Ok(String::new())
            } else {
                Err(CredentialsError::Keychain(msg))
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn write_keychain(token: &str) -> Result<(), CredentialsError> {
    use security_framework::passwords::{delete_generic_password, set_generic_password};
    let _ = delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
    set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, token.as_bytes())
        .map_err(|e| CredentialsError::Keychain(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_portal_url_points_at_agents_host() {
        std::env::remove_var("FABRIC_PORTAL_URL");
        assert_eq!(default_portal_url(), DEFAULT_PORTAL_URL);
    }

    #[test]
    fn candidate_paths_include_user_file_and_bak() {
        let paths = candidate_token_paths();
        let home = std::env::var("HOME").unwrap();
        let home_path = PathBuf::from(&home);
        assert!(paths.contains(&home_path.join(USER_TOKEN_FILE)));
        assert!(paths.contains(&home_path.join(".config/fabric/service_token.bak")));
    }
}
