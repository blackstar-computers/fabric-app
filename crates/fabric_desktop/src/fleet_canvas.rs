//! Relay-tree board — positioned DIV node cards over a Metal-painted Bézier
//! edge layer, with native GPUI drag-and-drop for GPU box assignment.
//!
//! The canvas element paints *only* the parent→child links and sits behind the
//! cards (added first, so later siblings stack on top). Every node is a real
//! `div` with text, an `id`, and an `on_click`, so hit-testing and selection
//! are handled by GPUI instead of hand-rolled coordinate math. Box assignment
//! uses `on_drag`/`on_drop`/`drag_over` so the drag is owned by the framework
//! and works across panels.

use crate::fleet_layout::{card_origin, content_size, TreeLayout, CARD_H, CARD_W, OFFX, OFFY};
use crate::fleets::{BoxDrag, FleetsView};
use crate::theme::Theme;
use fabric_types::TreeNode;
use gpui::{
    canvas, div, point, prelude::*, px, Context, PathBuilder, Pixels, Rgba, SharedString, Window,
};

const MIN_SCALE: f32 = 0.6;
const MAX_SCALE: f32 = 1.0;
const TARGET_W: f32 = 760.;

/// Fit-to-width scale (web Fleets.tsx `TreeView`), capped at 1× and floored at 0.6×.
pub fn fit_scale(layout: &TreeLayout) -> f32 {
    let (cw, _) = content_size(layout);
    (TARGET_W / cw.max(1.)).clamp(MIN_SCALE, MAX_SCALE)
}

fn px_f(p: Pixels) -> f32 {
    f32::from(p)
}

fn sx(base: f32, scale: f32) -> Pixels {
    px(base * scale)
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    gpui::Rgba { a: a * c.a, ..c }
}

fn edge_live(node: &TreeNode, fleet_size: i64) -> bool {
    node.relay
        && node.gnodes.is_some()
        && fleet_size > 0
        && node.gnodes.unwrap_or(0) >= fleet_size
}

fn node_fill(theme: &Theme, node: &TreeNode, selected: bool) -> Rgba {
    if selected {
        return theme.panel_edge;
    }
    if !node.up {
        return gpui::rgb(0x140000);
    }
    match node.state.as_deref() {
        Some("running") | Some("ready") => gpui::rgb(0x021200),
        Some("preparing") | Some("starting") => gpui::rgb(0x140d00),
        Some("error") => gpui::rgb(0x140000),
        _ => theme.panel,
    }
}

fn node_border(theme: &Theme, node: &TreeNode, selected: bool) -> Rgba {
    if selected {
        return theme.amber;
    }
    if !node.up {
        return theme.warn;
    }
    match node.state.as_deref() {
        Some("running") | Some("ready") => theme.live,
        Some("preparing") | Some("starting") => theme.amber,
        Some("error") => theme.warn,
        _ => theme.border_bright,
    }
}

fn status_color(theme: &Theme, node: &TreeNode) -> Rgba {
    if !node.up {
        return theme.warn;
    }
    match node.state.as_deref() {
        Some("running") | Some("ready") => theme.live,
        Some("preparing") | Some("starting") => theme.amber,
        Some("error") => theme.warn,
        _ => theme.idle,
    }
}

fn paint_edges(
    window: &mut Window,
    origin: gpui::Point<Pixels>,
    theme: &Theme,
    nodes: &[TreeNode],
    layout: &TreeLayout,
    fleet_size: i64,
    scale: f32,
) {
    let ox = px_f(origin.x);
    let oy = px_f(origin.y);
    for node in nodes {
        let Some(parent_tag) = node.parent.as_deref() else {
            continue;
        };
        let Some(p_pos) = layout.pos.get(parent_tag) else {
            continue;
        };
        let Some(c_pos) = layout.pos.get(&node.tag) else {
            continue;
        };
        // Same anchors as web Fleets.tsx TreeView: parent bottom-center → child top-center.
        let x1 = ox + (p_pos.x + OFFX) * scale;
        let y1 = oy + (p_pos.y + OFFY + CARD_H) * scale;
        let x2 = ox + (c_pos.x + OFFX) * scale;
        let y2 = oy + (c_pos.y + OFFY) * scale;
        let live = edge_live(node, fleet_size);
        let color = if live { theme.live } else { theme.border_bright };
        // Web path: M x1,y1 C x1,mid x2,mid x2,y2
        let mid = (y1 + y2) / 2.;
        let stroke = if live { 2.0 } else { 1.4 };
        let opacity = if node.up { 1.0 } else { 0.45 };
        let mut b = PathBuilder::stroke(px(stroke));
        b.move_to(point(px(x1), px(y1)));
        b.cubic_bezier_to(
            point(px(x1), px(mid)),
            point(px(x2), px(mid)),
            point(px(x2), px(y2)),
        );
        if let Ok(path) = b.build() {
            window.paint_path(path, with_alpha(color, opacity));
        }
    }
}

