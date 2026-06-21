//! Synchronized line charts for the War Room metric wall.
//!
//! Port of `web_app/src/components/Chart.tsx` + `MetricWall.tsx` onto GPUI's native
//! `canvas` painting API (no uPlot, no extra deps). Every panel shares:
//!   * a **crosshair** at [`Dashboard::cursor_x`](crate::dashboard::Dashboard) — hovering any panel
//!     moves the vertical cursor line on all of them (uPlot `syncKey` equivalent, x-axis only),
//!   * a **readout** in the panel header (right) — value at the crosshair epoch while hovering,
//!     otherwise the latest probe value.
//!
//! Mouse handlers live on the wrapping `div` (GPUI gives listeners view state + `Context`), while the
//! `canvas` paints into the same bounds. To translate a mouse x back into an epoch the canvas stashes
//! its painted plot geometry into a shared [`ChartGeoms`] map that the listeners read.

pub mod metric_chart;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fabric_types::RunSeries;
use gpui::{Path, Pixels};

/// Plot geometry captured at paint time (window-absolute pixels), keyed by panel/track id.
/// Used to map a mouse x position back to an epoch value for crosshair sync.
#[derive(Clone, Copy, Debug)]
pub struct PlotGeom {
    pub plot_left: f32,
    pub plot_width: f32,
    pub x_min: f64,
    pub x_max: f64,
}

impl PlotGeom {
    /// Map a window-absolute x pixel to an x-domain (epoch) value, clamped to the domain.
    pub fn px_to_x(&self, px_x: f32) -> Option<f64> {
        if self.plot_width <= 0.0 || self.x_max <= self.x_min {
            return None;
        }
        let t = ((px_x - self.plot_left) / self.plot_width).clamp(0.0, 1.0) as f64;
        Some(self.x_min + t * (self.x_max - self.x_min))
    }
}

/// Shared geometry registry: the canvases write, the mouse listeners read.
pub type ChartGeoms = Rc<RefCell<HashMap<String, PlotGeom>>>;

pub fn new_geoms() -> ChartGeoms {
    Rc::new(RefCell::new(HashMap::new()))
}

/// Cached canvas paths for a panel at a given size (avoids rebuilding on scroll/hover repaints).
#[derive(Clone, Debug)]
pub struct PanelPaintCache {
    pub width: f32,
    pub height: f32,
    pub point_count: usize,
    pub last_epoch: i64,
    pub grid: Path<Pixels>,
    pub line: Option<Path<Pixels>>,
}

pub type ChartPaintCache = Rc<RefCell<HashMap<String, PanelPaintCache>>>;

pub fn new_paint_cache() -> ChartPaintCache {
    Rc::new(RefCell::new(HashMap::new()))
}

/// Format an epoch-axis tick for display (0-based index → 1-based label).
pub fn fmt_epoch_tick(v: f64) -> String {
    crate::format::display_epoch(v.round() as i64).to_string()
}

/// Clean aligned `[x, y]` pair for one metric column (port of `MetricWall.buildXY`, epoch x-axis).
/// `x` is the epoch; `y` is `None` for non-finite samples (drawn as a gap, never a fake zero).
#[derive(Clone, Debug)]
pub struct Xy {
    pub x: Vec<f64>,
    pub y: Vec<Option<f64>>,
}

pub fn build_xy(series: &RunSeries, key: &str) -> Option<Xy> {
    let ys = series.nums(key);
    if ys.is_empty() {
        return None;
    }
    let mut x = Vec::with_capacity(ys.len());
    let mut y = Vec::with_capacity(ys.len());
    let mut any = false;
    for (i, &yv) in ys.iter().enumerate() {
        let Some(&ep) = series.epochs.get(i) else {
            continue;
        };
        x.push(ep as f64);
        if yv.is_finite() {
            y.push(Some(yv));
            any = true;
        } else {
            y.push(None);
        }
    }
    if any {
        Some(Xy { x, y })
    } else {
        None
    }
}

