use dashboard::Dashboard;
use fabric_api::{default_portal_url, load_service_token, Client};
use fabric_live::{LiveMessage, SseClient};
use gpui::{
    px, size, App, Bounds, Context, Entity, SharedString, Window, WindowBounds, WindowOptions,
};
use gpui_platform::application;
use tracing_subscriber::EnvFilter;

mod dashboard;
mod format;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("fabric_desktop=info".parse().unwrap()),
        )
        .init();

    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(960.), px(640.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(gpui::TitlebarOptions {
                    title: Some(SharedString::from("Fabric")),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let dashboard = cx.new(|cx| Dashboard::new(window, cx));
                bootstrap(dashboard, cx);
                dashboard
            },
        )
        .expect("open window");
        cx.activate(true);
    });
}

fn bootstrap(dashboard: Entity<Dashboard>, cx: &mut App) {
    let Ok(token) = load_service_token() else {
        dashboard.update(cx, |view, cx| {
            view.set_error(
                "No service token — run `fabric auth <token>` or set FABRIC_SERVICE_TOKEN",
                cx,
            );
        });
        return;
    };

    let client = Client::new(default_portal_url(), token);
    let mut live_rx = SseClient::spawn(client.clone());

    let dashboard_live = dashboard.clone();
    cx.spawn(async move |cx| {
        while let Some(msg) = live_rx.recv().await {
            let _ = dashboard_live.update(cx, |view, cx| {
                view.handle_live(msg, cx);
            });
        }
    })
    .detach();

    let dashboard_fetch = dashboard.clone();
    cx.spawn(async move |cx| {
        match client.fetch_runs_summary().await {
            Ok(summary) => {
                let _ = dashboard_fetch.update(cx, |view, cx| {
                    view.set_summary(summary, cx);
                });
            }
            Err(e) => {
                let _ = dashboard_fetch.update(cx, |view, cx| {
                    view.set_error(format!("{e}"), cx);
                });
            }
        }
    })
    .detach();
}
