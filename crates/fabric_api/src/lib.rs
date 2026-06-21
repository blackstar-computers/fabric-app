//! HTTP client for the Fabric portal — same endpoints as `web_app/src/api.ts`.

mod client;
mod credentials;
mod runtime;

pub use client::{Client, ClientError};
pub use credentials::{default_portal_url, load_service_token, save_service_token, CredentialsError};
pub use runtime::spawn as spawn_network;
