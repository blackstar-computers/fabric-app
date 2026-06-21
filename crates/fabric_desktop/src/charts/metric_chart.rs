//! One synchronized metric panel: header readout + GPUI canvas line chart + mouse-driven crosshair.
//!
//! Hovering any panel sets a shared crosshair epoch; the header readout (right) shows the value
//! at that epoch while hovering, otherwise the latest probe value.

use fabric_health::MetricPanel;
use fabric_types::RunSeries;
use gpui::{
    canvas, div, point, prelude::*, px, Bounds, MouseMoveEvent, Path, PathBuilder, Pixels, Rgba,
    Window,
};

use crate::charts::{
    build_xy, fmt_epoch_tick, fmt_readout, fmt_tick, y_domain, ChartGeoms, ChartPaintCache,
    PanelPaintCache, PlotGeom, Xy,
};
use crate::dashboard::Dashboard;
use crate::theme::Theme;

const CHART_H: f32 = 120.0;
const Y_GUTTER: f32 = 44.0;
const MAX_PAINT_POINTS: usize = 400;

/// Fixed row height for [`gpui::uniform_list`] virtualization in the metric wall.
///
/// `uniform_list` measures the first item and positions every row at this exact pitch, so the
/// constant must fit the panel's full content or rows visually overlap. Budget (border-box):
/// py(6)*2 = 12, header row 18, chart row mt_1(4) + 120, x-axis row mt_1(4) + 14, border_b 1
/// = 173. Rounded up to 176 for a little slack; `overflow_hidden()` on the root clips any
/// residual so neighbouring panels never bleed into each other.
const PANEL_HEADER_H: f32 = 18.0;
const PANEL_XAXIS_H: f32 = 14.0;
pub const METRIC_PANEL_ITEM_H: Pixels = px(176.);

/// Render a single metric panel (title + readout + line chart with hover sync).
/// `x_dom` is the wall-wide epoch domain so every panel's x-axis lines up for sync.
#[allow(clippy::too_many_arguments)]
pub fn panel(
    theme: &Theme,
    panel: &MetricPanel,
    series: &RunSeries,
    x_dom: (f64, f64),
    cursor_x: Option<f64>,
    geoms: ChartGeoms,
    paint_cache: ChartPaintCache,
    cx: &mut Context<Dashboard>,
) -> impl IntoElement {
    let Some(xy) = build_xy(series, &panel.series_key) else {
        return waiting_panel(theme, &panel.title);
    };
    let y_dom = y_domain(&xy);

    let hover_val = cursor_x.and_then(|x| series.value_at_epoch(&panel.series_key, x as i64));
    let latest_val = series.latest(&panel.series_key);
    let readout_val = hover_val.or(latest_val);
    let readout = readout_val
        .map(|v| fmt_readout(v, panel.pct, panel.unit.as_deref()))
        .unwrap_or_else(|| "—".into());
    let readout_color = if cursor_x.is_some() {
        theme.live
    } else {
        theme.data
    };

    let key = panel.id.clone();
    let line_color = theme.live;
    let grid_color = theme.border;
    let cursor_color = with_alpha(theme.link, 0.6);

    let draw_crosshair = cursor_x.is_some();
    let canvas_el = {
        let xy = xy.clone();
        let geoms = geoms.clone();
        let paint_cache = paint_cache.clone();
        let key = key.clone();
        let crosshair_x = if draw_crosshair { cursor_x } else { None };
        canvas(
            move |bounds, _window, _cx| bounds,
            move |bounds, _state, window, _cx| {
                geoms.borrow_mut().insert(key.clone(), geom_for(bounds, x_dom));
                paint_series(
                    &key,
                    bounds,
                    window,
                    &xy,
                    x_dom,
                    y_dom,
                    &paint_cache,
                    line_color,
                    grid_color,
                    crosshair_x,
                    cursor_color,
                );
            },
        )
        .size_full()
    };

    let chart_area = div()
        .id(panel.id.clone())
        .flex_1()
        .h_full()
        .child(canvas_el)
        .on_mouse_move(cx.listener({
            let geoms = geoms.clone();
            let key = key.clone();
            move |this, ev: &MouseMoveEvent, _w, cx| {
                if let Some(g) = geoms.borrow().get(&key).copied() {
                    if let Some(xv) = g.px_to_x(f32::from(ev.position.x)) {
                        this.set_cursor_x(Some(xv), cx);
                    }
                }
            }
        }))
        ;

    div()
        .flex_none()
        .h(METRIC_PANEL_ITEM_H)
        .overflow_hidden()
        .px(px(8.))
        .py(px(6.))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_none()
                .h(px(PANEL_HEADER_H))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_color(theme.amber_dim)
                        .text_size(px(10.))
                        .child(panel.title.clone()),
                )
                .child(
                    div()
                        .text_color(readout_color)
                        .text_size(px(11.))
                        .child(readout),
                ),
        )
        .child(
            div()
                .mt_1()
                .flex()
                .h(px(CHART_H))
                .child(y_axis(theme, y_dom, panel.pct))
                .child(chart_area),
        )
        .child(x_axis(theme, x_dom))
}

