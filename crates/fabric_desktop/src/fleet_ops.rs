//! Right-hand ops rail — jobs ledger, unassigned GPU boxes (select + assign +
//! drag), fleet actions, and details for the selected relay node.
//!
//! Box chips are draggable via the native GPUI `on_drag` API (the board owns
//! the matching `on_drop`), and also support a click-to-select + ASSIGN button
//! flow for users who prefer not to drag.

use crate::fleets::{status_dot_color, BoxDrag, FleetsView};
use crate::theme::Theme;
use fabric_types::{Instance, Job, TreeNode};
use gpui::{div, prelude::*, px, Context, MouseButton, Render, SharedString, Window};

const RAIL_W: f32 = 224.;

/// Lightweight floating chip rendered under the cursor while a box is dragged.
pub struct BoxDragPreview {
    pub label: String,
    pub theme: Theme,
}

impl Render for BoxDragPreview {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = &self.theme;
        div().child(
            div()
                .px(px(8.))
                .py(px(4.))
                .bg(t.panel_edge)
                .border_1()
                .border_color(t.amber)
                .text_size(px(10.))
                .text_color(t.amber)
                .child(format!("▦ {}", self.label)),
        )
    }
}

fn section_header(theme: &Theme, label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(4.))
        .bg(theme.panel_edge)
        .border_b_1()
        .border_color(theme.border)
        .text_size(px(10.))
        .text_color(theme.amber)
        .child(label.into())
}

fn job_row(theme: &Theme, job: &Job, selected: bool, cx: &mut Context<FleetsView>) -> impl IntoElement {
    let id = job.job_id.clone();
    div()
        .id(SharedString::from(format!("job-{id}")))
        .px(px(8.))
        .py(px(3.))
        .flex()
        .items_center()
        .gap_2()
        .border_b_1()
        .border_color(theme.border)
        .bg(if selected { theme.panel_edge } else { theme.row_a })
        .cursor_pointer()
        .hover(|s| s.bg(theme.panel_edge))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| this.select_job(id.clone(), cx)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(10.))
                .text_color(if selected { theme.amber } else { theme.text })
                .child(job.run_name.clone()),
        )
        .child(
            div()
                .flex_none()
                .text_size(px(9.))
                .text_color(status_dot_color(theme, &job.state))
                .child(job.state.clone()),
        )
}

fn box_chip(
    theme: &Theme,
    inst: &Instance,
    selected: bool,
    cx: &mut Context<FleetsView>,
) -> impl IntoElement {
    let label = inst
        .pod_name
        .clone()
        .or_else(|| inst.label.clone())
        .unwrap_or_else(|| inst.id_str());
    let gpu = inst.gpu_name.clone().unwrap_or_else(|| "GPU".into());
    let ng = inst.num_gpus.unwrap_or(0);
    let provision = inst
        .provision_state
        .as_deref()
        .or(inst.status.as_deref())
        .filter(|s| !s.is_empty() && *s != "ready");
    let id = inst.id_str();
    let drag = BoxDrag {
        contract: id.clone(),
        label: label.clone(),
    };
    let drag_theme = theme.clone();

    div()
        .id(SharedString::from(format!("box-{id}")))
        .flex_none()
        .mx(px(6.))
        .mb(px(4.))
        .flex()
        .flex_col()
        .gap(px(1.))
        .px(px(6.))
        .py(px(4.))
        .border_1()
        .border_color(if selected { theme.amber } else { theme.border_bright })
        .bg(if selected { theme.panel_edge } else { theme.panel })
        .cursor_pointer()
        .hover(|s| s.border_color(theme.amber))
        .on_click(cx.listener({
            let id = id.clone();
            move |this, _, _, cx| this.select_box(id.clone(), cx)
        }))
        .on_drag(drag, move |d: &BoxDrag, _pos, _window, cx| {
            let label = d.label.clone();
            let theme = drag_theme.clone();
            cx.new(|_| BoxDragPreview { label, theme })
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(div().flex_none().text_size(px(9.)).text_color(theme.text_dim).child("▦"))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(10.))
                        .text_color(theme.data)
                        .child(label),
                ),
        )
        .child(
            div()
                .text_size(px(9.))
                .text_color(theme.text_dim)
                .truncate()
                .child(format!("{gpu} ×{ng}")),
        )
        .when_some(provision.map(|s| s.to_string()), |el, state| {
            el.child(
                div()
                    .text_size(px(9.))
                    .text_color(theme.amber_dim)
                    .truncate()
                    .child(state),
            )
        })
}

