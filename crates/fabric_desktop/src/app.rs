//! Root shell — tab navigation, shared network, Runs + Fleets + Topology views.

use crate::auth::AuthMsg;
use crate::dashboard::Dashboard;
use crate::fleets::FleetsView;
use crate::login::LoginView;
use crate::nav::app_toolbar;
use crate::network::{spawn_app_network, AppUiMsg, NetworkCommand, NetworkHandle};
use crate::overview::config_dir;
use crate::theme::Theme;
use crate::topology::TopologyView;
use fabric_api::{
    clear_session, default_portal_url, load_service_token, load_session, save_session,
    session_expired, spawn_network, Client, SessionBundle,
};
use futures::channel::mpsc;
use futures::StreamExt;
use gpui::{div, prelude::*, px, App, Context, Entity, MouseButton, Render, Window};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tracing::warn;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Runs,
    Fleets,
    Topology,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthPhase {
    Login,
    Authenticated,
}

pub struct FabricApp {
    auth_phase: AuthPhase,
    mode: AppMode,
    login: Entity<LoginView>,
    dashboard: Entity<Dashboard>,
    fleets: Entity<FleetsView>,
    topology: Entity<TopologyView>,
    ui_tx: Option<mpsc::UnboundedSender<AppUiMsg>>,
    network: Option<NetworkHandle>,
}

const MODE_FILE: &str = "fabricApp.mode.json";

/// Startup auto-login from Keychain session or service token file.
const AUTO_LOGIN_ON_START: bool = true;

fn load_mode() -> AppMode {
    let Some(dir) = config_dir() else {
        return AppMode::Runs;
    };
    let Ok(raw) = std::fs::read_to_string(dir.join(MODE_FILE)) else {
        return AppMode::Runs;
    };
    match raw.trim().trim_matches('"') {
        "fleets" => AppMode::Fleets,
        "topology" => AppMode::Topology,
        _ => AppMode::Runs,
    }
}

fn save_mode(mode: AppMode) {
    let Some(dir) = config_dir() else {
        return;
    };
    let value = match mode {
        AppMode::Runs => "runs",
        AppMode::Fleets => "fleets",
        AppMode::Topology => "topology",
    };
    let _ = std::fs::write(dir.join(MODE_FILE), format!("\"{value}\""));
}

