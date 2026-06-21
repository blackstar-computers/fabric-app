//! Mode tabs + toolbar — sits below the native title bar (straight edges, no traffic-light overlap).

use gpui::{div, prelude::*, px, Context, Div, MouseButton, MouseDownEvent, Pixels};

use crate::app::{AppMode, FabricApp};
use crate::brand::brand;
use crate::theme::Theme;

const TAB_GAP: Pixels = px(1.);
const TOOLBAR_H: Pixels = px(28.);

/// App chrome row: tabs on the left, refresh/live on the right.
pub fn app_toolbar(
    cx: &mut Context<FabricApp>,
    theme: &Theme,
    mode: AppMode,
    right: impl IntoElement,
) -> impl IntoElement {
    div()
        .id("app-toolbar")
        .flex_none()
        .w_full()
        .h(TOOLBAR_H)
        .px(px(8.))
        .bg(theme.panel_edge)
        .border_b_1()
        .border_color(theme.border)
        .flex()
        .items_stretch()
        .child(
            div()
                .flex_none()
                .h_full()
                .flex()
                .items_stretch()
                .child(brand(theme))
                .child(mode_tabs(cx, theme, mode)),
        )
        .child(div().flex_1().min_w_0().h_full())
        .child(
            div()
                .flex_none()
                .flex()
                .h_full()
                .items_stretch()
                .child(right),
        )
}

pub fn mode_tabs(cx: &mut Context<FabricApp>, theme: &Theme, mode: AppMode) -> Div {
    let tabs = [(AppMode::Runs, "RUNS"), (AppMode::Fleets, "FLEETS")];
    let mut row = div().flex().h_full().items_stretch().gap(TAB_GAP);
    for (tab_mode, label) in tabs {
        let active = mode == tab_mode;
        let fill = if active { theme.amber } else { theme.panel_edge };
        let border = if active { theme.amber } else { theme.border };
        let fg = if active { theme.bg } else { theme.text_dim };
        let tab = div()
            .flex_none()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .px(px(12.))
            .bg(fill)
            .border_1()
            .border_color(border)
            .text_size(px(10.))
            .line_height(px(10.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(fg)
            .cursor_pointer()
            .when(!active, |s| {
                s.hover(|s| s.text_color(theme.text).border_color(theme.border_bright))
            })
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.set_mode(tab_mode, cx);
                }),
            );
        row = row.child(tab);
    }
    row
}
