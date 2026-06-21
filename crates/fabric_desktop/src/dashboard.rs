use crate::charts::{new_geoms, new_paint_cache, ChartGeoms, ChartPaintCache};
use crate::columns::{self, rgba_from_hex, RUN_TABLE};
use crate::detail;
use crate::network::{DashboardMsg, NetworkCommand};
use crate::overview::{load_hidden, load_kind, save_hidden, save_kind};
use crate::search_input::SearchInput;
use crate::theme::Theme;
use fabric_health::{
    color_for_key, group_key, pick_sparkline_key, sparkline_values, KindFilter,
};
use fabric_live::{append_point, patch_summary, LiveMessage};
use fabric_types::{
    sort_key_changed, sort_runs, RunScalars, RunSeries, RunsSummary, SortColumn, SortDirection,
    SortState,
};
use crate::sparkline::sparkline_path;
use futures::channel::mpsc;
use std::sync::Arc;
use gpui::{div, prelude::*, px, rgb, uniform_list, AnyElement, Context, Div, Entity, Pixels, SharedString, Window};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::time::{Duration, Instant};

const LIVE_NOTIFY_MIN: Duration = Duration::from_millis(150);
const SPARKLINE_WINDOW: usize = 36;
const MAX_SPARKLINE_FETCHES: usize = 28;

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
    kind_filter: KindFilter,
    hidden_groups: HashSet<String>,
    search: Entity<SearchInput>,
    search_query: String,
    last_live_notify: Option<Instant>,
    cmd_tx: Option<mpsc::UnboundedSender<NetworkCommand>>,
    operator_email: Option<String>,
    selected: Option<RunKey>,
    series: Option<RunSeries>,
    series_loading: bool,
    series_error: Option<SharedString>,
    sparklines: HashMap<RunKey, RunSeries>,
    sparkline_paths: HashMap<RunKey, Arc<gpui::Path<gpui::Pixels>>>,
    sparkline_pending: HashSet<RunKey>,
    /// Shared crosshair x (epoch units) — hovering any metric chart sets it, all charts draw it.
    pub(crate) cursor_x: Option<f64>,
    /// Per-chart plot geometry written at paint time; read by mouse handlers to map px -> epoch.
    pub(crate) chart_geom: ChartGeoms,
    /// Cached grid/line paths so scroll + crosshair repaints avoid rebuilding polylines.
    pub(crate) chart_paint: ChartPaintCache,
}

