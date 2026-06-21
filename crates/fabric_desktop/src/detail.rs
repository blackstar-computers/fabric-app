//! War Room detail pane — port of `web_app/src/routes/WarRoom.tsx`
//! (KPIs + health + synchronized line charts driven by hover crosshair).

use crate::charts::{self, metric_chart};
use crate::dashboard::Dashboard;
use crate::format::{fmt_ago, fmt_epoch, fmt_eta, fmt_num};
use crate::theme::Theme;
use fabric_health::{
    derive_signals, is_lm_run, metric_panels_for_run, tone_label, worst_tone, HealthSignal, Tone,
};
use fabric_types::{RunScalars, RunSeries};
use gpui::{div, prelude::*, px, rgb, uniform_list, Context, Div, FontWeight, SharedString};
use std::ops::Range;

pub const SERIES_MAX_POINTS: u32 = 2000;

#[allow(clippy::too_many_arguments)]
pub fn render_detail(
    theme: &Theme,
    run: &RunScalars,
    series: Option<&RunSeries>,
    series_loading: bool,
    series_error: Option<&SharedString>,
    back: impl IntoElement,
    cx: &mut Context<Dashboard>,
) -> Div {
    let signals = derive_signals(run, series);
    let tone = worst_tone(&signals);
    let _lm = is_lm_run(run);

    div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .border_l_1()
        .border_color(theme.border)
        .child(command_bar(theme, run, series, tone, back))
        .child(health_strip(theme, &signals))
        .child(metric_wall(
            theme,
            run,
            series,
            series_loading,
            series_error,
            cx,
        ))
        .child(config_card(theme, run))
}

fn command_bar(
    theme: &Theme,
    run: &RunScalars,
    series: Option<&RunSeries>,
    tone: Tone,
    back: impl IntoElement,
) -> Div {
    let lm = is_lm_run(run);
    let loss = series
        .and_then(|s| {
            if lm {
                s.latest("ce_val").or_else(|| s.latest("ppl"))
            } else {
                s.latest("loss")
            }
        })
        .or(run.last_top1);
    let lr = series.and_then(|s| s.latest("lr")).or(run.last_lr);
    let gnorm = series.and_then(|s| s.latest("gnorm"));
    let util = series.and_then(|s| s.latest("gpu_util"));

    div()
        .flex_none()
        .flex()
        .flex_col()
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.panel_edge)
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .px(px(8.))
                .py(px(6.))
                .child(back)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.link)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(run.name.clone()),
                )
                .child(theme.chip(run.pod.clone()))
                .child(theme.chip(run.fleet.clone()))
                .child(theme.tone_chip(tone, tone_label(tone)))
                .child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme.text_dim)
                        .child(run.status.clone().unwrap_or_else(|| "unknown".into())),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_3()
                .px(px(8.))
                .pb(px(6.))
                .children(kpi_row(
                    theme,
                    &[
                        ("EPOCH", fmt_epoch(run.last_epoch, run.total_epochs)),
                        ("ETA", fmt_eta(run.eta_sec)),
                        (
                            if lm { "BEST PPL" } else { "BEST" },
                            fmt_num(run.best, 3),
                        ),
                        ("LOSS", fmt_num(loss, 3)),
                        ("LR", fmt_num(lr, 4)),
                        ("GNORM", fmt_num(gnorm, 3)),
                        ("GPU", fmt_gpu(util)),
                    ],
                )),
        )
}

fn kpi_row(theme: &Theme, items: &[(&str, String)]) -> Vec<Div> {
    items
        .iter()
        .map(|(label, value)| {
            div().child(
                div()
                    .child(
                        div()
                            .text_color(theme.data)
                            .text_size(px(11.))
                            .child(value.clone()),
                    )
                        .child(
                            div()
                                .text_color(theme.text_dim)
                                .text_size(px(9.))
                                .child(label.to_string()),
                        ),
            )
        })
        .collect()
}

fn fmt_gpu(util: Option<f64>) -> String {
    util.map(|u| format!("{u:.0}%")).unwrap_or_else(|| "—".into())
}

fn health_strip(theme: &Theme, signals: &[HealthSignal]) -> Div {
    if signals.is_empty() {
        return div()
            .flex_none()
            .px(px(8.))
            .py(px(6.))
            .border_b_1()
            .border_color(theme.border)
            .text_size(px(10.))
            .text_color(theme.text_dim)
            .child("NO HEALTH SIGNALS — awaiting gnorm / GPU / objective points");
    }

    div()
        .flex_none()
        .flex()
        .flex_wrap()
        .gap_2()
        .px(px(8.))
        .py(px(6.))
        .border_b_1()
        .border_color(theme.border)
        .children(
            signals
                .iter()
                .map(|sig| health_card(theme, sig))
                .collect::<Vec<_>>(),
        )
}

