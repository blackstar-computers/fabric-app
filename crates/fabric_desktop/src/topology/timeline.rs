//! Horizontal tick scrubber when decoded frames are available.
//!
//! Each tick is one substrate forward pass returned by `/api/step` with `state_fmt=quant8`.
//! Tick 0 is the state after the first pass; tick N−1 is the converged rollout (where RUN lands).

use crate::theme::Theme;
use crate::topology::TopologyView;
use gpui::{div, prelude::*, px, relative, Context, MouseButton};

const MAX_TICK_PILLS: usize = 24;

pub fn timeline_bar(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let n = view.decoded_frames.len();
    if n == 0 {
        return div()
            .flex_none()
            .h(px(28.))
            .w_full()
            .px(px(10.))
            .flex()
            .items_center()
            .bg(theme.panel_edge)
            .border_t_1()
            .border_color(theme.border)
            .text_size(px(10.))
            .text_color(theme.text_dim)
            .child("Timeline — choose an input and press ▶ RUN to step the substrate")
            .into_any_element();
    }

    let scrub = view.scrub_tick.min(n.saturating_sub(1));
    let playing = view.playing;

    div()
        .id("topology-timeline")
        .flex_none()
        .h(px(36.))
        .w_full()
        .px(px(10.))
        .py(px(4.))
        .flex()
        .items_center()
        .gap_2()
        .bg(theme.panel_edge)
        .border_t_1()
        .border_color(theme.border)
        .child(
            theme
                .title_button(if playing { "⏸ STOP" } else { "▶ PLAY" }, playing)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.toggle_playback(cx)),
                ),
        )
        .child(
            theme
                .title_button("◀", false)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.bump_scrub_tick(-1, cx)),
                ),
        )
        .child(
            div()
                .flex_none()
                .min_w(px(120.))
                .text_size(px(9.))
                .text_color(theme.amber_dim)
                .child(format!(
                    "pass {}/{} · forward steps",
                    scrub + 1,
                    n
                )),
        )
        .child(
            theme
                .title_button("▶", false)
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.bump_scrub_tick(1, cx)),
                ),
        )
        .child(tick_track(n, scrub, theme, cx))
        .into_any_element()
}

fn tick_track(
    n: usize,
    scrub: usize,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> gpui::AnyElement {
    if n <= MAX_TICK_PILLS {
        let ticks: Vec<_> = (0..n)
            .map(|i| {
                let active = i == scrub;
                div()
                    .flex_none()
                    .w(px(18.))
                    .h(px(18.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_1()
                    .border_color(if active {
                        theme.amber
                    } else {
                        theme.border
                    })
                    .bg(if active {
                        theme.panel_edge
                    } else {
                        theme.row_a
                    })
                    .text_size(px(9.))
                    .text_color(if active {
                        theme.amber
                    } else {
                        theme.text_dim
                    })
                    .cursor_pointer()
                    .child(i.to_string())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| this.set_scrub_tick(i, cx)),
                    )
            })
            .collect();

        return div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .gap_1()
            .children(ticks)
            .into_any_element();
    }

    // Many ticks (e.g. 64 for MNIST): show a compact progress bar instead of 64 pills.
    let frac = if n <= 1 {
        0.0
    } else {
        scrub as f32 / (n - 1) as f32
    };

    div()
        .flex_1()
        .min_w_0()
        .h(px(14.))
        .relative()
        .bg(theme.row_a)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left_0()
                .w(relative(frac))
                .bg(theme.amber_dim),
        )
        .child(
            div()
                .absolute()
                .top(px(-2.))
                .left(relative(frac.clamp(0.02, 0.98)))
                .w(px(4.))
                .h(px(18.))
                .bg(theme.amber),
        )
        .into_any_element()
}
