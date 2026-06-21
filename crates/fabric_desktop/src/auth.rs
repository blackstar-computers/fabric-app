//! Auth plane — Google SSO and saved service token, off the UI thread.
//!
//! Handshake runs against the SSO host (`fabric.blackstar.inc`, IAP-gated).
//! Authenticated API traffic uses the data host (`agents.fabric.blackstar.inc`, bearer-only).

use fabric_api::{
    default_portal_url, load_service_token, save_service_token, sign_in_with_google,
    spawn_network, SessionBundle,
};
use futures::channel::mpsc::UnboundedSender;

use crate::network::AppUiMsg;

#[derive(Debug)]
pub enum AuthMsg {
    Failed(String),
    Session(SessionBundle),
    ServiceToken(String),
}

pub fn spawn_google_sign_in(ui_tx: UnboundedSender<AppUiMsg>, auth_portal: String) {
    spawn_network(async move {
        let msg = match sign_in_with_google(&auth_portal).await {
            Ok(token) => {
                let bundle = SessionBundle {
                    access_token: token.access_token,
                    portal_url: default_portal_url(),
                    email: token.email,
                    expires_at: token.exp,
                };
                AuthMsg::Session(bundle)
            }
            Err(e) => AuthMsg::Failed(e.to_string()),
        };
        let _ = ui_tx.unbounded_send(AppUiMsg::Auth(msg));
    });
}

pub fn spawn_saved_token_sign_in(ui_tx: UnboundedSender<AppUiMsg>) {
    spawn_network(async move {
        let msg = match load_service_token() {
            Ok(token) => {
                let _ = save_service_token(&token);
                AuthMsg::ServiceToken(token)
            }
            Err(e) => AuthMsg::Failed(e.to_string()),
        };
        let _ = ui_tx.unbounded_send(AppUiMsg::Auth(msg));
    });
}
