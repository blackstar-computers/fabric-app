use std::borrow::Cow;

use app::FabricApp;
use assets::Assets;
use gpui::{prelude::*, px, size, App, Bounds, TitlebarOptions, WindowBounds, WindowOptions};
use gpui_platform::application;
use theme::Theme;
use tracing_subscriber::EnvFilter;

mod app;
mod assets;
mod brand;
mod charts;
mod columns;
mod dashboard;
mod detail;
mod fleet_canvas;
mod fleet_layout;
mod fleet_ops;
mod fleets;
mod format;
mod network;
mod nav;
mod overview;
mod search_input;
mod sparkline;
mod theme;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("fabric_desktop=info".parse().unwrap()),
        )
        .init();

    application()
        .with_assets(Assets::new())
        .run(|cx: &mut App| {
            let fonts = [include_bytes!("../assets/fonts/SpaceGrotesk-Bold.ttf")]
                .iter()
                .map(|bytes| Cow::Borrowed(&bytes[..]))
                .collect();
            _ = cx.text_system().add_fonts(fonts);

            Theme::init(cx);
            let bounds = Bounds::centered(None, size(px(1200.), px(760.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: None,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |_, cx| {
                    let app = cx.new(FabricApp::new);
                    app.update(cx, |view, cx| view.start(cx));
                    app
                },
            )
            .expect("open window");
            cx.activate(true);
        });
}