fn node_card(
    view: &FleetsView,
    theme: &Theme,
    node: &TreeNode,
    scale: f32,
    cx: &mut Context<FleetsView>,
) -> Option<gpui::AnyElement> {
    let pos = view.layout.pos.get(&node.tag)?;
    let (lx, ly) = card_origin(pos);
    let selected = view.selected_node.as_deref() == Some(node.tag.as_str());
    let tag = node.tag.clone();

    let epoch = node
        .epoch
        .map(|e| format!("epoch {e}"))
        .unwrap_or_else(|| "epoch —".into());
    let gpus = match (node.gpus_free, node.gpus_total) {
        (Some(f), Some(t)) => format!("gpu {f}/{t}"),
        (None, Some(t)) => format!("gpu ×{t}"),
        _ => "gpu —".into(),
    };
    let state = node
        .state
        .clone()
        .unwrap_or_else(|| if node.up { "up".into() } else { "down".into() });
    let metric = match (&node.metric_name, node.metric_value) {
        (Some(name), Some(v)) => Some(format!("{name} {v:.3}")),
        _ => None,
    };

    let card = div()
        .id(SharedString::from(format!("node-{tag}")))
        .absolute()
        .left(sx(lx, scale))
        .top(sx(ly, scale))
        .w(sx(CARD_W, scale))
        .h(sx(CARD_H, scale))
        .flex()
        .flex_col()
        .gap(sx(2., scale))
        .px(sx(8., scale))
        .py(sx(6., scale))
        .border_1()
        .border_color(node_border(theme, node, selected))
        .bg(node_fill(theme, node, selected))
        .cursor_pointer()
        .hover(|s| s.border_color(theme.amber))
        .on_click(cx.listener({
            let tag = tag.clone();
            move |this, _, _, cx| this.select_node(tag.clone(), cx)
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(sx(4., scale))
                .child(
                    div()
                        .flex_none()
                        .w(sx(6., scale))
                        .h(sx(6., scale))
                        .bg(status_color(theme, node)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(sx(12., scale))
                        .text_color(if selected { theme.amber } else { theme.data })
                        .child(tag.clone()),
                ),
        )
        .child(
            div()
                .text_size(sx(10., scale))
                .text_color(theme.text_dim)
                .truncate()
                .child(format!("{epoch} · {gpus}")),
        )
        .child(
            div()
                .text_size(sx(10., scale))
                .text_color(status_color(theme, node))
                .truncate()
                .child(state),
        )
        .when_some(metric, |el, m| {
            el.child(
                div()
                    .text_size(sx(9., scale))
                    .text_color(theme.text_dim)
                    .truncate()
                    .child(m),
            )
        });

    Some(card.into_any_element())
}

/// The scrollable graph board: edge canvas + positioned node cards, acting as
/// the drop target for unassigned GPU boxes.
pub fn fleet_board(
    view: &FleetsView,
    theme: &Theme,
    cx: &mut Context<FleetsView>,
) -> impl IntoElement {
    let scale = view.board_scale;
    let (cw, ch) = content_size(&view.layout);
    let content_w = cw * scale;
    let content_h = ch * scale;
    let nodes = view.tree_nodes().to_vec();
    let empty = nodes.is_empty();
    let loading = view.tree_loading;

    let nodes_paint = nodes.clone();
    let layout_paint = view.layout.clone();
    let theme_paint = theme.clone();
    let fleet_size = view.fleet_size;

    let cards: Vec<_> = nodes
        .iter()
        .filter_map(|n| node_card(view, theme, n, scale, cx))
        .collect();

    let drop_hi = theme.amber;
    let board_inner = div()
        .relative()
        .flex_none()
        .w(px(content_w))
        .h(px(content_h))
        .child(
            canvas(
                move |bounds, _, _| bounds,
                move |bounds, _, window, _| {
                    paint_edges(
                        window,
                        bounds.origin,
                        &theme_paint,
                        &nodes_paint,
                        &layout_paint,
                        fleet_size,
                        scale,
                    );
                },
            )
            .absolute()
            .size_full(),
        )
        .children(cards);

    let board_center = div()
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .h_full()
        .min_h(px(content_h))
        .min_w(px(content_w))
        .child(board_inner);

    div()
        .id("fleet-board")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .bg(theme.bg)
        .overflow_scroll()
        .p(px(8.))
        .border_1()
        .border_color(theme.border)
        .drag_over::<BoxDrag>(move |style, _, _, _| {
            style
                .bg(with_alpha(drop_hi, 0.08))
                .border_color(drop_hi)
        })
        .on_drop::<BoxDrag>(cx.listener(|this, drag: &BoxDrag, _, cx| {
            this.assign_box(&drag.contract, cx);
        }))
        .child(board_center)
        .when(empty, |el| {
            el.child(
                div()
                    .absolute()
                    .top(px(16.))
                    .left(px(16.))
                    .text_color(theme.text_dim)
                    .text_size(px(11.))
                    .child(if loading {
                        "PROBING RELAY TREE …"
                    } else {
                        "NO RELAY TREE — drag a GPU box here to seed this fleet"
                    }),
            )
        })
}
