//! Right rail — load, input controls, step, display mode, cell info.

use crate::format::fmt_num;
use crate::theme::Theme;
use crate::topology::previews::previews_column;
use crate::topology::{DisplayMode, TopologyView};
use fabric_viz::{display_regions, region_at, substrate_cols};
use fabric_viz::viz_sources;
use gpui::{div, prelude::*, px, Context, MouseButton};

const RAIL_W: f32 = 224.;

pub fn controls_rail(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let can_load = view.selected.is_some() && view.selected_file.is_some();
    let stepping = view.stepping;
    let loading = view.loading;

    div()
        .id("topology-controls")
        .flex_none()
        .w(px(RAIL_W))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.panel)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_none()
                .px(px(10.))
                .py(px(8.))
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(10.))
                .text_color(theme.amber)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("CONTROLS"),
        )
        .child(
            div()
                .flex_none()
                .px(px(10.))
                .py(px(8.))
                .flex()
                .flex_col()
                .gap_3()
                .child(
                    theme
                        .title_button(
                            if loading { "LOADING…" } else { "LOAD" },
                            loading,
                        )
                        .when(!can_load || loading, |b| b.opacity(0.5))
                        .when(can_load && !loading, |b| {
                            b.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.load_checkpoint(cx)),
                            )
                        }),
                )
                .child(section_label(theme, "INPUT"))
                .child(input_sources_row(view, theme, cx))
                .child(spinner_row(
                    theme,
                    "IDX",
                    view.input_idx,
                    cx,
                    -1,
                    1,
                ))
                .child(spinner_row_ticks(theme, view.ticks, cx))
                .child(
                    theme
                        .title_button(
                            if stepping {
                                "RUNNING…"
                            } else {
                                "▶ RUN"
                            },
                            stepping,
                        )
                        .when(view.viz_meta.is_none() || stepping, |b| b.opacity(0.5))
                        .when(view.viz_meta.is_some() && !stepping, |b| {
                            b.cursor_pointer().on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| this.step(cx)),
                            )
                        }),
                )
                .child(section_label(theme, "DISPLAY"))
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .child(mode_toggle(
                            theme,
                            cx,
                            DisplayMode::Structure,
                            view.display_mode,
                            "STRUCT",
                        ))
                        .child(mode_toggle(
                            theme,
                            cx,
                            DisplayMode::LiveFlow,
                            view.display_mode,
                            "LIVE",
                        )),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_h_0()
                .px(px(10.))
                .py(px(8.))
                .border_t_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap_2()
                .child(section_label(theme, "CELL"))
                .child(cell_info(view, theme))
                .child(previews_column(view, theme)),
        )
}

/// Input-source pills built from the loaded run's selectable sources (run dataset
/// + built-ins, via `viz_sources`). Falls back to a single MNIST pill before a run
///   is loaded so the rail is never empty.
fn input_sources_row(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let sources = view
        .viz_meta
        .as_ref()
        .map(viz_sources)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| vec!["mnist".to_string()]);

    let pills: Vec<_> = sources
        .into_iter()
        .map(|src| {
            let active = view.input_source == src;
            let target = src.clone();
            theme
                .filter_pill(active, src.to_ascii_uppercase())
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.set_input_source(target.clone(), cx);
                    }),
                )
                .into_any_element()
        })
        .collect();

    div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .children(pills)
}

fn section_label(theme: &Theme, text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(9.))
        .text_color(theme.text_dim)
        .child(text)
}

fn spinner_row(
    theme: &Theme,
    label: &'static str,
    value: u32,
    cx: &mut Context<TopologyView>,
    dec: i32,
    inc: i32,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(10.))
                .text_color(theme.text_dim)
                .child(label),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    theme
                        .title_button("−", false)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.bump_input_idx(dec, cx)),
                        ),
                )
                .child(
                    div()
                        .w(px(36.))
                        .text_center()
                        .text_color(theme.data)
                        .child(value.to_string()),
                )
                .child(
                    theme
                        .title_button("+", false)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| this.bump_input_idx(inc, cx)),
                        ),
                ),
        )
}

fn spinner_row_ticks(
    theme: &Theme,
    value: u32,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_between()
        .child(
            div()
                .text_size(px(10.))
                .text_color(theme.text_dim)
                .child("TICKS"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(
                    theme
                        .title_button("−", false)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.bump_ticks(-1, cx)),
                        ),
                )
                .child(
                    div()
                        .w(px(36.))
                        .text_center()
                        .text_color(theme.data)
                        .child(value.to_string()),
                )
                .child(
                    theme
                        .title_button("+", false)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.bump_ticks(1, cx)),
                        ),
                ),
        )
}

fn mode_toggle(
    theme: &Theme,
    cx: &mut Context<TopologyView>,
    mode: DisplayMode,
    active_mode: DisplayMode,
    label: &'static str,
) -> impl IntoElement {
    let active = mode == active_mode;
    theme
        .filter_pill(active, label)
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.set_display_mode(mode, cx)),
        )
}

fn cell_info(view: &TopologyView, theme: &Theme) -> impl IntoElement {
    let Some(cell) = view.selected_cell else {
        return div()
            .text_size(px(10.))
            .text_color(theme.text_dim)
            .child("Click a cell in the canvas")
            .into_any_element();
    };

    let activation = view
        .decoded_frames
        .get(view.scrub_tick)
        .and_then(|f| f.get(cell))
        .copied();
    let region = cell_region_label(view, cell);

    div()
        .flex()
        .flex_col()
        .gap_1()
        .text_size(px(10.))
        .text_color(theme.text)
        .child(format!("index {cell}"))
        .child(format!("region {region}"))
        .child(format!("act {}", fmt_num(activation.map(f64::from), 4)))
        .into_any_element()
}

fn cell_region_label(view: &TopologyView, cell: usize) -> String {
    if let Some(topo) = &view.topo_data {
        return format!("{:?}", topo.region_of(cell));
    }
    let Some(meta) = &view.viz_meta else {
        return "—".into();
    };
    let Some(geo) = &meta.geometry else {
        return "—".into();
    };
    let cols = substrate_cols(geo).max(1);
    let row = cell as u32 / cols;
    let col = cell as u32 % cols;
    let regions = display_regions(geo, meta.viz_kind.as_ref());
    format!("{:?}", region_at(row, col, &regions))
}
