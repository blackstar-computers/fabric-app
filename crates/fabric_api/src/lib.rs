//! HTTP client for the Fabric portal — same endpoints as `web_app/src/api.ts`.

mod auth;
mod client;
mod credentials;
mod runtime;
#[cfg(target_os = "macos")]
pub mod macos_oauth;
mod session;

pub use auth::{
    desktop_auth_url, exchange_desktop_code, login_with_service_token, parse_callback_url,
    parse_desktop_callback, sign_in_with_google, DesktopCallback,
};
pub use client::{AuthKind, Client, ClientError};
pub use credentials::{default_portal_url, load_service_token, save_service_token, CredentialsError};
pub use session::{clear_session, load_session, save_session, SessionBundle, SessionError};
pub use runtime::spawn as spawn_network;
