use crate::charts::{new_geoms, new_paint_cache, ChartGeoms, ChartPaintCache};
use crate::columns::{self, RUN_TABLE};
use crate::detail;
use crate::network::{spawn_dashboard_network, DashboardMsg, NetworkCommand};
use crate::theme::Theme;
use fabric_api::{default_portal_url, load_service_token, Client};
use fabric_live::{append_point, patch_summary, LiveMessage};
use fabric_types::{
    sort_key_changed, sort_runs, RunScalars, RunSeries, RunsSummary, SortColumn, SortDirection,
    SortState,
};
use futures::channel::mpsc;
use futures::StreamExt;
use gpui::{div, prelude::*, px, rgb, uniform_list, Context, Div, Pixels, SharedString, Window};
use std::collections::HashMap;
use std::ops::Range;
use std::time::{Duration, Instant};

const LIVE_NOTIFY_MIN: Duration = Duration::from_millis(150);

/// Run list width beside the War Room — wide enough for every column incl. STARTED.
const SPLIT_LIST_W: Pixels = columns::RUN_TABLE_MIN_W;

type RunKey = (String, String);

pub struct Dashboard {
    summary: Option<RunsSummary>,
    runs: Vec<RunScalars>,
    run_index: HashMap<RunKey, usize>,
    error: Option<SharedString>,
    live: bool,
    refreshing: bool,
    sort: SortState,
    last_live_notify: Option<Instant>,
    cmd_tx: Option<mpsc::UnboundedSender<NetworkCommand>>,
    selected: Option<RunKey>,
    series: Option<RunSeries>,
    series_loading: bool,
    series_error: Option<SharedString>,
    /// Shared crosshair x (epoch units) — hovering any metric chart sets it, all charts draw it.
    pub(crate) cursor_x: Option<f64>,
    /// Per-chart plot geometry written at paint time; read by mouse handlers to map px -> epoch.
    pub(crate) chart_geom: ChartGeoms,
    /// Cached grid/line paths so scroll + crosshair repaints avoid rebuilding polylines.
    pub(crate) chart_paint: ChartPaintCache,
}