fn waiting_panel(theme: &Theme, title: &str) -> gpui::Div {
    div()
        .flex_none()
        .h(METRIC_PANEL_ITEM_H)
        .overflow_hidden()
        .px(px(8.))
        .py(px(6.))
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .text_color(theme.amber_dim)
                .text_size(px(10.))
                .child(title.to_string()),
        )
        .child(
            div()
                .mt_1()
                .h(px(CHART_H))
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.text_dim)
                .text_size(px(10.))
                .child("waiting for probe samples…"),
        )
}

fn y_axis(theme: &Theme, y_dom: (f64, f64), pct: bool) -> gpui::Div {
    div()
        .flex_none()
        .w(px(Y_GUTTER))
        .h_full()
        .flex()
        .flex_col()
        .justify_between()
        .items_end()
        .pr(px(4.))
        .text_size(px(9.))
        .text_color(theme.text_dim)
        .child(div().child(fmt_tick(y_dom.1, pct)))
        .child(div().child(fmt_tick(y_dom.0, pct)))
}

fn x_axis(theme: &Theme, x_dom: (f64, f64)) -> gpui::Div {
    div()
        .flex_none()
        .h(px(PANEL_XAXIS_H))
        .flex()
        .mt_1()
        .text_size(px(9.))
        .text_color(theme.text_dim)
        .child(div().flex_none().w(px(Y_GUTTER)))
        .child(
            div()
                .flex_1()
                .flex()
                .justify_between()
                .child(div().child(fmt_epoch_tick(x_dom.0)))
                .child(div().child(fmt_epoch_tick(x_dom.1))),
        )
}

fn geom_for(bounds: Bounds<Pixels>, x_dom: (f64, f64)) -> PlotGeom {
    PlotGeom {
        plot_left: f32::from(bounds.origin.x),
        plot_width: f32::from(bounds.size.width),
        x_min: x_dom.0,
        x_max: x_dom.1,
    }
}

fn paint_indices(len: usize, budget: usize) -> Vec<usize> {
    let budget = budget.clamp(2, MAX_PAINT_POINTS);
    if len <= budget {
        return (0..len).collect();
    }
    let step = (len - 1) as f64 / (budget - 1) as f64;
    let mut idx = Vec::with_capacity(budget);
    for i in 0..budget {
        idx.push(((i as f64) * step).round() as usize);
    }
    idx.sort_unstable();
    idx.dedup();
    if *idx.last().unwrap_or(&0) != len - 1 {
        idx.push(len - 1);
    }
    idx
}

