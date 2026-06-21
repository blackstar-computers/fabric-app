//! Root shell — tab navigation, shared network, Runs + Fleets views.

use crate::auth::AuthMsg;
use crate::dashboard::Dashboard;
use crate::fleets::FleetsView;
use crate::login::LoginView;
use crate::nav::app_toolbar;
use crate::network::{spawn_app_network, AppUiMsg, NetworkCommand};
use crate::overview::config_dir;
use crate::theme::Theme;
use fabric_api::{
    default_portal_url, load_service_token, load_session, save_session, Client, SessionBundle,
};
use futures::channel::mpsc;
use futures::StreamExt;
use gpui::{div, prelude::*, px, App, Context, Entity, MouseButton, Render, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Runs,
    Fleets,
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
    ui_tx: Option<mpsc::UnboundedSender<AppUiMsg>>,
    cmd_tx: Option<mpsc::UnboundedSender<NetworkCommand>>,
}

const MODE_FILE: &str = "fabricApp.mode.json";

/// Startup auto-login from Keychain session or service token file. Off while testing SSO.
const AUTO_LOGIN_ON_START: bool = false;

fn load_mode() -> AppMode {
    let Some(dir) = config_dir() else {
        return AppMode::Runs;
    };
    let Ok(raw) = std::fs::read_to_string(dir.join(MODE_FILE)) else {
        return AppMode::Runs;
    };
    match raw.trim().trim_matches('"') {
        "fleets" => AppMode::Fleets,
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
    };
    let _ = std::fs::write(dir.join(MODE_FILE), format!("\"{value}\""));
}

impl FabricApp {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let login = cx.new(|_| LoginView::new());
        let dashboard = cx.new(Dashboard::new);
        let fleets = cx.new(|_| FleetsView::new());
        Self {
            auth_phase: AuthPhase::Login,
            mode: load_mode(),
            login,
            dashboard,
            fleets,
            ui_tx: None,
            cmd_tx: None,
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        self.ensure_ui_bridge(cx);
        if let Some(ui_tx) = self.ui_tx.clone() {
            self.login.update(cx, |login, _| login.attach_ui(ui_tx));
        }

        if AUTO_LOGIN_ON_START {
            if let Ok(session) = load_session() {
                if !session_expired(session.expires_at) {
                    self.connect_session(session, cx);
                    return;
                }
            }
            if let Ok(token) = load_service_token() {
                self.connect_service_token(token, None, cx);
                return;
            }
        }
        self.auth_phase = AuthPhase::Login;
        cx.notify();
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
        cx.spawn(async move |this, cx| {
            while let Some(msg) = ui_rx.next().await {
                let _ = this.update(cx, |app, cx| {
                    app.handle_ui_msg(msg, &login, &dashboard, &fleets, cx);
                });
            }
        })
        .detach();
    }

    fn attach_data_network(&mut self, client: Client, cx: &mut Context<Self>) {
        if self.cmd_tx.is_some() {
            return;
        }
        let ui_tx = self
            .ui_tx
            .as_ref()
            .expect("ui bridge started")
            .clone();
        let (cmd_tx, cmd_rx) = mpsc::unbounded::<NetworkCommand>();
        self.cmd_tx = Some(cmd_tx.clone());
        self.dashboard.update(cx, |d, _| d.attach(cmd_tx.clone()));
        self.fleets.update(cx, |f, _| f.attach(cmd_tx));
        spawn_app_network(client, ui_tx, cmd_rx);
    }

    fn connect_session(&mut self, session: SessionBundle, cx: &mut Context<Self>) {
        let email = session.email.clone();
        let client = Client::with_session(session.api_portal_url(), &session.access_token);
        self.connect(client, email, cx);
    }

    fn connect_service_token(&mut self, token: String, email: Option<String>, cx: &mut Context<Self>) {
        let client = Client::new(default_portal_url(), token);
        self.connect(client, email, cx);
    }

    fn connect(&mut self, client: Client, email: Option<String>, cx: &mut Context<Self>) {
        if self.auth_phase == AuthPhase::Authenticated && self.cmd_tx.is_some() {
            return;
        }
        self.auth_phase = AuthPhase::Authenticated;
        self.attach_data_network(client, cx);
        self.dashboard
            .update(cx, |d, _| d.set_operator_email(email.clone()));
        self.fleets
            .update(cx, |f, _| f.set_operator_email(email));

        if self.mode == AppMode::Fleets {
            self.fleets.update(cx, |f, cx| f.on_visible(cx));
        }
        cx.notify();
    }

    fn handle_ui_msg(
        &mut self,
        msg: AppUiMsg,
        login: &Entity<LoginView>,
        dashboard: &Entity<Dashboard>,
        fleets: &Entity<FleetsView>,
        cx: &mut Context<Self>,
    ) {
        match msg {
            AppUiMsg::Auth(AuthMsg::Session(bundle)) => {
                let _ = save_session(&bundle);
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
        }
        cx.notify();
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
        }
        cx.notify();
    }

    fn live_connected(&self, cx: &App) -> bool {
        self.dashboard.read(cx).live()
    }

    fn refreshing(&self, cx: &App) -> bool {
        match self.mode {
            AppMode::Runs => self.dashboard.read(cx).refreshing(),
            AppMode::Fleets => self.fleets.read(cx).refreshing,
        }
    }
}

fn session_expired(expires_at: Option<i64>) -> bool {
    let Some(exp) = expires_at else {
        return false;
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    exp <= now
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
                    .when(mode == AppMode::Fleets, |el| el.child(self.fleets.clone())),
            )
    }
}