fn node_details(theme: &Theme, node: &TreeNode) -> impl IntoElement {
    let mut rows: Vec<(SharedString, SharedString)> = Vec::new();
    rows.push(("tag".into(), node.tag.clone().into()));
    if let Some(p) = &node.parent {
        rows.push(("parent".into(), p.clone().into()));
    }
    rows.push((
        "state".into(),
        node.state.clone().unwrap_or_else(|| "—".into()).into(),
    ));
    rows.push((
        "link".into(),
        if node.up { "up" } else { "down" }.into(),
    ));
    if let Some(e) = node.epoch {
        rows.push(("epoch".into(), e.to_string().into()));
    }
    match (node.gpus_free, node.gpus_total) {
        (Some(f), Some(t)) => rows.push(("gpus".into(), format!("{f} free / {t}").into())),
        (None, Some(t)) => rows.push(("gpus".into(), format!("×{t}").into())),
        _ => {}
    }
    if let Some(h) = &node.host {
        rows.push(("host".into(), h.clone().into()));
    }
    if let (Some(name), Some(v)) = (&node.metric_name, node.metric_value) {
        rows.push((name.clone().into(), format!("{v:.3}").into()));
    }

    let body: Vec<_> = rows
        .into_iter()
        .map(|(k, v)| {
            div()
                .px(px(8.))
                .py(px(1.))
                .flex()
                .gap_2()
                .text_size(px(10.))
                .child(div().flex_none().w(px(54.)).text_color(theme.text_dim).child(k))
                .child(div().flex_1().min_w_0().truncate().text_color(theme.data).child(v))
        })
        .collect();

    div().flex_none().flex().flex_col().pb(px(4.)).children(body)
}

/// Build the full ops rail for the current [`FleetsView`] state.
pub fn ops_rail(
    view: &FleetsView,
    theme: &Theme,
    cx: &mut Context<FleetsView>,
) -> impl IntoElement {
    let jobs = view.jobs.clone();
    let selected_job = view.selected_job.clone();
    let boxes = view.unassigned_boxes();
    let selected_box = view.selected_box.clone();
    let has_fleet = !view.selected_fleet.is_empty();
    let can_assign = selected_box.is_some() && has_fleet;
    let can_stop = has_fleet;

    let job_rows: Vec<_> = jobs
        .iter()
        .map(|j| {
            let sel = selected_job.as_deref() == Some(j.job_id.as_str());
            job_row(theme, j, sel, cx).into_any_element()
        })
        .collect();

    let chips: Vec<_> = boxes
        .iter()
        .map(|b| {
            let sel = selected_box.as_deref() == Some(b.id_str().as_str());
            box_chip(theme, b, sel, cx).into_any_element()
        })
        .collect();

    let selected_node = view
        .selected_node
        .as_ref()
        .and_then(|tag| view.tree_nodes().iter().find(|n| &n.tag == tag).cloned());

    div()
        .flex_none()
        .w(px(RAIL_W))
        .min_h_0()
        .flex()
        .flex_col()
        .border_l_1()
        .border_color(theme.border)
        .bg(theme.bg)
        // Jobs ledger
        .child(section_header(theme, format!("JOBS ({})", jobs.len())))
        .child(
            div()
                .id("ops-jobs")
                .flex_none()
                .max_h(px(150.))
                .overflow_y_scroll()
                .children(job_rows),
        )
        // Unassigned GPU boxes
        .child(section_header(theme, format!("UNASSIGNED GPU ({})", boxes.len())))
        .child(
            div()
                .id("ops-boxes")
                .flex_1()
                .min_h_0()
                .pt(px(4.))
                .overflow_y_scroll()
                .when(chips.is_empty(), |el| {
                    el.child(
                        div()
                            .px(px(8.))
                            .py(px(6.))
                            .text_size(px(10.))
                            .text_color(theme.text_dim)
                            .child("no free boxes"),
                    )
                })
                .children(chips),
        )
        .child(
            div().flex_none().p(px(6.)).child(
                theme
                    .title_button(" ASSIGN ▸ ", can_assign)
                    .id("assign-box")
                    .on_click(cx.listener(|this, _, _, cx| this.assign_selected_box(cx))),
            ),
        )
        // Fleet actions
        .child(section_header(theme, "ACTIONS"))
        .child(
            div()
                .flex_none()
                .p(px(6.))
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    theme
                        .title_button(" + NEW FLEET ", false)
                        .id("new-fleet")
                        .on_click(cx.listener(|this, _, _, cx| this.new_fleet(cx))),
                )
                .child(
                    theme
                        .title_button(" ◼ STOP FLEET ", can_stop)
                        .id("stop-job")
                        .on_click(cx.listener(|this, _, _, cx| this.stop_selected_job(cx))),
                )
                .child(
                    theme
                        .title_button(" ↻ REFRESH TREE ", false)
                        .id("refresh-tree")
                        .on_click(cx.listener(|this, _, _, cx| this.refresh_tree(cx))),
                ),
        )
        // Selected node details
        .when_some(selected_node, |el, node| {
            el.child(section_header(theme, "NODE"))
                .child(node_details(theme, &node))
        })
}