impl Dashboard {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| SearchInput::new(cx));
        cx.observe(&search, move |this, search, cx| {
            let q = search.read(cx).query().to_string();
            if this.search_query != q {
                this.search_query = q;
                this.rebuild_runs();
                this.queue_sparkline_fetches();
                if this.selected.is_some() && this.selected_run().is_none() {
                    this.selected = None;
                    this.series = None;
                    this.series_loading = false;
                    this.series_error = None;
                    this.cursor_x = None;
                    this.clear_chart_caches();
                }
                cx.notify();
            }
        })
        .detach();

        Self {
            summary: None,
            runs: Vec::new(),
            run_index: HashMap::new(),
            error: None,
            live: false,
            refreshing: false,
            sort: SortState::default(),
            kind_filter: load_kind(),
            hidden_groups: load_hidden(),
            search,
            search_query: String::new(),
            last_live_notify: None,
            cmd_tx: None,
            operator_email: None,
            selected: None,
            series: None,
            series_loading: false,
            series_error: None,
            sparklines: HashMap::new(),
            sparkline_paths: HashMap::new(),
            sparkline_pending: HashSet::new(),
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

    fn upsert_series_cache(&mut self, key: RunKey, series: RunSeries) {
        let replace = match self.sparklines.get(&key) {
            Some(existing) => series.epochs.len() >= existing.epochs.len(),
            None => true,
        };
        if replace {
            self.sparklines.insert(key.clone(), series);
            let run = self
                .runs
                .iter()
                .find(|r| r.pod == key.0 && r.name == key.1)
                .cloned();
            if let Some(run) = run {
                self.sync_sparkline_path(&key, &run);
            }
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
        let mut runs: Vec<RunScalars> = summary
            .runs
            .iter()
            .filter(|r| fabric_health::matches_kind(r, self.kind_filter))
            .filter(|r| fabric_health::matches_search(r, &self.search_query))
            .cloned()
            .collect();
        sort_runs(&mut runs, self.sort);
        self.runs = runs;
        self.rebuild_index();
        if self.selected.is_some() && self.selected_run().is_none() {
            self.selected = None;
            self.series = None;
            self.series_loading = false;
            self.series_error = None;
            self.cursor_x = None;
            self.clear_chart_caches();
        }
    }

    fn queue_sparkline_fetches(&mut self) {
        let mut queued = 0usize;
        for run in &self.runs {
            let key = (run.pod.clone(), run.name.clone());
            if self.sparklines.contains_key(&key) || self.sparkline_pending.contains(&key) {
                continue;
            }
            if queued >= MAX_SPARKLINE_FETCHES {
                break;
            }
            self.sparkline_pending.insert(key.clone());
            if let Some(tx) = &self.cmd_tx {
                let _ = tx.unbounded_send(NetworkCommand::FetchSparkline {
                    pod: key.0,
                    name: key.1,
                });
            }
            queued += 1;
        }
    }

    pub fn set_kind_filter(&mut self, kind: KindFilter, cx: &mut Context<Self>) {
        if self.kind_filter == kind {
            return;
        }
        self.kind_filter = kind;
        save_kind(kind);
        self.rebuild_runs();
        self.queue_sparkline_fetches();
        if self.selected.is_some() && self.selected_run().is_none() {
            self.clear_selection(cx);
        } else {
            cx.notify();
        }
    }

    pub fn toggle_hidden(&mut self, group: String, cx: &mut Context<Self>) {
        if self.hidden_groups.contains(&group) {
            self.hidden_groups.remove(&group);
        } else {
            self.hidden_groups.insert(group);
        }
        save_hidden(&self.hidden_groups);
        cx.notify();
    }

    pub fn toggle_hide_all(&mut self, cx: &mut Context<Self>) {
        let all_hidden = !self.runs.is_empty()
            && self
                .runs
                .iter()
                .all(|r| self.hidden_groups.contains(&group_key(r)));
        if all_hidden {
            self.hidden_groups.clear();
        } else {
            self.hidden_groups = self.runs.iter().map(group_key).collect();
        }
        save_hidden(&self.hidden_groups);
        cx.notify();
    }

    fn sync_sparkline_path(&mut self, key: &RunKey, run: &RunScalars) {
        if let Some(vals) = self.sparkline_values_for(run) {
            if let Some(path) = sparkline_path(&vals) {
                self.sparkline_paths.insert(key.clone(), Arc::new(path));
                return;
            }
        }
        self.sparkline_paths.remove(key);
    }

    fn sparkline_values_for(&self, run: &RunScalars) -> Option<Vec<f64>> {
        let key = (run.pod.clone(), run.name.clone());
        let series = self.sparklines.get(&key)?;
        let metric = pick_sparkline_key(run);
        let vals = sparkline_values(series, &metric, SPARKLINE_WINDOW);
        (vals.len() >= 2).then_some(vals)
    }

    fn sparkline_path_for_run(&self, run: &RunScalars) -> Option<Arc<gpui::Path<gpui::Pixels>>> {
        let key = (run.pod.clone(), run.name.clone());
        self.sparkline_paths.get(&key).cloned()
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

    pub fn attach(&mut self, cmd_tx: mpsc::UnboundedSender<NetworkCommand>) {
        self.cmd_tx = Some(cmd_tx);
    }

    pub fn set_operator_email(&mut self, email: Option<String>) {
        self.operator_email = email;
    }

    pub fn live(&self) -> bool {
        self.live
    }

    pub fn refreshing(&self) -> bool {
        self.refreshing
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some(tx) = &self.cmd_tx {
            self.refreshing = true;
            let _ = tx.unbounded_send(NetworkCommand::RefreshSummary);
            cx.notify();
        }
    }

    pub fn select_run(&mut self, pod: String, name: String, cx: &mut Context<Self>) {
        self.selected = Some((pod.clone(), name.clone()));
        self.series_error = None;
        self.cursor_x = None;
        self.clear_chart_caches();
        let key = (pod.clone(), name.clone());
        if let Some(cached) = self.sparklines.get(&key).cloned() {
            self.series = Some(cached);
            self.series_loading = false;
        } else {
            self.series = None;
            self.series_loading = true;
        }
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

    pub fn handle_msg(&mut self, msg: DashboardMsg, cx: &mut Context<Self>) {
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
                match result {
                    Ok(series) => {
                        self.upsert_series_cache((pod.clone(), name.clone()), series.clone());
                        if self.selected.as_ref() == Some(&(pod.clone(), name.clone())) {
                            self.series_loading = false;
                            self.clear_chart_caches();
                            self.series = Some(series);
                            self.series_error = None;
                        }
                    }
                    Err(e) => {
                        if self.selected.as_ref() == Some(&(pod.clone(), name.clone())) {
                            self.series_loading = false;
                            self.series = None;
                            self.series_error = Some(format!("{e}").into());
                        }
                    }
                }
                cx.notify();
            }
            DashboardMsg::Sparkline { pod, name, result } => {
                self.sparkline_pending.remove(&(pod.clone(), name.clone()));
                if let Ok(series) = result {
                    self.upsert_series_cache((pod, name), series);
                    cx.notify();
                }
            }
        }
    }

    pub fn set_summary(&mut self, summary: RunsSummary, cx: &mut Context<Self>) {
        self.error = None;
        self.summary = Some(summary);
        self.rebuild_runs();
        self.queue_sparkline_fetches();
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
        if !fabric_health::matches_kind(&src, self.kind_filter)
            || !fabric_health::matches_search(&src, &self.search_query)
        {
            if self.run_index.contains_key(&key) {
                self.rebuild_runs();
            }
            return;
        }
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
                let run_key = (ev.pod.clone(), ev.run.clone());
                if !ev.point.is_empty() {
                    if self.selected.as_ref() == Some(&run_key) {
                        if let Some(series) = &mut self.series {
                            append_point(
                                &mut series.epochs,
                                &mut series.metrics,
                                &ev.point,
                                detail::SERIES_MAX_POINTS as usize,
                            );
                            let snapshot = series.clone();
                            self.upsert_series_cache(run_key.clone(), snapshot);
                            self.clear_chart_caches();
                        }
                    } else if let Some(spark) = self.sparklines.get_mut(&run_key) {
                        append_point(
                            &mut spark.epochs,
                            &mut spark.metrics,
                            &ev.point,
                            detail::SERIES_MAX_POINTS as usize,
                        );
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
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(overview_toolbar(self, cx, &theme))
            .child(theme.block().child(body(self, cx, &theme)))
    }
}

fn overview_toolbar(
    view: &Dashboard,
    cx: &mut Context<Dashboard>,
    theme: &Theme,
) -> impl IntoElement {
    let kinds = [KindFilter::All, KindFilter::Recon, KindFilter::Lm];
    let kind_pills: Vec<_> = kinds
        .into_iter()
        .map(|k| {
            let active = view.kind_filter == k;
            theme
                .filter_pill(active, k.label())
                .id(format!("kind-{}", k.label()))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.set_kind_filter(k, cx);
                }))
        })
        .collect();

    let stale = view.summary.as_ref().and_then(|s| s.stale).unwrap_or(false);

    div()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .w_full()
        .h(px(26.))
        .px(px(8.))
        .bg(theme.bg)
        .border_b_1()
        .border_color(theme.border)
        .child(view.search.clone())
        .child(
            div()
                .flex()
                .items_center()
                .gap_1()
                .children(kind_pills),
        )
        .child(div().flex_1())
        .when(stale, |el| {
            el.child(
                div()
                    .px(px(6.))
                    .py(px(2.))
                    .border_1()
                    .border_color(theme.amber)
                    .text_size(px(9.))
                    .text_color(theme.amber)
                    .child("STALE SNAPSHOT"),
            )
        })
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
    let total = summary.runs.len();
    let shown = view.runs.len();
    let run_label = if shown != total {
        format!("{shown}/{total} RUNS")
    } else {
        format!("{shown} RUNS")
    };
    let footer = format!(
        "{} │ GPU {}/{} │ {} │ {}{}",
        run_label,
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
            .child(theme.status_bar(
                footer,
                view.operator_email
                    .clone()
                    .map(SharedString::from),
            ))
            .into_any_element();
    }

    div()
        .flex_1()
        .min_h_0()
        .w_full()
        .flex()
        .flex_col()
        .child(run_list(view, cx, theme, None).flex_1())
        .child(theme.status_bar(
            footer,
            view.operator_email.clone().map(SharedString::from),
        ))
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
            if view.runs.is_empty() {
                empty_filter_panel(theme).into_any_element()
            } else {
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
                .w_full()
                .into_any_element()
            }
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

fn empty_filter_panel(theme: &Theme) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_color(theme.text_dim)
        .child("NO RUNS MATCH FILTER")
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
    let all_hidden = !view.runs.is_empty()
        && view
            .runs
            .iter()
            .all(|r| view.hidden_groups.contains(&group_key(r)));
    let eye_label = if all_hidden { "◌" } else { "◉" };

    let eye = columns::eye_button(theme, all_hidden, eye_label)
        .id("hdr-eye")
        .on_click(cx.listener(|this, _, _, cx| {
            this.toggle_hide_all(cx);
        }));

    let data_headers: Vec<_> = RUN_TABLE
        .iter()
        .filter(|col| col.id != "eye")
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

    let header_cells: Vec<AnyElement> = std::iter::once(eye.into_any_element())
        .chain(data_headers.into_iter().map(|c| c.into_any_element()))
        .collect();

    theme
        .table_header_row()
        .child(theme.col_row(header_cells))
}