impl FabricApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let login = cx.new(|_| LoginView::new());
        let dashboard = cx.new(Dashboard::new);
        let fleets = cx.new(|_| FleetsView::new());
        let topology = cx.new(TopologyView::new);
        Self {
            auth_phase: AuthPhase::Login,
            mode: load_mode(),
            login,
            dashboard,
            fleets,
            topology,
            ui_tx: None,
            network: None,
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.ensure_ui_bridge(cx);
        let shell = cx.entity();
        self.dashboard.update(cx, |d, _| d.set_shell(shell));
        if let Some(ui_tx) = self.ui_tx.clone() {
            self.login.update(cx, |login, _| login.attach_ui(ui_tx));
        }

        if AUTO_LOGIN_ON_START {
            if let Ok(session) = load_session() {
                if !session_expired(session.expires_at) {
                    self.preflight_session(session, cx);
                    return;
                }
                let _ = clear_session();
            }
            if let Ok(token) = load_service_token() {
                self.preflight_service_token(token, cx);
                return;
            }
        }
        self.auth_phase = AuthPhase::Login;
        cx.notify();
    }

    fn preflight_service_token(&mut self, token: String, _cx: &mut Context<Self>) {
        let ui_tx = self.ui_tx.clone();
        spawn_network(async move {
            let client = Client::new(default_portal_url(), &token);
            let msg = match client.health_check().await {
                Ok(()) => AuthMsg::ServiceToken(token),
                Err(e) if e.is_unauthorized() => AuthMsg::Failed("Invalid service token".into()),
                Err(e) => AuthMsg::Failed(e.to_string()),
            };
            if let Some(tx) = ui_tx {
                let _ = tx.unbounded_send(AppUiMsg::Auth(msg));
            }
        });
    }

    fn preflight_session(&mut self, session: SessionBundle, _cx: &mut Context<Self>) {
        let ui_tx = self.ui_tx.clone();
        spawn_network(async move {
            let client =
                Client::with_session(session.api_portal_url(), &session.access_token);
            let msg = match client.health_check().await {
                Ok(()) => AuthMsg::Session(session),
                Err(e) if e.is_unauthorized() => {
                    let _ = clear_session();
                    AuthMsg::Failed("Session expired — sign in again".into())
                }
                Err(e) => AuthMsg::Failed(e.to_string()),
            };
            if let Some(tx) = ui_tx {
                let _ = tx.unbounded_send(AppUiMsg::Auth(msg));
            }
        });
    }

    fn ensure_ui_bridge(&mut self, cx: &mut Context<Self>) {
        if self.ui_tx.is_some() {
            return;
        }
        let (ui_tx, mut ui_rx) = mpsc::unbounded::<AppUiMsg>();
        self.ui_tx = Some(ui_tx);

        let login = self.login.clone();
        let dashboard = self.dashboard.clone();
        let fleets = self.fleets.clone();
        let topology = self.topology.clone();
        cx.spawn(async move |this, cx| {
            while let Some(msg) = ui_rx.next().await {
                let _ = this.update(cx, |app, cx| {
                    app.handle_ui_msg(msg, &login, &dashboard, &fleets, &topology, cx);
                });
            }
        })
        .detach();
    }

    fn disconnect(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.network.take() {
            handle.stop();
        }
        self.dashboard.update(cx, |d, _| d.detach());
        self.fleets.update(cx, |f, _| f.detach());
        self.topology.update(cx, |t, _| t.detach());
    }

    fn attach_data_network(&mut self, client: Client, cx: &mut Context<Self>) {
        self.disconnect(cx);
        let ui_tx = self
            .ui_tx
            .as_ref()
            .expect("ui bridge started")
            .clone();
        let shutdown = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = mpsc::unbounded::<NetworkCommand>();
        spawn_app_network(client, ui_tx, cmd_rx, shutdown.clone());
        let handle = NetworkHandle::new(shutdown, cmd_tx);
        let cmd = handle.cmd();
        self.network = Some(handle);
        self.dashboard.update(cx, |d, _| d.attach(cmd.clone()));
        self.fleets.update(cx, |f, _| f.attach(cmd.clone()));
        self.topology.update(cx, |t, _| t.attach(cmd));
    }

    fn connect_session(&mut self, session: SessionBundle, cx: &mut Context<Self>) {
        if session_expired(session.expires_at) {
            self.force_relogin("Session expired — sign in again", cx);
            return;
        }
        let email = session.email.clone();
        let client = Client::with_session(session.api_portal_url(), &session.access_token);
        self.connect(client, email, cx);
    }

    fn connect_service_token(&mut self, token: String, email: Option<String>, cx: &mut Context<Self>) {
        let client = Client::new(default_portal_url(), token);
        self.connect(client, email, cx);
    }

    fn connect(&mut self, client: Client, email: Option<String>, cx: &mut Context<Self>) {
        self.auth_phase = AuthPhase::Authenticated;
        self.attach_data_network(client, cx);
        self.dashboard
            .update(cx, |d, _| d.set_operator_email(email.clone()));
        self.fleets
            .update(cx, |f, _| f.set_operator_email(email));
        self.login.update(cx, |login, cx| login.reset(cx));

        if self.mode == AppMode::Fleets {
            self.fleets.update(cx, |f, cx| f.on_visible(cx));
        } else if self.mode == AppMode::Topology {
            self.topology.update(cx, |t, cx| t.on_visible(cx));
        }
        cx.notify();
    }

    fn force_relogin(
        &mut self,
        message: impl Into<gpui::SharedString>,
        cx: &mut Context<Self>,
    ) {
        let _ = clear_session();
        self.disconnect(cx);
        self.auth_phase = AuthPhase::Login;
        self.login
            .update(cx, |login, cx| login.on_auth_failed(message, cx));
        cx.notify();
    }

    fn handle_ui_msg(
        &mut self,
        msg: AppUiMsg,
        login: &Entity<LoginView>,
        dashboard: &Entity<Dashboard>,
        fleets: &Entity<FleetsView>,
        topology: &Entity<TopologyView>,
        cx: &mut Context<Self>,
    ) {
        match msg {
            AppUiMsg::Unauthorized(message) => {
                self.force_relogin(message, cx);
            }
            AppUiMsg::Auth(AuthMsg::Session(bundle)) => {
                let bundle_save = bundle.clone();
                spawn_network(async move {
                    if let Err(e) = save_session(&bundle_save) {
                        warn!("failed to persist session to keychain: {e}");
                    }
                });
                self.connect_session(bundle, cx);
            }
            AppUiMsg::Auth(AuthMsg::ServiceToken(token)) => {
                self.connect_service_token(token, None, cx);
            }
            AppUiMsg::Auth(AuthMsg::Failed(e)) => {
                self.auth_phase = AuthPhase::Login;
                login.update(cx, |login, cx| login.on_auth_failed(e, cx));
            }
            AppUiMsg::Dashboard(m) => {
                dashboard.update(cx, |d, cx| d.handle_msg(m, cx));
            }
            AppUiMsg::Fleets(m) => {
                fleets.update(cx, |f, cx| f.handle_msg(m, cx));
            }
            AppUiMsg::Topology(m) => {
                topology.update(cx, |t, cx| t.handle_msg(m, cx));
            }
        }
    }

    pub fn set_mode(&mut self, mode: AppMode, cx: &mut Context<Self>) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        save_mode(mode);
        if mode == AppMode::Fleets {
            self.fleets.update(cx, |f, cx| f.on_visible(cx));
        } else if mode == AppMode::Topology {
            self.topology.update(cx, |t, cx| t.on_visible(cx));
        }
        cx.notify();
    }

    pub fn open_topology_for_run(
        &mut self,
        fleet: String,
        pod: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.topology.update(cx, |t, cx| {
            t.select_from_war_room(fleet, pod, name, cx);
        });
        self.set_mode(AppMode::Topology, cx);
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if self.auth_phase != AuthPhase::Authenticated {
            return;
        }
        match self.mode {
            AppMode::Runs => {
                self.dashboard.update(cx, |d, cx| d.refresh(cx));
            }
            AppMode::Fleets => {
                self.fleets.update(cx, |f, cx| f.refresh_all(cx));
            }
            AppMode::Topology => {
                self.topology.update(cx, |t, cx| t.refresh(cx));
            }
        }
        cx.notify();
    }

    fn live_connected(&self, cx: &App) -> bool {
        match self.mode {
            AppMode::Runs => self.dashboard.read(cx).live(),
            AppMode::Fleets => self.fleets.read(cx).live,
            AppMode::Topology => false,
        }
    }

    fn refreshing(&self, cx: &App) -> bool {
        match self.mode {
            AppMode::Runs => self.dashboard.read(cx).refreshing(),
            AppMode::Fleets => self.fleets.read(cx).refreshing,
            AppMode::Topology => self.topology.read(cx).refreshing(),
        }
    }
}

impl Render for FabricApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);

        if self.auth_phase == AuthPhase::Login {
            return div().size_full().child(self.login.clone());
        }

        let mode = self.mode;
        let live = self.live_connected(cx);
        let refreshing = self.refreshing(cx);

        let refresh = theme
            .title_bar_refresh_button(refreshing)
            .id("refresh")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.refresh(cx)),
            );

        let toolbar = div()
            .flex_none()
            .flex()
            .h_full()
            .items_stretch()
            .child(refresh)
            .child(theme.title_bar_live_pill(live));

        theme
            .shell()
            .child(app_toolbar(cx, &theme, mode, toolbar))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .py(px(6.))
                    .when(mode == AppMode::Runs, |el| el.child(self.dashboard.clone()))
                    .when(mode == AppMode::Fleets, |el| el.child(self.fleets.clone()))
                    .when(mode == AppMode::Topology, |el| el.child(self.topology.clone())),
            )
    }
}
