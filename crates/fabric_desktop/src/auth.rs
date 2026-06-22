//! Auth plane — Google SSO and saved service token, off the UI thread.
//!
//! Handshake runs against the SSO host (`fabric.blackstar.inc`, IAP-gated).
//! Authenticated API traffic uses the data host (`agents.fabric.blackstar.inc`, bearer-only).

use fabric_api::{
    default_portal_url, load_service_token, save_service_token, session_expired, sign_in_with_google,
    spawn_network, Client, SessionBundle,
};
use futures::channel::mpsc::UnboundedSender;

use crate::network::AppUiMsg;

#[derive(Debug)]
pub enum AuthMsg {
    Failed(String),
    Session(SessionBundle),
    ServiceToken(String),
}

fn post_auth(ui_tx: &UnboundedSender<AppUiMsg>, msg: AuthMsg) {
    if ui_tx.unbounded_send(AppUiMsg::Auth(msg)).is_err() {
        tracing::warn!("auth result dropped — UI bridge unavailable");
    }
}

pub fn spawn_google_sign_in(ui_tx: UnboundedSender<AppUiMsg>, auth_portal: String) {
    spawn_network(async move {
        let msg = match sign_in_with_google(&auth_portal).await {
            Ok(token) => {
                if session_expired(token.exp) {
                    AuthMsg::Failed("Session already expired — try again".into())
                } else {
                    let bundle = SessionBundle {
                        access_token: token.access_token,
                        portal_url: default_portal_url(),
                        email: token.email,
                        expires_at: token.exp,
                    };
                    AuthMsg::Session(bundle)
                }
            }
            Err(e) => AuthMsg::Failed(auth_error_message(&e)),
        };
        post_auth(&ui_tx, msg);
    });
}

pub fn spawn_saved_token_sign_in(ui_tx: UnboundedSender<AppUiMsg>) {
    spawn_network(async move {
        let msg = match load_service_token() {
            Ok(token) => {
                let client = Client::new(default_portal_url(), &token);
                match client.health_check().await {
                    Ok(()) => {
                        let _ = save_service_token(&token);
                        AuthMsg::ServiceToken(token)
                    }
                    Err(e) if e.is_unauthorized() => {
                        AuthMsg::Failed("Invalid service token".into())
                    }
                    Err(e) => AuthMsg::Failed(auth_error_message(&e)),
                }
            }
            Err(e) => AuthMsg::Failed(e.to_string()),
        };
        post_auth(&ui_tx, msg);
    });
}

fn auth_error_message(err: &fabric_api::ClientError) -> String {
    if err.is_unauthorized() {
        "Sign in failed — authentication rejected".into()
    } else {
        err.to_string()
    }
}