fn run_row(
    view: &Dashboard,
    cx: &mut Context<Dashboard>,
    theme: &Theme,
    ix: usize,
    run: &RunScalars,
) -> impl IntoElement {
    let gk = group_key(run);
    let hidden = view.hidden_groups.contains(&gk);
    let selected = view.selected.as_ref() == Some(&(run.pod.clone(), run.name.clone()));
    let stripe = if selected {
        theme.panel_edge
    } else if hidden {
        theme.row_b
    } else if ix % 2 == 0 {
        theme.row_a
    } else {
        theme.row_b
    };

    let pod = run.pod.clone();
    let name = run.name.clone();
    let spark_path = view.sparkline_path_for_run(run);
    let spark_color = rgba_from_hex(color_for_key(&gk));
    let eye_label = if hidden { "◌" } else { "◉" };
    let group = gk.clone();
    let eye = columns::eye_button(theme, hidden, eye_label)
        .id(format!("eye-{ix}"))
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_hidden(group.clone(), cx);
        }));

    let data_cells: Vec<_> = RUN_TABLE
        .iter()
        .filter(|col| col.id != "eye")
        .map(|col| {
            columns::render_cell(
                theme,
                col,
                run,
                hidden,
                spark_path.clone(),
                spark_color,
            )
        })
        .collect();

    let row_cells: Vec<AnyElement> = std::iter::once(eye.into_any_element())
        .chain(data_cells.into_iter().map(|c| c.into_any_element()))
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
        .when(hidden, |el| el.opacity(0.45))
        .cursor_pointer()
        .hover(|s| s.bg(theme.panel_edge))
        .on_click(cx.listener({
            let pod = pod.clone();
            let name = name.clone();
            move |this, _, _, cx| {
                this.select_run(pod.clone(), name.clone(), cx);
            }
        }))
        .child(theme.col_row(row_cells))
}
