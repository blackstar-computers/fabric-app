//! Diverging heatmap colors matching `SubstrateGrid.tsx` / viewer `_state_thumb_png`.

/// Warm (+) / cool (-) diverging RGBA for a signed activation normalized by `vmax`.
pub fn diverging_rgba(v: f32, vmax: f32) -> [u8; 4] {
    let m = if vmax > 0.0 {
        (v.abs() / vmax).min(1.0)
    } else {
        0.0
    };
    let pos = if v > 0.0 { 1.0 } else { 0.0 };
    [
        (22.0 + (255.0 - 22.0) * m * pos + (85.0 - 22.0) * m * (1.0 - pos)) as u8,
        (24.0 + (150.0 - 24.0) * m) as u8,
        (27.0 + (70.0 - 27.0) * m * pos + (255.0 - 27.0) * m * (1.0 - pos)) as u8,
        255,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_activation_is_dark_baseline() {
        let c = diverging_rgba(0.0, 1.0);
        assert_eq!(c, [22, 24, 27, 255]);
    }

    #[test]
    fn positive_saturates_warm_red() {
        let c = diverging_rgba(1.0, 1.0);
        assert_eq!(c, [255, 150, 70, 255]);
    }

    #[test]
    fn negative_saturates_cool_blue() {
        let c = diverging_rgba(-1.0, 1.0);
        assert_eq!(c, [85, 150, 255, 255]);
    }

    #[test]
    fn scales_with_vmax() {
        let half = diverging_rgba(0.5, 1.0);
        let full = diverging_rgba(1.0, 2.0);
        assert_eq!(half, full);
    }

    #[test]
    fn zero_vmax_yields_baseline() {
        let c = diverging_rgba(5.0, 0.0);
        assert_eq!(c, [22, 24, 27, 255]);
    }
}