/// Shared x-domain across the whole wall = the run's epoch axis. Keeping one domain for every panel
/// is what makes the crosshair line up vertically across panels of differing length.
pub fn x_domain(series: &RunSeries) -> Option<(f64, f64)> {
    let first = *series.epochs.first()? as f64;
    let last = *series.epochs.last()? as f64;
    if (last - first).abs() < f64::EPSILON {
        Some((first - 0.5, last + 0.5))
    } else if last < first {
        Some((last, first))
    } else {
        Some((first, last))
    }
}

/// y-domain from the finite samples, padded ~6% so curves don't hug the frame. Flat series get a
/// small symmetric band so the line stays centered instead of pinned to an edge.
pub fn y_domain(xy: &Xy) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for v in xy.y.iter().flatten() {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return (0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-12 {
        let pad = lo.abs().max(1.0) * 0.05;
        return (lo - pad, hi + pad);
    }
    let pad = (hi - lo) * 0.06;
    (lo - pad, hi + pad)
}

/// Compact axis-tick / readout formatting (mirrors `Chart.tsx` `fmtTick`).
pub fn fmt_tick(v: f64, pct: bool) -> String {
    if pct {
        return format!("{:.0}%", v * 100.0);
    }
    let a = v.abs();
    if a != 0.0 && (a < 1e-3 || a >= 1e5) {
        format!("{v:.0e}")
    } else if a < 1.0 {
        format!("{v:.2}")
    } else if a < 100.0 {
        format!("{v:.1}")
    } else {
        format!("{}", v.round() as i64)
    }
}

/// Panel readout formatting (mirrors `MetricWall.fmtReadout`).
pub fn fmt_readout(v: f64, pct: bool, unit: Option<&str>) -> String {
    if pct {
        return format!("{:.1}%", v * 100.0);
    }
    let a = v.abs();
    let body = if a != 0.0 && (a < 1e-3 || a >= 1e5) {
        format!("{v:.1e}")
    } else if a < 1.0 {
        format!("{v:.3}")
    } else if a < 100.0 {
        format!("{v:.2}")
    } else {
        format!("{}", v.round() as i64)
    };
    match unit {
        Some(u) => format!("{body}{u}"),
        None => body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap as Map;

    fn series(epochs: Vec<i64>, key: &str, vals: Vec<f64>) -> RunSeries {
        RunSeries {
            pod: "p".into(),
            name: "r".into(),
            epochs,
            metrics: Map::from([(key.to_string(), vals)]),
        }
    }

    #[test]
    fn build_xy_drops_nonfinite_y_as_gaps() {
        let s = series(vec![1, 2, 3], "loss", vec![0.5, f64::NAN, 0.1]);
        let xy = build_xy(&s, "loss").expect("some");
        assert_eq!(xy.x, vec![1.0, 2.0, 3.0]);
        assert_eq!(xy.y, vec![Some(0.5), None, Some(0.1)]);
    }

    #[test]
    fn build_xy_none_when_all_nonfinite() {
        let s = series(vec![1, 2], "loss", vec![f64::NAN, f64::INFINITY]);
        assert!(build_xy(&s, "loss").is_none());
    }

    #[test]
    fn x_domain_spans_epoch_axis() {
        let s = series(vec![1000, 5000], "loss", vec![1.0, 2.0]);
        assert_eq!(x_domain(&s), Some((1000.0, 5000.0)));
    }

    #[test]
    fn fmt_epoch_tick_is_one_based() {
        assert_eq!(fmt_epoch_tick(0.0), "1");
        assert_eq!(fmt_epoch_tick(49.0), "50");
    }

    #[test]
    fn px_to_x_maps_and_clamps() {
        let g = PlotGeom {
            plot_left: 100.0,
            plot_width: 200.0,
            x_min: 0.0,
            x_max: 1000.0,
        };
        assert_eq!(g.px_to_x(200.0), Some(500.0));
        assert_eq!(g.px_to_x(50.0), Some(0.0)); // clamps below
        assert_eq!(g.px_to_x(400.0), Some(1000.0)); // clamps above
    }
}