impl Dashboard {
    pub fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            summary: None,
            runs: Vec::new(),
            run_index: HashMap::new(),
            error: None,
            live: false,
            refreshing: false,
            sort: SortState::default(),
            last_live_notify: None,
            cmd_tx: None,
            selected: None,
            series: None,
            series_loading: false,
            series_error: None,
            cursor_x: None,
            chart_geom: new_geoms(),
            chart_paint: new_paint_cache(),
        }
    }

    fn clear_chart_caches(&mut self) {
        self.chart_geom.borrow_mut().clear();
        self.chart_paint.borrow_mut().clear();
    }

    pub fn set_cursor_x(&mut self, x: Option<f64>, cx: &mut Context<Self>) {
        let x = x.map(f64::round);
        if self.cursor_x != x {
            self.cursor_x = x;
            cx.notify();
        }
    }

    fn rebuild_index(&mut self) {
        self.run_index = self
            .runs
            .iter()
            .enumerate()
            .map(|(i, r)| ((r.pod.clone(), r.name.clone()), i))
            .collect();
    }

    fn rebuild_runs(&mut self) {
        let Some(summary) = &self.summary else {
            self.runs.clear();
            self.run_index.clear();
            return;
        };
        let mut runs = summary.runs.clone();
        sort_runs(&mut runs, self.sort);
        self.runs = runs;
        self.rebuild_index();
    }

    pub fn selected_run(&self) -> Option<&RunScalars> {
        let (pod, name) = self.selected.as_ref()?;
        self.runs
            .iter()
            .find(|r| r.pod == *pod && r.name == *name)
    }

    pub fn selected_series(&self) -> Option<&RunSeries> {
        self.series.as_ref()
    }

    pub fn start(&mut self, cx: &mut Context<Self>) {
        let Ok(token) = load_service_token() else {
            self.set_error(
                "AUTH FAIL — run `fabric auth <token>` or set FABRIC_SERVICE_TOKEN",
                cx,
            );
            return;
        };

        let client = Client::new(default_portal_url(), token);
        let (ui_tx, mut ui_rx) = mpsc::unbounded::<DashboardMsg>();
        let (cmd_tx, cmd_rx) = mpsc::unbounded::<NetworkCommand>();
        self.cmd_tx = Some(cmd_tx);

        spawn_dashboard_network(client, ui_tx, cmd_rx);

        cx.spawn(async move |this, cx| {
            while let Some(msg) = ui_rx.next().await {
                if this
                    .update(cx, |view, cx| view.handle_msg(msg, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.unbounded_send(NetworkCommand::RefreshSummary);
            cx.notify();
        }
    }

    pub fn select_run(&mut self, pod: String, name: String, cx: &mut Context<Self>) {
        self.selected = Some((pod.clone(), name.clone()));
        self.series = None;
        self.series_error = None;
        self.series_loading = true;
        self.cursor_x = None;
        self.clear_chart_caches();
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.unbounded_send(NetworkCommand::FetchSeries { pod, name });
        }
        cx.notify();
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selected = None;
        self.series = None;
        self.series_loading = false;
        self.series_error = None;
        self.cursor_x = None;
        self.clear_chart_caches();
        cx.notify();
    }

    pub fn toggle_sort(&mut self, column: SortColumn, cx: &mut Context<Self>) {
        if self.sort.column == column {
            self.sort.direction = match self.sort.direction {
                SortDirection::Asc => SortDirection::Desc,
                SortDirection::Desc => SortDirection::Asc,
            };
        } else {
            self.sort.column = column;
            self.sort.direction = SortDirection::Desc;
        }
        self.rebuild_runs();
        cx.notify();
    }

    fn handle_msg(&mut self, msg: DashboardMsg, cx: &mut Context<Self>) {
        match msg {
            DashboardMsg::Summary(Ok(summary)) => {
                self.refreshing = false;
                self.set_summary(summary, cx);
            }
            DashboardMsg::Summary(Err(e)) => {
                self.refreshing = false;
                self.set_error(format!("API ERR — {e}"), cx);
            }
            DashboardMsg::Live(live) => self.handle_live(live, cx),
            DashboardMsg::RefreshStarted => {
                self.refreshing = true;
                cx.notify();
            }
            DashboardMsg::Series { pod, name, result } => {
                if self.selected.as_ref() != Some(&(pod, name)) {
                    return;
                }
                self.series_loading = false;
                match result {
                    Ok(series) => {
                        self.clear_chart_caches();
                        self.series = Some(series);
                        self.series_error = None;
                    }
                    Err(e) => {
                        self.series = None;
                        self.series_error = Some(format!("{e}").into());
                    }
                }
                cx.notify();
            }
            DashboardMsg::Sparkline { .. } => {}
        }
    }

    pub fn set_summary(&mut self, summary: RunsSummary, cx: &mut Context<Self>) {
        self.error = None;
        self.summary = Some(summary);
        self.rebuild_runs();
        if self.selected.is_some() && self.selected_run().is_none() {
            self.clear_selection(cx);
        } else {
            cx.notify();
        }
    }

    pub fn set_error(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.error = Some(message.into());
        cx.notify();
    }

    fn apply_run_patch(&mut self, pod: &str, name: &str, src: RunScalars) {
        let key = (pod.to_string(), name.to_string());
        let Some(&ix) = self.run_index.get(&key) else {
            self.rebuild_runs();
            return;
        };

        let old = self.runs[ix].clone();
        self.runs[ix] = src;
        if sort_key_changed(&old, &self.runs[ix], self.sort.column) {
            sort_runs(&mut self.runs, self.sort);
            self.rebuild_index();
        }
    }

    pub fn handle_live(&mut self, msg: LiveMessage, cx: &mut Context<Self>) {
        let force = matches!(msg, LiveMessage::Connected | LiveMessage::Disconnected);
        match msg {
            LiveMessage::Connected => self.live = true,
            LiveMessage::Disconnected => self.live = false,
            LiveMessage::JobEvent(_) => {}
                        LiveMessage::RunEvent(ev) => {
                if self.selected.as_ref() == Some(&(ev.pod.clone(), ev.run.clone())) {
                    if !ev.point.is_empty() {
                        if let Some(series) = &mut self.series {
                            append_point(
                                &mut series.epochs,
                                &mut series.metrics,
                                &ev.point,
                                detail::SERIES_MAX_POINTS as usize,
                            );
                        }
                    }
                }
                if let Some(summary) = self.summary.as_mut() {
                    if ev.is_run_v2() {
                        if patch_summary(summary, &ev) {
                            let pod = ev.pod.clone();
                            let name = ev.run.clone();
                            if let Some(src) = summary
                                .runs
                                .iter()
                                .find(|r| r.pod == pod && r.name == name)
                                .cloned()
                            {
                                self.apply_run_patch(&pod, &name, src);
                            }
                        } else {
                            self.rebuild_runs();
                        }
                    }
                }
            }
        }
        self.notify_live(cx, force);
    }

    fn notify_live(&mut self, cx: &mut Context<Self>, force: bool) {
        let now = Instant::now();
        if force
            || self
                .last_live_notify
                .is_none_or(|t| now.duration_since(t) >= LIVE_NOTIFY_MIN)
        {
            self.last_live_notify = Some(now);
            cx.notify();
        }
    }
}

impl Render for Dashboard {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        let refresh = theme
            .title_button(
                if self.refreshing {
                    " FETCH… "
                } else {
                    " ↻ REFRESH "
                },
                self.refreshing,
            )
            .id("refresh")
            .on_click(cx.listener(|this, _, _, cx| this.refresh(cx)));

        theme
            .shell()
            .child(theme.title_bar(self.live, refresh))
            .child(theme.block().child(body(self, cx, &theme)))
    }
}

fn body(view: &Dashboard, cx: &mut Context<Dashboard>, theme: &Theme) -> impl IntoElement {
    if let Some(err) = &view.error {
        return fault_panel(theme, err).into_any_element();
    }

    if view.summary.is_none() {
        return loading_panel(theme).into_any_element();
    }

    let summary = view.summary.as_ref().unwrap();
    let stream = if view.live { "SSE OK" } else { "SSE ---" };
    let sel = view
        .selected
        .as_ref()
        .map(|(p, n)| format!(" │ SEL {n}@{p}"))
        .unwrap_or_default();
    let sort = format!(
        "SORT {} {}",
        sort_column_label(view.sort.column),
        if view.sort.direction == SortDirection::Desc {
            "▼"
        } else {
            "▲"
        }
    );
    let footer = format!(
        "{} RUNS │ GPU {}/{} │ {} │ {}{}",
        view.runs.len(),
        summary.gpus.active.unwrap_or(0),
        summary.gpus.total.unwrap_or(0),
        stream,
        sort,
        sel,
    );

    if let Some(run) = view.selected_run() {
        let run = run.clone();
        let back = theme
            .title_button(" ← BACK ", false)
            .id("detail-back")
            .on_click(cx.listener(|this, _, _, cx| this.clear_selection(cx)));

        let split_list = run_list(view, cx, theme, Some(columns::RUN_TABLE_MIN_W));

        return div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(
                        split_list
                            .w(SPLIT_LIST_W)
                            .flex_none(),
                    )
                    .child(detail::render_detail(
                        theme,
                        &run,
                        view.series.as_ref(),
                        view.series_loading,
                        view.series_error.as_ref(),
                        back,
                        cx,
                    )),
            )
            .child(theme.status_bar(footer))
            .into_any_element();
    }

    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .child(run_list(view, cx, theme, None).flex_1())
        .child(theme.status_bar(footer))
        .into_any_element()
}

