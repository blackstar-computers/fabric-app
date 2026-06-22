//! Dataset gallery side pane — thumbnail grid for picking the input image fed to STEP.
//!
//! Mirrors `InputPicker.tsx`: a source picker plus a virtualized grid of 48×48 dataset
//! thumbnails. Clicking a thumbnail sets the active input index; the selected cell is framed
//! in amber. Sits between the run picker and the substrate explorer.

use crate::theme::Theme;
use crate::topology::TopologyView;
use fabric_viz::viz_sources;
use gpui::{
    div, img, prelude::*, px, uniform_list, App, Context, ListSizingBehavior, MouseButton,
    RenderImage, SharedString,
};
use std::collections::HashMap;
use std::sync::Arc;

const PANE_W: f32 = 220.;
const THUMB: f32 = 48.;
const COLS: usize = 3;

pub fn gallery_pane(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    div()
        .id("topology-gallery")
        .flex_none()
        .w(px(PANE_W))
        .h_full()
        .flex()
        .flex_col()
        .bg(theme.panel)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_none()
                .px(px(8.))
                .py(px(6.))
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(10.))
                .text_color(theme.amber)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("DATASET"),
        )
        .child(source_pills(view, theme, cx))
        .child(grid(view, theme, cx))
        .child(footer(view, theme, cx))
}

/// Source pills (run dataset + built-ins) reusing the loaded run's `viz_sources`. Clicking a pill
/// switches the active source and refetches the gallery for it.
fn source_pills(
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
        .flex_none()
        .px(px(8.))
        .py(px(6.))
        .flex()
        .flex_wrap()
        .items_center()
        .gap_1()
        .children(pills)
}

fn grid(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let items: Vec<i64> = view
        .gallery
        .as_ref()
        .map(|g| g.items.iter().map(|it| it.idx).collect())
        .unwrap_or_default();

    if items.is_empty() {
        let label = if view.gallery_loading {
            "Loading thumbnails…"
        } else if view.viz_meta.is_none() {
            "Load a run to browse inputs"
        } else {
            "No thumbnails for this source"
        };
        return div()
            .flex_1()
            .min_h_0()
            .px(px(8.))
            .py(px(8.))
            .text_size(px(10.))
            .text_color(theme.text_dim)
            .child(label)
            .into_any_element();
    }

    let entity = cx.entity();
    let selected_idx = view.input_idx as i64;
    let theme = theme.clone();
    let row_count = items.len().div_ceil(COLS);
    let thumb_images: HashMap<i64, Arc<RenderImage>> = items
        .iter()
        .filter_map(|&idx| view.gallery_image(idx).map(|img| (idx, img)))
        .collect();

    let list = uniform_list(
        "topology-gallery-grid",
        row_count,
        move |range, _window, _app| {
            range
                .map(|row| {
                    let start = row * COLS;
                    let mut cells = Vec::new();
                    for col in 0..COLS {
                        let i = start + col;
                        if i >= items.len() {
                            break;
                        }
                        let idx = items[i];
                        let render_img = thumb_images.get(&idx).cloned();
                        cells.push(thumb_cell(idx, render_img, selected_idx, &theme, &entity));
                    }
                    div().flex().gap_1().pb_1().children(cells)
                })
                .collect()
        },
    )
    .with_sizing_behavior(ListSizingBehavior::Auto);

    div()
        .flex_1()
        .min_h_0()
        .px(px(8.))
        .py(px(4.))
        .child(list.flex_1().min_h_0().size_full())
        .into_any_element()
}

fn thumb_cell(
    idx: i64,
    render_img: Option<Arc<RenderImage>>,
    selected_idx: i64,
    theme: &Theme,
    entity: &gpui::Entity<TopologyView>,
) -> gpui::AnyElement {
    let active = idx == selected_idx;
    let entity = entity.clone();

    let cell = div()
        .id(SharedString::from(format!("gal-{idx}")))
        .flex_none()
        .w(px(THUMB))
        .h(px(THUMB))
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.panel_edge)
        .border_1()
        .border_color(if active { theme.amber } else { theme.border })
        .cursor_pointer()
        .hover(|s| s.border_color(theme.amber_dim))
        .on_mouse_down(MouseButton::Left, move |_, _, app: &mut App| {
            entity.update(app, |this, cx| this.set_input_idx(idx as u32, cx));
        });

    let cell = match render_img {
        Some(image) => cell.child(img(image).w(px(THUMB - 4.)).h(px(THUMB - 4.))),
        None => cell.child(
            div()
                .text_size(px(8.))
                .text_color(theme.text_dim)
                .child(idx.to_string()),
        ),
    };

    cell.into_any_element()
}

/// Prev / next index steppers plus the dataset's validation-split offset, when known.
fn footer(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let val_start = view
        .gallery
        .as_ref()
        .and_then(|g| g.val_start)
        .or_else(|| view.viz_meta.as_ref().and_then(|m| m.val_start));

    div()
        .flex_none()
        .px(px(8.))
        .py(px(6.))
        .border_t_1()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap_1()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    theme
                        .title_button("◀ PREV", false)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.bump_input_idx(-1, cx)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme.data)
                        .child(format!("IDX {}", view.input_idx)),
                )
                .child(
                    theme
                        .title_button("NEXT ▶", false)
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| this.bump_input_idx(1, cx)),
                        ),
                ),
        )
        .when_some(val_start, |el, vs| {
            el.child(
                div()
                    .text_size(px(9.))
                    .text_color(theme.text_dim)
                    .child(format!("val_start {vs}")),
            )
        })
}
