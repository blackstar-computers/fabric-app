//! Left rail — searchable run list merged with viz-eligible checkpoints.
//!
//! Rows are precomputed into [`TopologyView::picker_rows`] and rendered through a
//! virtualized [`uniform_list`] (mirrors `dashboard::run_list`), so scrolling stays
//! smooth even with thousands of checkpoints.

use crate::format::fmt_num;
use crate::theme::Theme;
use crate::topology::{run_matches_checkpoint, TopologyView};
use fabric_health::{group_key, run_is_lm};
use fabric_types::{CheckpointRun, RunScalars};
use fabric_viz::viz_eligible_checkpoint;
use gpui::{div, prelude::*, px, uniform_list, Context, MouseButton};
use std::ops::Range;

const RAIL_W: f32 = 200.;
const ROW_H: f32 = 52.;

#[derive(Clone, Copy)]
pub(crate) struct Badge {
    pub(crate) tone: fabric_health::Tone,
    pub(crate) label: &'static str,
}

/// One filtered, render-ready row in the left rail.
#[derive(Clone)]
pub(crate) struct PickerRowData {
    pub(crate) fleet: String,
    pub(crate) pod: String,
    pub(crate) run: String,
    pub(crate) file: String,
    pub(crate) label: String,
    pub(crate) best: Option<f64>,
    pub(crate) badge: Badge,
}

pub fn picker_rail(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let count = view.picker_rows.len();
    let refreshing = view.refreshing();
    let theme_rows = theme.clone();

    let list_area = if count == 0 {
        div()
            .id("topology-run-list")
            .flex_1()
            .min_h_0()
            .px(px(8.))
            .py(px(8.))
            .text_size(px(10.))
            .text_color(theme.text_dim)
            .child(if refreshing {
                "Loading…"
            } else {
                "No viz-eligible runs"
            })
            .into_any_element()
    } else {
        uniform_list(
            "topology-run-list",
            count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                range
                    .filter_map(|ix| {
                        this.picker_rows
                            .get(ix)
                            .cloned()
                            .map(|row| picker_row(this, cx, &theme_rows, ix, &row))
                    })
                    .collect()
            }),
        )
        .flex_1()
        .min_h_0()
        .w_full()
        .into_any_element()
    };

    div()
        .id("topology-picker")
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
                .px(px(8.))
                .py(px(6.))
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(10.))
                .text_color(theme.amber)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("RUNS"),
        )
        .child(
            div()
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .child(view.search.clone()),
        )
        .child(
            theme
                .table_header_row()
                .child(div().flex_1().min_w_0().truncate().child("RUN"))
                .child(div().flex_none().child("BEST")),
        )
        .child(list_area)
}

fn picker_row(
    view: &TopologyView,
    cx: &mut Context<TopologyView>,
    theme: &Theme,
    ix: usize,
    row: &PickerRowData,
) -> gpui::AnyElement {
    let active = view.selected.as_ref() == Some(&(row.pod.clone(), row.run.clone()))
        && view.selected_file.as_deref() == Some(row.file.as_str());
    let stripe = if active {
        theme.panel_edge
    } else if ix.is_multiple_of(2) {
        theme.row_a
    } else {
        theme.row_b
    };

    let fleet = row.fleet.clone();
    let pod = row.pod.clone();
    let run = row.run.clone();
    let file = row.file.clone();
    let badge = row.badge;

    div()
        .id(ix)
        .w_full()
        .h(px(ROW_H))
        .flex()
        .flex_col()
        .justify_center()
        .gap_1()
        .px(px(8.))
        .py(px(5.))
        .bg(stripe)
        .border_b_1()
        .border_color(if active { theme.amber } else { theme.border })
        .cursor_pointer()
        .hover(|s| s.bg(theme.panel_edge))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.select_run(fleet.clone(), pod.clone(), run.clone(), file.clone(), cx);
            }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if active { theme.amber } else { theme.data })
                        .child(row.label.clone()),
                )
                .child(theme.tone_chip(badge.tone, badge.label)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .text_size(px(9.))
                .text_color(theme.text_dim)
                .child(row.file.clone())
                .child(fmt_num(row.best, 3)),
        )
        .into_any_element()
}

/// Build the search-filtered, sorted rows for the rail from the current deck state.
pub(crate) fn build_picker_rows(view: &TopologyView) -> Vec<PickerRowData> {
    let query = view.picker_query().to_ascii_lowercase();
    build_rows(view)
        .into_iter()
        .filter(|row| {
            query.is_empty()
                || row.label.to_ascii_lowercase().contains(&query)
                || row.pod.to_ascii_lowercase().contains(&query)
                || row.run.to_ascii_lowercase().contains(&query)
                || row.file.to_ascii_lowercase().contains(&query)
                || row.fleet.to_ascii_lowercase().contains(&query)
        })
        .collect()
}

fn build_rows(view: &TopologyView) -> Vec<PickerRowData> {
    let summary_runs: Vec<&RunScalars> = view
        .summary
        .as_ref()
        .map(|s| s.runs.iter().collect())
        .unwrap_or_default();
    let ckpt_runs: Vec<&CheckpointRun> = view
        .checkpoints
        .as_ref()
        .map(|c| c.runs.iter().collect())
        .unwrap_or_default();

    let mut rows = Vec::new();
    for ck in ckpt_runs {
        let best = summary_runs
            .iter()
            .find(|r| run_matches_checkpoint(r, &ck.pod, &ck.run))
            .and_then(|r| r.best);
        let scalar = summary_runs
            .iter()
            .find(|r| run_matches_checkpoint(r, &ck.pod, &ck.run))
            .copied();
        for file in ck
            .files
            .iter()
            .filter(|f| viz_eligible_checkpoint(ck, f, scalar))
        {
            rows.push(PickerRowData {
                fleet: ck.fleet.clone(),
                pod: ck.pod.clone(),
                run: ck.run.clone(),
                file: file.filename.clone(),
                label: format!("{}:{}", ck.pod, ck.run),
                best: file.psnr.or(best),
                badge: substrate_badge(scalar),
            });
        }
    }

    if rows.is_empty() {
        for run in summary_runs {
            rows.push(PickerRowData {
                fleet: run.fleet.clone(),
                pod: run.pod.clone(),
                run: run.name.clone(),
                file: String::new(),
                label: format!("{}:{}", run.pod, run.name),
                best: run.best,
                badge: substrate_badge(Some(run)),
            });
        }
    }

    rows.sort_by(|a, b| a.label.cmp(&b.label));
    rows
}

fn substrate_badge(run: Option<&RunScalars>) -> Badge {
    let Some(run) = run else {
        return Badge {
            tone: fabric_health::Tone::Neutral,
            label: "—",
        };
    };
    if run_is_lm(run) {
        Badge {
            tone: fabric_health::Tone::Warn,
            label: "LM",
        }
    } else if run
        .runspec
        .as_ref()
        .and_then(|rs| rs.substrate_kind.as_deref())
        == Some("canvas")
    {
        Badge {
            tone: fabric_health::Tone::Good,
            label: "CVS",
        }
    } else {
        let _ = group_key(run);
        Badge {
            tone: fabric_health::Tone::Neutral,
            label: "REC",
        }
    }
}