/// Build the grid + line paths in **local** coordinates (origin `0,0` relative to the canvas
/// bounds). The cache is keyed by `w/h/point_count` only, so the same paths can be reused as the
/// panel scrolls; [`paint_series`] translates them to the live `bounds.origin` at paint time.
/// Baking window-absolute coordinates here instead would desync the chart from its scrolling chrome
/// (the cache would hit on a stale absolute Y while titles/labels move with the list).
fn build_panel_paths(
    w: f32,
    h: f32,
    xy: &Xy,
    x_dom: (f64, f64),
    y_dom: (f64, f64),
) -> PanelPaintCache {
    let (x_min, x_max) = x_dom;
    let (y_min, y_max) = y_dom;
    let x_span = (x_max - x_min).max(1e-9);
    let y_span = (y_max - y_min).max(1e-9);

    let x_px = |xv: f64| -> f32 { ((xv - x_min) / x_span) as f32 * w };
    let y_px = |yv: f64| -> f32 { (1.0 - ((yv - y_min) / y_span) as f32) * h };

    let mut grid = PathBuilder::stroke(px(1.0));
    for i in 0..=2 {
        let gy = (i as f32 / 2.0) * h;
        grid.move_to(point(px(0.0), px(gy)));
        grid.line_to(point(px(w), px(gy)));
    }
    let grid = grid.build().expect("grid path");

    let mut line = PathBuilder::stroke(px(1.5));
    let mut pen_down = false;
    let mut drew = false;
    let point_budget = (w.ceil() as usize).clamp(2, MAX_PAINT_POINTS);
    for i in paint_indices(xy.x.len(), point_budget) {
        match xy.y[i] {
            Some(yv) => {
                let p = point(px(x_px(xy.x[i])), px(y_px(yv)));
                if pen_down {
                    line.line_to(p);
                    drew = true;
                } else {
                    line.move_to(p);
                    pen_down = true;
                }
            }
            None => pen_down = false,
        }
    }
    let line = drew.then(|| line.build().expect("line path"));

    PanelPaintCache {
        width: w,
        height: h,
        point_count: xy.x.len(),
        last_epoch: xy.x.last().copied().unwrap_or(0.0) as i64,
        grid,
        line,
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_series(
    cache_key: &str,
    bounds: Bounds<Pixels>,
    window: &mut Window,
    xy: &Xy,
    x_dom: (f64, f64),
    y_dom: (f64, f64),
    paint_cache: &ChartPaintCache,
    line_color: Rgba,
    grid_color: Rgba,
    cursor_x: Option<f64>,
    cursor_color: Rgba,
) {
    let left = f32::from(bounds.origin.x);
    let top = f32::from(bounds.origin.y);
    let w = f32::from(bounds.size.width);
    let h = f32::from(bounds.size.height);
    if w <= 1.0 || h <= 1.0 {
        return;
    }

    let cached = {
        let mut cache = paint_cache.borrow_mut();
        let last_epoch = xy.x.last().copied().unwrap_or(0.0) as i64;
        let stale = cache.get(cache_key).is_none_or(|c| {
            c.width != w
                || c.height != h
                || c.point_count != xy.x.len()
                || c.last_epoch != last_epoch
        });
        if stale {
            let built = build_panel_paths(w, h, xy, x_dom, y_dom);
            cache.insert(cache_key.to_string(), built);
        }
        cache.get(cache_key).cloned().expect("cache entry")
    };

    // Cached paths live in local (0,0-origin) space; offset to the live bounds so the chart tracks
    // its panel as the `uniform_list` scrolls (window.paint_path bakes absolute coords and ignores
    // element offsets, so the translation must be applied to the path itself).
    let origin = bounds.origin;
    window.paint_path(translate_path(&cached.grid, origin), with_alpha(grid_color, 0.5));
    if let Some(line) = cached.line.as_ref() {
        window.paint_path(translate_path(line, origin), line_color);
    }

    let (x_min, x_max) = x_dom;
    let x_span = (x_max - x_min).max(1e-9);
    let x_px = |xv: f64| -> f32 { left + ((xv - x_min) / x_span) as f32 * w };

    if let Some(cv) = cursor_x {
        if cv >= x_min && cv <= x_max {
            paint_vline(window, x_px(cv), top, h, cursor_color);
        }
    }
}

fn paint_vline(window: &mut Window, x: f32, top: f32, h: f32, color: Rgba) {
    let mut b = PathBuilder::stroke(px(1.0));
    b.move_to(point(px(x), px(top)));
    b.line_to(point(px(x), px(top + h)));
    if let Ok(p) = b.build() {
        window.paint_path(p, color);
    }
}

/// Offset a cached local-space path into window-absolute space by `origin`. Only the rendered
/// geometry (vertices + bounds) needs shifting; `paint_path` consumes the vertices directly.
fn translate_path(path: &Path<Pixels>, origin: gpui::Point<Pixels>) -> Path<Pixels> {
    let mut shifted = path.clone();
    for v in shifted.vertices.iter_mut() {
        v.xy_position += origin;
    }
    shifted.bounds.origin += origin;
    shifted
}

fn with_alpha(c: Rgba, a: f32) -> Rgba {
    Rgba { a, ..c }
}