fn health_card(theme: &Theme, sig: &HealthSignal) -> Div {
    let color = theme.tone_color(sig.tone);
    div()
        .px(px(6.))
        .py(px(4.))
        .border_1()
        .border_color(color)
        .bg(rgb(0x0a0a0a))
        .child(
            div()
                .flex()
                .items_start()
                .gap_2()
                .child(div().text_color(color).child("■"))
                .child(
                    div()
                        .child(
                            div()
                                .text_color(theme.data)
                                .text_size(px(11.))
                                .child(sig.value.clone()),
                        )
                        .child(
                            div()
                                .text_color(theme.text_dim)
                                .text_size(px(9.))
                                .child(sig.label),
                        ),
                ),
        )
}

#[allow(clippy::too_many_arguments)]
fn metric_wall(
    theme: &Theme,
    run: &RunScalars,
    series: Option<&RunSeries>,
    loading: bool,
    error: Option<&SharedString>,
    cx: &mut Context<Dashboard>,
) -> Div {
    let header = div()
        .flex_none()
        .px(px(8.))
        .py(px(4.))
        .bg(theme.panel_edge)
        .border_b_1()
        .border_color(theme.border)
        .text_color(theme.amber)
        .text_size(px(10.))
        .child("METRICS");

    let shell = div().flex_1().min_h_0().flex().flex_col().child(header);

    if let Some(err) = error {
        return shell.child(
            div()
                .flex_1()
                .p(px(8.))
                .text_color(theme.warn)
                .child(format!("■ SERIES ERR — {err}")),
        );
    }
    let (Some(s), false) = (series, loading) else {
        return shell.child(
            div()
                .flex_1()
                .p(px(8.))
                .text_color(theme.amber)
                .child("▸ LOADING SERIES…"),
        );
    };

    let panels = metric_panels_for_run(run, Some(s));
    let x_dom = charts::x_domain(s);
    let (false, Some(_x_dom)) = (panels.is_empty(), x_dom) else {
        return shell.child(
            div()
                .flex_1()
                .p(px(8.))
                .text_color(theme.text_dim)
                .child("NO METRIC SERIES YET"),
        );
    };

    let panel_count = panels.len();

    shell.child(
        uniform_list(
            "metric-wall",
            panel_count,
            cx.processor(move |this, range: Range<usize>, _window, cx| {
                let theme = Theme::get(cx);
                let run = match this.selected_run() {
                    Some(r) => r.clone(),
                    None => return Vec::new(),
                };
                let Some(s) = this.selected_series() else {
                    return Vec::new();
                };
                let Some(x_dom) = charts::x_domain(s) else {
                    return Vec::new();
                };
                let panels = metric_panels_for_run(&run, Some(s));
                range
                    .filter_map(|ix| {
                        let panel = panels.get(ix)?;
                        Some(metric_chart::panel(
                            &theme,
                            panel,
                            s,
                            x_dom,
                            this.cursor_x,
                            this.chart_geom.clone(),
                            this.chart_paint.clone(),
                            cx,
                        ))
                    })
                    .collect()
            }),
        )
        .flex_1()
        .min_h_0()
        .w_full(),
    )
}

fn config_card(theme: &Theme, run: &RunScalars) -> Div {
    div()
        .flex_none()
        .px(px(8.))
        .py(px(6.))
        .border_t_1()
        .border_color(theme.border)
        .bg(theme.panel_edge)
        .child(
            div()
                .text_color(theme.amber_dim)
                .text_size(px(10.))
                .mb_1()
                .child("CONFIG"),
        )
        .child(config_line(theme, "DATASET", run.dataset.as_deref()))
        .child(config_line(theme, "GRID", run.grid.as_deref()))
        .child(config_line(theme, "METRIC", run.metric.as_deref()))
        .child(config_line(theme, "SWEEP", run.sweep.as_deref()))
        .child(config_line(
            theme,
            "STARTED",
            Some(&fmt_ago(run.created)),
        ))
}

fn config_line(theme: &Theme, label: &'static str, value: Option<&str>) -> Div {
    div()
        .flex()
        .gap_2()
        .text_size(px(10.))
        .child(
            div()
                .w(px(64.))
                .text_color(theme.text_dim)
                .child(label),
        )
        .child(
            div()
                .text_color(theme.text)
                .child(value.map(|s| s.to_string()).unwrap_or_else(|| "—".into())),
        )
}
