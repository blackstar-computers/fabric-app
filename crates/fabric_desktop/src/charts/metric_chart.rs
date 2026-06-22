//! One synchronized metric panel: header readout + GPUI canvas line chart + mouse-driven crosshair.

use fabric_health::MetricPanel;
use fabric_types::RunSeries;
use gpui::{
    canvas, div, point, prelude::*, px, relative, Bounds, MouseMoveEvent, Path, PathBuilder,
    Pixels, Rgba, Window,
};

use crate::charts::{
    build_xy, fmt_epoch_tick, fmt_readout, fmt_tick, y_domain, ChartGeoms, ChartPaintCache,
    Crosshair, Domain, PanelPaintCache, PlotGeom, PlotScale, Xy,
};
use crate::dashboard::Dashboard;
use crate::theme::Theme;

const CHART_H: f32 = 120.0;
const Y_GUTTER: f32 = 44.0;
const MAX_PAINT_POINTS: usize = 2000;
const LINE_WIDTH: f32 = 1.6;
const CURSOR_POINT_R: f32 = 3.0;

const PANEL_HEADER_H: f32 = 18.0;
const PANEL_XAXIS_H: f32 = 14.0;
pub const METRIC_PANEL_ITEM_H: Pixels = px(176.);

#[allow(clippy::too_many_arguments)]
pub fn panel(
    theme: &Theme,
    panel: &MetricPanel,
    series: &RunSeries,
    x_dom: Domain,
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
    let crosshair = cursor_x.map(|x| Crosshair::from_cursor(x, hover_val));

    let canvas_el = {
        let xy = xy.clone();
        let geoms = geoms.clone();
        let paint_cache = paint_cache.clone();
        let key = key.clone();
        canvas(
            move |bounds, _window, _cx| bounds,
            move |bounds, _state, window, _cx| {
                geoms
                    .borrow_mut()
                    .insert(key.clone(), geom_for(bounds, x_dom));
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
                    crosshair,
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
        }));

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
        .child(x_axis(theme, x_dom, cursor_x))
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

fn y_axis(theme: &Theme, y_dom: Domain, pct: bool) -> gpui::Div {
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
        .child(div().child(fmt_tick(y_dom.max, pct)))
        .child(div().child(fmt_tick(y_dom.min, pct)))
}

fn x_axis(theme: &Theme, x_dom: Domain, cursor_x: Option<f64>) -> gpui::Div {
    let cursor_in_dom = cursor_x.filter(|&cv| x_dom.contains(cv));

    let mut plot_axis = div()
        .flex_1()
        .relative()
        .h_full()
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .text_color(theme.text_dim)
                .child(fmt_epoch_tick(x_dom.min)),
        )
        .child(
            div()
                .absolute()
                .top_0()
                .right_0()
                .text_color(theme.text_dim)
                .child(fmt_epoch_tick(x_dom.max)),
        );

    if let Some(cv) = cursor_in_dom {
        let frac = x_dom.fraction(cv).clamp(0.06, 0.94);
        plot_axis = plot_axis.child(
            div()
                .absolute()
                .top_0()
                .left(relative(frac))
                .text_color(theme.live)
                .child(fmt_epoch_tick(cv)),
        );
    }

    div()
        .flex_none()
        .h(px(PANEL_XAXIS_H))
        .flex()
        .mt_1()
        .text_size(px(9.))
        .child(div().flex_none().w(px(Y_GUTTER)))
        .child(plot_axis)
}

fn geom_for(bounds: Bounds<Pixels>, x_dom: Domain) -> PlotGeom {
    PlotGeom::from_bounds(
        f32::from(bounds.origin.x),
        f32::from(bounds.size.width),
        x_dom,
    )
}

