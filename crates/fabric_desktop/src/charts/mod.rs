//! Synchronized line charts for the War Room metric wall.
//!
//! Port of `web_app/src/components/Chart.tsx` + `MetricWall.tsx` onto GPUI's native
//! `canvas` painting API (no uPlot, no extra deps). Every panel shares:
//!   * a **crosshair** at [`Dashboard::cursor_x`](crate::dashboard::Dashboard) — hovering any panel
//!     moves the vertical cursor line on all of them (uPlot `syncKey` equivalent, x-axis only),
//!   * a **readout** in the panel header (right) — value at the crosshair epoch while hovering,
//!     otherwise the latest probe value,
//!   * a **hover marker** and live x-axis tick at the snapped epoch.
//!
//! Coordinate mapping follows the same split as [plotly.rs](https://github.com/plotly/plotly.rs)
//! (`xref`/`yref` data space vs layout): series paths are built in local plot space, crosshair
//! overlays paint in window space, and axis labels use normalized fractions.

pub mod metric_chart;

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use fabric_types::RunSeries;
use gpui::{Path, Pixels};

/// Data-space axis bounds (plotly `AxisRange` / layout axis `range`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Domain {
    pub min: f64,
    pub max: f64,
}

impl Domain {
    pub const fn new(min: f64, max: f64) -> Self {
        Self { min, max }
    }

    pub fn span(&self) -> f64 {
        (self.max - self.min).max(1e-9)
    }

    pub fn contains(&self, v: f64) -> bool {
        v >= self.min && v <= self.max
    }

    /// Normalized position in `[0, 1]` for layout (`xref: "paper"`).
    pub fn fraction(&self, v: f64) -> f32 {
        ((v - self.min) / self.span()).clamp(0.0, 1.0) as f32
    }
}

/// Maps data coordinates into plot pixels (local or window-absolute via origin).
#[derive(Clone, Copy, Debug)]
pub struct PlotScale {
    pub x: Domain,
    pub y: Domain,
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
}

impl PlotScale {
    pub fn local(width: f32, height: f32, x: Domain, y: Domain) -> Self {
        Self {
            x,
            y,
            width,
            height,
            origin_x: 0.0,
            origin_y: 0.0,
        }
    }

    pub fn with_origin(self, origin_x: f32, origin_y: f32) -> Self {
        Self {
            origin_x,
            origin_y,
            ..self
        }
    }

    pub fn x_px(&self, xv: f64) -> f32 {
        self.origin_x + self.x.fraction(xv) * self.width
    }

    pub fn y_px(&self, yv: f64) -> f32 {
        self.origin_y + (1.0 - self.y.fraction(yv)) * self.height
    }
}

/// Hover crosshair state (plotly axis `showspikes` + `spikesnap: "data"` + marker).
#[derive(Clone, Copy, Debug)]
pub struct Crosshair {
    pub x: f64,
    pub y: Option<f64>,
}

impl Crosshair {
    pub fn from_cursor(cursor_x: f64, y_at_x: Option<f64>) -> Self {
        Self {
            x: cursor_x,
            y: y_at_x.filter(|v| v.is_finite()),
        }
    }
}

/// Plot geometry captured at paint time (window-absolute pixels), keyed by panel/track id.
/// Used to map a mouse x position back to an epoch value for crosshair sync.
#[derive(Clone, Copy, Debug)]
pub struct PlotGeom {
    pub plot_left: f32,
    pub plot_width: f32,
    pub x: Domain,
}

impl PlotGeom {
    pub fn from_bounds(plot_left: f32, plot_width: f32, x_dom: Domain) -> Self {
        Self {
            plot_left,
            plot_width,
            x: x_dom,
        }
    }

    /// Map a window-absolute x pixel to an x-domain (epoch) value, clamped to the domain.
    pub fn px_to_x(&self, px_x: f32) -> Option<f64> {
        if self.plot_width <= 0.0 {
            return None;
        }
        let t = ((px_x - self.plot_left) / self.plot_width).clamp(0.0, 1.0) as f64;
        Some(self.x.min + t * self.x.span())
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
pub fn x_domain(series: &RunSeries) -> Option<Domain> {
    let first = *series.epochs.first()? as f64;
    let last = *series.epochs.last()? as f64;
    if (last - first).abs() < f64::EPSILON {
        Some(Domain::new(first - 0.5, last + 0.5))
    } else if last < first {
        Some(Domain::new(last, first))
    } else {
        Some(Domain::new(first, last))
    }
}

/// y-domain from the finite samples, padded ~6% so curves don't hug the frame. Flat series get a
/// small symmetric band so the line stays centered instead of pinned to an edge.
pub fn y_domain(xy: &Xy) -> Domain {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for v in xy.y.iter().flatten() {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    if !lo.is_finite() || !hi.is_finite() {
        return Domain::new(0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-12 {
        let pad = lo.abs().max(1.0) * 0.05;
        return Domain::new(lo - pad, hi + pad);
    }
    let pad = (hi - lo) * 0.06;
    Domain::new(lo - pad, hi + pad)
}

/// Compact axis-tick / readout formatting (mirrors `Chart.tsx` `fmtTick`).
pub fn fmt_tick(v: f64, pct: bool) -> String {
    if pct {
        return format!("{:.0}%", v * 100.0);
    }
    let a = v.abs();
    if a != 0.0 && !(1e-3..1e5).contains(&a) {
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
    let body = if a != 0.0 && !(1e-3..1e5).contains(&a) {
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
        assert_eq!(x_domain(&s), Some(Domain::new(1000.0, 5000.0)));
    }

    #[test]
    fn domain_fraction_maps_linearly() {
        let d = Domain::new(0.0, 100.0);
        assert!((d.fraction(50.0) - 0.5).abs() < f32::EPSILON);
        assert_eq!(d.fraction(-10.0), 0.0);
        assert_eq!(d.fraction(200.0), 1.0);
    }

    #[test]
    fn plot_scale_local_coords() {
        let scale = PlotScale::local(200.0, 100.0, Domain::new(0.0, 100.0), Domain::new(0.0, 10.0));
        assert!((scale.x_px(50.0) - 100.0).abs() < f32::EPSILON);
        assert!((scale.y_px(10.0) - 0.0).abs() < f32::EPSILON);
        assert!((scale.y_px(0.0) - 100.0).abs() < f32::EPSILON);
    }

    #[test]
    fn fmt_epoch_tick_is_one_based() {
        assert_eq!(fmt_epoch_tick(0.0), "1");
        assert_eq!(fmt_epoch_tick(49.0), "50");
    }

    #[test]
    fn px_to_x_maps_and_clamps() {
        let g = PlotGeom::from_bounds(100.0, 200.0, Domain::new(0.0, 1000.0));
        assert_eq!(g.px_to_x(200.0), Some(500.0));
        assert_eq!(g.px_to_x(50.0), Some(0.0)); // clamps below
        assert_eq!(g.px_to_x(400.0), Some(1000.0)); // clamps above
    }
}
