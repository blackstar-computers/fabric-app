//! Mini trend line for overview table rows (port of `Leaderboard.Sparkline`).
//!
//! Paths are built in local (0,0) space and translated at paint time — same pattern as War Room
//! charts — so `uniform_list` scroll does not rebuild geometry every frame.

use gpui::{canvas, div, point, prelude::*, px, Path, PathBuilder, Pixels, Rgba};
use std::sync::Arc;

use crate::theme::Theme;

pub const SPARK_W: f32 = 56.0;
pub const SPARK_H: f32 = 20.0;

fn downsample(values: &[f64], budget: usize) -> Vec<f64> {
    let budget = budget.clamp(2, values.len());
    if values.len() <= budget {
        return values.to_vec();
    }
    let step = (values.len() - 1) as f64 / (budget - 1) as f64;
    let mut out = Vec::with_capacity(budget);
    for i in 0..budget {
        out.push(values[((i as f64) * step).round() as usize]);
    }
    out
}

/// Build a normalized polyline in local (0,0) space for the given samples.
pub fn sparkline_path(values: &[f64]) -> Option<Path<Pixels>> {
    if values.len() < 2 {
        return None;
    }
    let values = downsample(values, SPARK_W as usize);
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let span = (max - min).max(1e-12);
    let last = values.len() - 1;
    let mut b = PathBuilder::stroke(px(1.));
    let mut started = false;
    for (i, &v) in values.iter().enumerate() {
        let x = (i as f32 / last as f32) * SPARK_W;
        let y = SPARK_H - ((v - min) / span) as f32 * (SPARK_H - 2.0) - 1.0;
        let p = point(px(x), px(y));
        if started {
            b.line_to(p);
        } else {
            b.move_to(p);
            started = true;
        }
    }
    b.build().ok()
}

pub fn sparkline_cell(
    theme: &Theme,
    path: Option<Arc<Path<Pixels>>>,
    color: Rgba,
    dimmed: bool,
) -> impl IntoElement {
    let Some(path) = path else {
        return div()
            .flex_shrink_0()
            .w(px(SPARK_W))
            .h(px(SPARK_H))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(9.))
            .text_color(theme.text_dim)
            .child("—")
            .into_any_element();
    };

    let stroke = if dimmed {
        theme.text_dim
    } else {
        color
    };

    canvas(
        move |bounds, _, _| bounds,
        move |bounds, _, window, _| {
            window.paint_path(translate_path(path.as_ref(), bounds.origin), stroke);
        },
    )
    .flex_shrink_0()
    .w(px(SPARK_W))
    .h(px(SPARK_H))
    .into_any_element()
}

fn translate_path(path: &Path<Pixels>, origin: gpui::Point<Pixels>) -> Path<Pixels> {
    let mut shifted = path.clone();
    for v in shifted.vertices.iter_mut() {
        v.xy_position += origin;
    }
    shifted.bounds.origin += origin;
    shifted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_needs_two_points() {
        assert!(sparkline_path(&[1.0]).is_none());
        assert!(sparkline_path(&[1.0, 2.0, 1.5]).is_some());
    }

    #[test]
    fn downsample_caps_vertices() {
        let big: Vec<f64> = (0..500).map(|i| i as f64).collect();
        let small = downsample(&big, 56);
        assert_eq!(small.len(), 56);
    }
}