fn paint_indices(xy: &Xy, x_dom: Domain, w: f32) -> Vec<usize> {
    let len = xy.x.len();
    let budget = ((w.ceil() as usize) * 2).clamp(4, MAX_PAINT_POINTS);
    if len <= budget {
        return (0..len).collect();
    }

    let cols = (w.ceil() as usize).max(2);
    let x_col = |i: usize| -> usize {
        let t = x_dom.fraction(xy.x[i]);
        (t * (cols - 1) as f32).round() as usize
    };

    let mut out = Vec::with_capacity(budget);
    let mut i = 0;
    while i < len {
        if xy.y[i].is_none() {
            out.push(i);
            while i < len && xy.y[i].is_none() {
                i += 1;
            }
            continue;
        }
        let seg_start = i;
        while i < len && xy.y[i].is_some() {
            i += 1;
        }
        decimate_segment(xy, seg_start, i, x_col, cols, &mut out);
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn decimate_segment(
    xy: &Xy,
    start: usize,
    end: usize,
    x_col: impl Fn(usize) -> usize,
    cols: usize,
    out: &mut Vec<usize>,
) {
    if end - start <= cols * 2 {
        out.extend(start..end);
        return;
    }

    let mut col = x_col(start);
    let mut min_i = start;
    let mut max_i = start;

    for i in start..end {
        let c = x_col(i);
        if c != col {
            push_min_max(out, xy, min_i, max_i);
            col = c;
            min_i = i;
            max_i = i;
        } else {
            let y = xy.y[i].expect("segment");
            if y < xy.y[min_i].expect("segment") {
                min_i = i;
            }
            if y > xy.y[max_i].expect("segment") {
                max_i = i;
            }
        }
    }
    push_min_max(out, xy, min_i, max_i);
}

fn push_min_max(out: &mut Vec<usize>, xy: &Xy, min_i: usize, max_i: usize) {
    if min_i == max_i {
        out.push(min_i);
    } else if xy.x[min_i] <= xy.x[max_i] {
        out.push(min_i);
        out.push(max_i);
    } else {
        out.push(max_i);
        out.push(min_i);
    }
}

fn build_panel_paths(w: f32, h: f32, xy: &Xy, x_dom: Domain, y_dom: Domain) -> PanelPaintCache {
    let scale = PlotScale::local(w, h, x_dom, y_dom);

    let mut grid = PathBuilder::stroke(px(1.0));
    for i in 0..=2 {
        let gy = (i as f32 / 2.0) * h;
        grid.move_to(point(px(0.0), px(gy)));
        grid.line_to(point(px(w), px(gy)));
    }
    let grid = grid.build().expect("grid path");

    let mut line = PathBuilder::stroke(px(LINE_WIDTH));
    let mut pen_down = false;
    let mut drew = false;
    for i in paint_indices(xy, x_dom, w) {
        match xy.y[i] {
            Some(yv) => {
                let p = point(px(scale.x_px(xy.x[i])), px(scale.y_px(yv)));
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
    x_dom: Domain,
    y_dom: Domain,
    paint_cache: &ChartPaintCache,
    line_color: Rgba,
    grid_color: Rgba,
    crosshair: Option<Crosshair>,
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

    let origin = bounds.origin;
    window.paint_path(translate_path(&cached.grid, origin), with_alpha(grid_color, 0.5));
    if let Some(line) = cached.line.as_ref() {
        window.paint_path(translate_path(line, origin), line_color);
    }

    let Some(ch) = crosshair.filter(|ch| x_dom.contains(ch.x)) else {
        return;
    };
    let scale = PlotScale::local(w, h, x_dom, y_dom).with_origin(left, top);
    paint_crosshair(window, &scale, h, &ch, line_color, cursor_color);
}

fn paint_crosshair(
    window: &mut Window,
    scale: &PlotScale,
    plot_h: f32,
    ch: &Crosshair,
    marker_color: Rgba,
    spike_color: Rgba,
) {
    let x = scale.x_px(ch.x);
    paint_vline(window, x, scale.origin_y, plot_h, spike_color);
    if let Some(yv) = ch.y {
        paint_marker(window, x, scale.y_px(yv), marker_color);
    }
}

fn paint_marker(window: &mut Window, cx: f32, cy: f32, color: Rgba) {
    let r = CURSOR_POINT_R;
    let rpx = px(r);
    let mut b = PathBuilder::fill();
    b.move_to(point(px(cx + r), px(cy)));
    b.arc_to(
        point(rpx, rpx),
        px(0.),
        false,
        true,
        point(px(cx - r), px(cy)),
    );
    b.arc_to(
        point(rpx, rpx),
        px(0.),
        false,
        true,
        point(px(cx + r), px(cy)),
    );
    b.close();
    if let Ok(p) = b.build() {
        window.paint_path(p, color);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::charts::Xy;

    #[test]
    fn minmax_decimation_keeps_spike() {
        let xy = Xy {
            x: (0..100).map(|i| i as f64).collect(),
            y: (0..100)
                .map(|i| {
                    if i == 50 {
                        Some(100.0)
                    } else {
                        Some(1.0)
                    }
                })
                .collect(),
        };
        let idx = paint_indices(&xy, Domain::new(0.0, 99.0), 20.0);
        assert!(idx.contains(&50), "spike index preserved: {idx:?}");
    }

    #[test]
    fn decimation_noop_for_short_series() {
        let xy = Xy {
            x: vec![0.0, 1.0, 2.0],
            y: vec![Some(1.0), Some(2.0), Some(1.5)],
        };
        assert_eq!(
            paint_indices(&xy, Domain::new(0.0, 2.0), 400.0),
            vec![0, 1, 2]
        );
    }
}
