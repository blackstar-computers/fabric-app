use dashboard::Dashboard;
use gpui::{prelude::*, px, size, App, Bounds, SharedString, WindowBounds, WindowOptions};
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
            |_, cx| {
                let dashboard = cx.new(|cx| Dashboard::new(cx));
                dashboard.update(cx, |view, cx| view.start(cx));
                dashboard
            },
        )
        .expect("open window");
        cx.activate(true);
    });
}
