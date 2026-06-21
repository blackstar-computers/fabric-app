use crate::format::{fmt_ago, fmt_epoch, fmt_eta, fmt_num, status_label};
use fabric_live::{patch_summary, LiveMessage};
use fabric_types::{sort_runs, RunScalars, RunsSummary, SortState};
use gpui::{
    div, px, rgb, Context, Div, IntoElement, ParentElement, Render, SharedString, Styled, Window,
};

const BG: gpui::Rgba = rgb(0x000000);
const SURFACE: gpui::Rgba = rgb(0x1c1c1e);
const BORDER: gpui::Rgba = rgb(0x38383a);
const TEXT: gpui::Rgba = rgb(0xf5f5f7);
const TEXT_DIM: gpui::Rgba = rgb(0x98989d);
const GOOD: gpui::Rgba = rgb(0x30d158);
const WARN: gpui::Rgba = rgb(0xff9f0a);
const ACCENT: gpui::Rgba = rgb(0x0a84ff);

pub struct Dashboard {
    summary: Option<RunsSummary>,
    error: Option<SharedString>,
    live: bool,
    sort: SortState,
}

impl Dashboard {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            summary: None,
            error: None,
            live: false,
            sort: SortState::default(),
        }
    }

    pub fn set_summary(&mut self, summary: RunsSummary, cx: &mut Context<Self>) {
        self.error = None;
        self.summary = Some(summary);
        cx.notify();
    }

    pub fn set_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    pub fn handle_live(&mut self, msg: LiveMessage, cx: &mut Context<Self>) {
        match msg {
            LiveMessage::Connected => self.live = true,
            LiveMessage::Disconnected => self.live = false,
            LiveMessage::RunEvent(ev) => {
                if let Some(summary) = self.summary.as_mut() {
                    if ev.is_run_v2() {
                        if !patch_summary(summary, &ev) {
                            // New run — full refetch handled by caller in a later iteration.
                        }
                    }
                }
            }
        }
        cx.notify();
    }
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(BG)
            .text_color(TEXT)
            .flex()
            .flex_col()
            .child(header(self.live))
            .child(body(self))
    }
}

fn header(live: bool) -> Div {
    let status_color = if live { GOOD } else { TEXT_DIM };
    let status_text = if live { "Live" } else { "Polling" };

    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.))
        .py(px(10.))
        .bg(SURFACE)
        .border_b_1()
        .border_color(BORDER)
        .child(
            div()
                .text_lg()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child("Runs"),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .size(px(8.))
                        .rounded_full()
                        .bg(status_color),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(TEXT_DIM)
                        .child(status_text),
                ),
        )
}

fn body(view: &Dashboard) -> Div {
    if let Some(err) = &view.error {
        return div().p_4().text_color(WARN).child(err.clone());
    }

    let Some(summary) = &view.summary else {
        return div().p_4().text_color(TEXT_DIM).child("Loading runs…");
    };

    let mut runs = summary.runs.clone();
    sort_runs(&mut runs, view.sort);

    div()
        .flex_1()
        .overflow_y_scroll()
        .child(table_header())
        .children(runs.iter().map(run_row))
        .child(footer(summary, runs.len()))
}

fn table_header() -> Div {
    div()
        .flex()
        .px(px(16.))
        .py(px(8.))
        .bg(SURFACE)
        .border_b_1()
        .border_color(BORDER)
        .text_xs()
        .text_color(TEXT_DIM)
        .child(col("", px(20.)))
        .child(col("Run", px(220.)))
        .child(col("Pod", px(100.)))
        .child(col("Fleet", px(100.)))
        .child(col("Best", px(80.)))
        .child(col("Epoch", px(80.)))
        .child(col("ETA", px(60.)))
        .child(col("Started", px(100.)))
}

fn col(label: &str, width: gpui::Pixels) -> Div {
    div().w(width).child(label)
}

fn run_row(run: &RunScalars) -> Div {
    let status = status_label(run.status.as_deref());
    let dot = match status {
        "running" | "starting" => GOOD,
        "stopping" => WARN,
        _ => TEXT_DIM,
    };

    div()
        .flex()
        .px(px(16.))
        .py(px(6.))
        .border_b_1()
        .border_color(BORDER)
        .hover(|s| s.bg(SURFACE))
        .child(
            div()
                .w(px(20.))
                .flex()
                .items_center()
                .child(
                    div()
                        .size(px(8.))
                        .rounded_full()
                        .bg(dot),
                ),
        )
        .child(
            div()
                .w(px(220.))
                .text_sm()
                .text_color(ACCENT)
                .child(run.name.clone()),
        )
        .child(
            div()
                .w(px(100.))
                .text_sm()
                .text_color(TEXT_DIM)
                .child(run.pod.clone()),
        )
        .child(
            div()
                .w(px(100.))
                .text_sm()
                .text_color(TEXT_DIM)
                .child(run.fleet.clone()),
        )
        .child(
            div()
                .w(px(80.))
                .text_sm()
                .child(fmt_num(run.best, 3)),
        )
        .child(
            div()
                .w(px(80.))
                .text_sm()
                .child(fmt_epoch(run.last_epoch, run.total_epochs)),
        )
        .child(
            div()
                .w(px(60.))
                .text_sm()
                .text_color(TEXT_DIM)
                .child(fmt_eta(run.eta_sec)),
        )
        .child(
            div()
                .w(px(100.))
                .text_sm()
                .text_color(TEXT_DIM)
                .child(fmt_ago(run.created)),
        )
}

fn footer(summary: &RunsSummary, n: usize) -> Div {
    let active = summary.gpus.active.unwrap_or(0);
    let total = summary.gpus.total.unwrap_or(0);
    div()
        .px(px(16.))
        .py(px(8.))
        .text_xs()
        .text_color(TEXT_DIM)
        .child(format!("{n} runs · {active}/{total} GPUs active"))
}
