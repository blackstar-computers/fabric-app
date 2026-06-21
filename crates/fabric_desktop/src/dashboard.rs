use crate::format::{fmt_ago, fmt_epoch, fmt_eta, fmt_num, status_label};
use fabric_api::{default_portal_url, load_service_token, spawn_network, Client, ClientError};
use fabric_live::{patch_summary, run_sse_loop, LiveMessage};
use fabric_types::{sort_runs, RunScalars, RunsSummary, SortState};
use gpui::{div, prelude::*, px, rgb, Context, SharedString, Window};
use std::sync::mpsc;
use std::time::Duration;

enum DashboardMsg {
    Summary(Result<RunsSummary, ClientError>),
    Live(LiveMessage),
}

pub struct Dashboard {
    summary: Option<RunsSummary>,
    error: Option<SharedString>,
    live: bool,
    sort: SortState,
}

impl Dashboard {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            summary: None,
            error: None,
            live: false,
            sort: SortState::default(),
        }
    }

    /// Load token, fetch runs, and open the SSE stream.
    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Ok(token) = load_service_token() else {
            self.set_error(
                "No service token — run `fabric auth <token>` or set FABRIC_SERVICE_TOKEN",
                cx,
            );
            return;
        };

        let client = Client::new(default_portal_url(), token);
        let (tx, rx) = mpsc::channel::<DashboardMsg>();

        spawn_network(async move {
            let summary = client.fetch_runs_summary().await;
            let _ = tx.send(DashboardMsg::Summary(summary));
            run_sse_loop(client, |msg| {
                let _ = tx.send(DashboardMsg::Live(msg));
            })
            .await;
        });

        cx.spawn(async move |this, cx| {
            loop {
                while let Ok(msg) = rx.try_recv() {
                    let _ = this.update(cx, |view, cx| {
                        view.handle_msg(msg, cx);
                    });
                }
                cx.background_executor()
                    .timer(Duration::from_millis(32))
                    .await;
            }
        })
        .detach();
    }

    fn handle_msg(&mut self, msg: DashboardMsg, cx: &mut Context<Self>) {
        match msg {
            DashboardMsg::Summary(Ok(summary)) => self.set_summary(summary, cx),
            DashboardMsg::Summary(Err(e)) => self.set_error(format!("{e}"), cx),
            DashboardMsg::Live(live) => self.handle_live(live, cx),
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
                        let _ = patch_summary(summary, &ev);
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
            .bg(rgb(0x000000))
            .text_color(rgb(0xf5f5f7))
            .flex()
            .flex_col()
            .child(header(self.live))
            .child(body(self))
    }
}

fn header(live: bool) -> impl IntoElement {
    let status_color = if live {
        rgb(0x30d158)
    } else {
        rgb(0x98989d)
    };
    let status_text = if live { "Live" } else { "Polling" };

    div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(16.))
        .py(px(10.))
        .bg(rgb(0x1c1c1e))
        .border_b_1()
        .border_color(rgb(0x38383a))
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
                        .text_color(rgb(0x98989d))
                        .child(status_text),
                ),
        )
}

fn body(view: &Dashboard) -> impl IntoElement {
    if let Some(err) = &view.error {
        return div().p_4().text_color(rgb(0xff9f0a)).child(err.clone()).into_any_element();
    }

    let Some(summary) = &view.summary else {
        return div()
            .p_4()
            .text_color(rgb(0x98989d))
            .child("Loading runs…")
            .into_any_element();
    };

    let mut runs = summary.runs.clone();
    sort_runs(&mut runs, view.sort);

    div()
        .id("run-list")
        .flex_1()
        .overflow_y_scroll()
        .child(table_header())
        .children(runs.iter().map(run_row))
        .child(footer(summary, runs.len()))
        .into_any_element()
}

fn table_header() -> impl IntoElement {
    div()
        .flex()
        .px(px(16.))
        .py(px(8.))
        .bg(rgb(0x1c1c1e))
        .border_b_1()
        .border_color(rgb(0x38383a))
        .text_xs()
        .text_color(rgb(0x98989d))
        .child(col("", px(20.)))
        .child(col("Run", px(220.)))
        .child(col("Pod", px(100.)))
        .child(col("Fleet", px(100.)))
        .child(col("Best", px(80.)))
        .child(col("Epoch", px(80.)))
        .child(col("ETA", px(60.)))
        .child(col("Started", px(100.)))
}

fn col(label: &'static str, width: gpui::Pixels) -> impl IntoElement {
    div().w(width).child(label)
}

fn run_row(run: &RunScalars) -> impl IntoElement {
    let status = status_label(run.status.as_deref());
    let dot = match status {
        "running" | "starting" => rgb(0x30d158),
        "stopping" => rgb(0xff9f0a),
        _ => rgb(0x98989d),
    };

    div()
        .flex()
        .px(px(16.))
        .py(px(6.))
        .border_b_1()
        .border_color(rgb(0x38383a))
        .hover(|s| s.bg(rgb(0x1c1c1e)))
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
                .text_color(rgb(0x0a84ff))
                .child(run.name.clone()),
        )
        .child(
            div()
                .w(px(100.))
                .text_sm()
                .text_color(rgb(0x98989d))
                .child(run.pod.clone()),
        )
        .child(
            div()
                .w(px(100.))
                .text_sm()
                .text_color(rgb(0x98989d))
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
                .text_color(rgb(0x98989d))
                .child(fmt_eta(run.eta_sec)),
        )
        .child(
            div()
                .w(px(100.))
                .text_sm()
                .text_color(rgb(0x98989d))
                .child(fmt_ago(run.created)),
        )
}

fn footer(summary: &RunsSummary, n: usize) -> impl IntoElement {
    let active = summary.gpus.active.unwrap_or(0);
    let total = summary.gpus.total.unwrap_or(0);
    div()
        .px(px(16.))
        .py(px(8.))
        .text_xs()
        .text_color(rgb(0x98989d))
        .child(format!("{n} runs · {active}/{total} GPUs active"))
}
