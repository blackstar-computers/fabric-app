//! Root shell — tab navigation, shared network, Runs + Fleets views.

use crate::dashboard::Dashboard;
use crate::fleets::FleetsView;
use crate::nav::app_toolbar;
use crate::network::{spawn_app_network, AppUiMsg, NetworkCommand};
use crate::overview::config_dir;
use crate::theme::Theme;
use fabric_api::{default_portal_url, load_service_token, Client};
use futures::channel::mpsc;
use futures::StreamExt;
use gpui::{div, prelude::*, px, App, Context, Entity, MouseButton, Render, Window};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppMode {
    Runs,
    Fleets,
}

pub struct FabricApp {
    mode: AppMode,
    dashboard: Entity<Dashboard>,
    fleets: Entity<FleetsView>,
    cmd_tx: Option<mpsc::UnboundedSender<NetworkCommand>>,
}

const MODE_FILE: &str = "fabricApp.mode.json";

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
        let dashboard = cx.new(Dashboard::new);
        let fleets = cx.new(|_| FleetsView::new());
        Self {
            mode: load_mode(),
            dashboard,
            fleets,
            cmd_tx: None,
        }
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Ok(token) = load_service_token() else {
            self.dashboard.update(cx, |d, cx| {
                d.set_error(
                    "AUTH FAIL — run `fabric auth <token>` or set FABRIC_SERVICE_TOKEN",
                    cx,
                );
            });
            return;
        };

        let client = Client::new(default_portal_url(), token);
        let (ui_tx, mut ui_rx) = mpsc::unbounded::<AppUiMsg>();
        let (cmd_tx, cmd_rx) = mpsc::unbounded::<NetworkCommand>();
        self.cmd_tx = Some(cmd_tx.clone());

        self.dashboard.update(cx, |d, _| d.attach(cmd_tx.clone()));
        self.fleets.update(cx, |f, _| f.attach(cmd_tx));

        spawn_app_network(client, ui_tx, cmd_rx);

        let dashboard = self.dashboard.clone();
        let fleets = self.fleets.clone();
        cx.spawn(async move |this, cx| {
            while let Some(msg) = ui_rx.next().await {
                let _ = this.update(cx, |app, cx| {
                    app.dispatch(msg, &dashboard, &fleets, cx);
                });
            }
        })
        .detach();

        if self.mode == AppMode::Fleets {
            self.fleets.update(cx, |f, cx| f.on_visible(cx));
        }
    }

    fn dispatch(
        &mut self,
        msg: AppUiMsg,
        dashboard: &Entity<Dashboard>,
        fleets: &Entity<FleetsView>,
        cx: &mut Context<Self>,
    ) {
        match msg {
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

impl Render for FabricApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
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