fn run_list(
    view: &Dashboard,
    cx: &mut Context<Dashboard>,
    theme: &Theme,
    min_width: Option<Pixels>,
) -> Div {
    let theme_rows = theme.clone();
    let mut shell = div().min_h_0().flex().flex_col();
    if let Some(w) = min_width {
        shell = shell.min_w(w);
    }
    shell
        .child(column_header(view, cx, theme))
        .child(
            uniform_list(
                "run-list",
                view.runs.len(),
                cx.processor(move |this, range: Range<usize>, _window, cx| {
                    range
                        .filter_map(|ix| {
                            this.runs.get(ix).map(|run| {
                                run_row(this, cx, &theme_rows, ix, run)
                            })
                        })
                        .collect()
                }),
            )
            .flex_1()
            .min_h_0()
            .w_full(),
        )
}

fn sort_column_label(column: SortColumn) -> &'static str {
    match column {
        SortColumn::Name => "RUN",
        SortColumn::Best => "BEST",
        SortColumn::Epoch => "EPOCH",
        SortColumn::Status => "ST",
        SortColumn::Created => "STARTED",
    }
}

fn fault_panel(theme: &Theme, msg: &SharedString) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .px(px(12.))
                .py(px(8.))
                .bg(rgb(0x1a0000))
                .border_2()
                .border_color(theme.warn)
                .text_color(theme.warn)
                .child(format!("■ {msg}")),
        )
}

fn loading_panel(theme: &Theme) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.amber)
        .child("▸ LOADING RUNS…")
}

fn column_header(
    view: &Dashboard,
    cx: &mut Context<Dashboard>,
    theme: &Theme,
) -> impl IntoElement {
    let cells: Vec<_> = RUN_TABLE
        .iter()
        .map(|col| {
            let active = col.sort == Some(view.sort.column);
            let desc = view.sort.direction == SortDirection::Desc;
            let label = columns::header_label(col, active, desc);
            let cell = columns::header_shell(theme, col, label).id(format!("hdr-{}", col.id));

            if let Some(sort_col) = col.sort {
                cell.cursor_pointer()
                    .hover(|s| s.bg(theme.panel_edge))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_sort(sort_col, cx);
                    }))
            } else {
                cell
            }
        })
        .collect();

    theme.table_header_row().child(theme.col_row(cells))
}

fn run_row(
    view: &Dashboard,
    cx: &mut Context<Dashboard>,
    theme: &Theme,
    ix: usize,
    run: &RunScalars,
) -> impl IntoElement {
    let selected = view.selected.as_ref() == Some(&(run.pod.clone(), run.name.clone()));
    let stripe = if selected {
        theme.panel_edge
    } else if ix % 2 == 0 {
        theme.row_a
    } else {
        theme.row_b
    };

    let pod = run.pod.clone();
    let name = run.name.clone();
    let cells: Vec<_> = RUN_TABLE
        .iter()
        .map(|col| columns::render_cell(theme, col, run))
        .collect();

    div()
        .id(ix)
        .w_full()
        .h(px(theme.row_h))
        .flex()
        .items_center()
        .px(px(8.))
        .bg(stripe)
        .border_b_1()
        .border_color(if selected {
            theme.amber
        } else {
            theme.border
        })
        .cursor_pointer()
        .hover(|s| s.bg(theme.panel_edge))
        .on_click(cx.listener({
            let pod = pod.clone();
            let name = name.clone();
            move |this, _, _, cx| {
                this.select_run(pod.clone(), name.clone(), cx);
            }
        }))
        .child(theme.col_row(cells))
}
