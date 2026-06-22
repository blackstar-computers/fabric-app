//! Main GPUI canvas — structure grid or activation heatmap.
//!
//! Performance model (mirrors [`charts::metric_chart`](crate::charts::metric_chart)): the whole
//! substrate is rasterized **once** into an `Arc<RenderImage>` keyed by
//! [`GridImageKey`](crate::topology::GridImageKey) and blitted with a single `paint_image`.
//! Hover/select repaints reuse that bitmap, so only a handful of overlay paths (region
//! borders + the selected/hover outline) are rebuilt per frame instead of thousands of
//! per-cell `paint_path` calls.

use crate::theme::Theme;
use crate::topology::{
    DisplayMode, GridHitInfo, GridImageCache, GridImageKey, GridPaintCache, TopologyView,
};
use fabric_types::{RegionKind, TopoDataInMemory, VizGeometryRegions};
use fabric_viz::{
    display_regions, diverging_rgba, present_region_kinds, region_at as geo_region_at,
    region_kinds_in_cells, substrate_band_rows, substrate_cols, substrate_planes, substrate_rows,
};
use gpui::{
    canvas, div, point, prelude::*, px, relative, rgb, rgba, Bounds, Context, Corners, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathBuilder, Pixels, RenderImage, Rgba, Window,
};
use image::{Frame, RgbaImage};
use std::rc::Rc;
use std::sync::Arc;

// Region palette — matches `SubstrateGrid.tsx` stroke colors.
fn region_input() -> Rgba {
    rgb(0x3b82f6)
}
fn region_readout() -> Rgba {
    rgb(0x34d399)
}
fn region_recon() -> Rgba {
    rgb(0xf59e0b)
}
fn region_compute() -> Rgba {
    rgb(0x5b6573)
}

const SELECT_COLOR: u32 = 0xffa028;
const BORDER_PX: f32 = 2.0;
/// Round the canvas pixel size to this granularity so resize drags don't rebuild the raster
/// every frame (the bitmap is stretched to the exact bounds by `paint_image`).
const SIZE_BUCKET: f32 = 8.0;

/// A region rectangle in cell coordinates (plane-row / column space, plane 0).
#[derive(Clone, Copy)]
struct RegionRect {
    kind: RegionKind,
    r0: f32,
    r1: f32,
    c0: f32,
    c1: f32,
}

pub fn explorer_canvas(
    view: &TopologyView,
    theme: &Theme,
    cx: &mut Context<TopologyView>,
) -> impl IntoElement {
    let (cols, band_rows, planes) = grid_dims(view);
    let total_rows = substrate_total_rows(view, cols, band_rows, planes);
    let display_mode = view.display_mode;
    let topo = view.topo_data.clone();
    let geometry = view.viz_meta.as_ref().and_then(|m| m.geometry.clone());
    let structure_regions = geometry.as_ref().map(|geo| {
        display_regions(
            geo,
            view.viz_meta
                .as_ref()
                .and_then(|m| m.viz_kind.as_ref()),
        )
    });
    let decoded_frames = view.decoded_frames.clone();
    let scrub_tick = view.scrub_tick;
    let selected_cell = view.selected_cell;
    let hover_cell = view.hover_cell;
    let regions = region_rects(view, cols, band_rows, planes);
    let labels = region_labels(&regions, cols, total_rows);
    let grid_hit = Rc::clone(&view.grid_hit);
    let grid_cache = Rc::clone(&view.grid_cache);
    let bg = theme.bg;
    let paint_theme = theme.clone();

    div()
        .id("topology-explorer")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .relative()
        .bg(theme.bg)
        .border_1()
        .border_color(theme.border)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                if let Some(cell) = this.hit_cell(ev.position) {
                    this.select_cell(cell, cx);
                }
            }),
        )
        .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
            let hit = {
                let info = *this.grid_hit.borrow();
                info.and_then(|info| hit_test(ev.position, &info))
            };
            let hover = hit.map(|idx| {
                let c = cols.max(1);
                let row = idx as u32 / c;
                let col = idx as u32 % c;
                let val = this
                    .decoded_frames
                    .get(this.scrub_tick)
                    .and_then(|f| f.get(idx))
                    .copied()
                    .unwrap_or(0.0);
                (row, col, val)
            });
            this.set_hover_cell(hover, cx);
        }))
        .child(
            canvas(
                move |bounds, _, _| bounds,
                move |bounds, _, window, _| {
                    *grid_hit.borrow_mut() = Some(GridHitInfo {
                        origin_x: px_f(bounds.origin.x),
                        origin_y: px_f(bounds.origin.y),
                        width: px_f(bounds.size.width),
                        height: px_f(bounds.size.height),
                        cols,
                        total_rows,
                    });
                    paint_grid(
                        window,
                        bounds,
                        cols,
                        total_rows,
                        band_rows,
                        display_mode,
                        &topo,
                        structure_regions.as_ref(),
                        &decoded_frames,
                        scrub_tick,
                        selected_cell,
                        hover_cell,
                        &regions,
                        &grid_cache,
                        bg,
                    );
                },
            )
            .size_full(),
        )
        .children(labels)
        .when_some(view.hover_cell, |el, (row, col, val)| {
            el.child(
                div()
                    .absolute()
                    .top(px(8.))
                    .right(px(8.))
                    .px(px(6.))
                    .py(px(2.))
                    .bg(rgba(0x0a0c10e0))
                    .border_1()
                    .border_color(paint_theme.border)
                    .text_size(px(10.))
                    .text_color(paint_theme.live)
                    .child(format!("({row},{col}) = {val:.3}")),
            )
        })
}

impl TopologyView {
    pub(crate) fn hit_cell(&self, pos: gpui::Point<Pixels>) -> Option<usize> {
        let info = (*self.grid_hit.borrow())?;
        hit_test(pos, &info)
    }
}

fn px_f(p: Pixels) -> f32 {
    f32::from(p)
}

fn grid_dims(view: &TopologyView) -> (u32, u32, u32) {
    if let Some(topo) = &view.topo_data {
        return (topo.cols.max(1), topo.band_rows.max(1), topo.planes.max(1));
    }
    if let Some(meta) = &view.viz_meta {
        if let Some(geo) = &meta.geometry {
            return (
                substrate_cols(geo),
                substrate_band_rows(geo),
                substrate_planes(geo),
            );
        }
        if let (Some(r), Some(c)) = (meta.r, meta.c) {
            return (c.max(1) as u32, r.max(1) as u32, 1);
        }
    }
    (16, 4, 1)
}

fn substrate_total_rows(view: &TopologyView, cols: u32, band_rows: u32, planes: u32) -> u32 {
    let _ = cols;
    if let Some(geo) = view.viz_meta.as_ref().and_then(|m| m.geometry.as_ref()) {
        return substrate_rows(geo);
    }
    (planes * band_rows).max(1)
}

fn hit_test(pos: gpui::Point<Pixels>, info: &GridHitInfo) -> Option<usize> {
    let cols = info.cols.max(1);
    let total_rows = info.total_rows.max(1);

    let x = px_f(pos.x) - info.origin_x;
    let y = px_f(pos.y) - info.origin_y;
    if x < 0. || y < 0. || x >= info.width || y >= info.height {
        return None;
    }

    let cell_w = info.width / cols as f32;
    let cell_h = info.height / total_rows as f32;
    let col = (x / cell_w).floor() as u32;
    let row = (y / cell_h).floor() as u32;
    if col >= cols || row >= total_rows {
        return None;
    }
    Some((row * cols + col) as usize)
}

#[allow(clippy::too_many_arguments)]
fn paint_grid(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    cols: u32,
    total_rows: u32,
    band_rows: u32,
    display_mode: DisplayMode,
    topo: &Option<TopoDataInMemory>,
    structure_regions: Option<&VizGeometryRegions>,
    decoded_frames: &[Vec<f32>],
    scrub_tick: usize,
    selected_cell: Option<usize>,
    hover_cell: Option<(u32, u32, f32)>,
    regions: &[RegionRect],
    grid_cache: &GridPaintCache,
    bg: Rgba,
) {
    let cols = cols.max(1);
    let total_rows = total_rows.max(1);

    let w = px_f(bounds.size.width).max(1.);
    let h = px_f(bounds.size.height).max(1.);
    if w <= 1.0 || h <= 1.0 {
        return;
    }

    let width_bucket = bucket(w);
    let height_bucket = bucket(h);
    let key = GridImageKey {
        scrub_tick,
        display_mode,
        cols,
        rows: total_rows,
        width_bucket,
        height_bucket,
    };

    // Rebuild the raster only when the key changes; otherwise reuse the cached blit.
    let image = {
        let mut cache = grid_cache.borrow_mut();
        let stale = cache.as_ref().is_none_or(|c| c.key != key);
        if stale {
            let frame = decoded_frames.get(scrub_tick);
            let vmax = frame
                .map(|f| f.iter().copied().fold(0.0f32, |m, v| m.max(v.abs())))
                .unwrap_or(1.0)
                .max(0.01);
            let image = build_grid_image(
                width_bucket,
                height_bucket,
                cols,
                total_rows,
                band_rows,
                display_mode,
                topo,
                structure_regions,
                frame,
                vmax,
                bg,
            );
            *cache = Some(GridImageCache { key, image });
        }
        cache.as_ref().expect("grid image cached").image.clone()
    };

    // ONE blit instead of N per-cell paths.
    let _ = window.paint_image(bounds, Corners::default(), image, 0, false);

    let ox = px_f(bounds.origin.x);
    let oy = px_f(bounds.origin.y);
    let cell_w = w / cols as f32;
    let cell_h = h / total_rows as f32;

    // Region borders — visible in both Structure and LiveFlow (a few rects, never per-cell).
    for r in regions {
        let x = ox + r.c0 * cell_w;
        let y = oy + r.r0 * cell_h;
        let rw = (r.c1 - r.c0) * cell_w;
        let rh = (r.r1 - r.r0) * cell_h;
        stroke_rect(window, x, y, rw, rh, BORDER_PX, region_color(r.kind));
    }

    // Selected + hover outlines on top of the bitmap (not baked in, so the raster is stable).
    if let Some(cell) = selected_cell {
        if let Some((row, col)) = cell_rowcol(cell, cols, total_rows) {
            let x = ox + col as f32 * cell_w;
            let y = oy + row as f32 * cell_h;
            stroke_rect(window, x, y, cell_w, cell_h, BORDER_PX, rgb(SELECT_COLOR));
        }
    }
    if let Some((row, col, _)) = hover_cell {
        if row < total_rows && col < cols {
            let x = ox + col as f32 * cell_w;
            let y = oy + row as f32 * cell_h;
            stroke_rect(window, x, y, cell_w, cell_h, 1.0, rgba(0xffffffcc));
        }
    }
}

fn bucket(v: f32) -> u32 {
    (((v / SIZE_BUCKET).ceil()) as u32 * SIZE_BUCKET as u32).max(SIZE_BUCKET as u32)
}

fn cell_rowcol(cell: usize, cols: u32, total_rows: u32) -> Option<(u32, u32)> {
    let cols = cols.max(1);
    let row = cell as u32 / cols;
    let col = cell as u32 % cols;
    if row < total_rows {
        Some((row, col))
    } else {
        None
    }
}

/// Rasterize the substrate into a BGRA buffer (`RenderImage` stores BGRA, hence the R/B swap).
#[allow(clippy::too_many_arguments)]
fn build_grid_image(
    img_w: u32,
    img_h: u32,
    cols: u32,
    total_rows: u32,
    band_rows: u32,
    display_mode: DisplayMode,
    topo: &Option<TopoDataInMemory>,
    structure_regions: Option<&VizGeometryRegions>,
    frame: Option<&Vec<f32>>,
    vmax: f32,
    bg: Rgba,
) -> Arc<RenderImage> {
    let w = img_w.max(1);
    let h = img_h.max(1);
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];

    // Background fill (shows through the 1px inter-cell gaps).
    let bg_rgba = rgba_u8(bg);
    for px in buf.chunks_exact_mut(4) {
        write_bgra(px, bg_rgba);
    }

    let cell_w = w as f32 / cols as f32;
    let cell_h = h as f32 / total_rows as f32;
    let gap = if cell_w > 4.0 && cell_h > 4.0 { 1.0 } else { 0.0 };

    for row in 0..total_rows {
        for col in 0..cols {
            let idx = (row * cols + col) as usize;
            let color = cell_color(
                idx,
                row,
                col,
                band_rows,
                display_mode,
                topo,
                structure_regions,
                frame,
                vmax,
            );

            let x0 = ((col as f32 * cell_w) + gap).floor().max(0.0) as u32;
            let x1 = (((col + 1) as f32 * cell_w) - gap).ceil().min(w as f32) as u32;
            let y0 = ((row as f32 * cell_h) + gap).floor().max(0.0) as u32;
            let y1 = (((row + 1) as f32 * cell_h) - gap).ceil().min(h as f32) as u32;
            if x1 <= x0 || y1 <= y0 {
                continue;
            }

            for y in y0..y1 {
                let base = ((y * w + x0) * 4) as usize;
                let end = ((y * w + x1) * 4) as usize;
                for px in buf[base..end].chunks_exact_mut(4) {
                    write_bgra(px, color);
                }
            }
        }
    }

    let buffer = RgbaImage::from_raw(w, h, buf).expect("buffer matches w*h*4");
    Arc::new(RenderImage::new(vec![Frame::new(buffer)]))
}

#[allow(clippy::too_many_arguments)]
fn cell_color(
    idx: usize,
    row: u32,
    col: u32,
    _band_rows: u32,
    display_mode: DisplayMode,
    topo: &Option<TopoDataInMemory>,
    structure_regions: Option<&VizGeometryRegions>,
    frame: Option<&Vec<f32>>,
    vmax: f32,
) -> [u8; 4] {
    if display_mode == DisplayMode::LiveFlow {
        if let Some(frame) = frame {
            let v = frame.get(idx).copied().unwrap_or(0.0);
            return diverging_rgba(v, vmax);
        }
        return rgba_u8(region_compute());
    }
    let kind = if let Some(topo) = topo {
        topo.region_of(idx)
    } else if let Some(regions) = structure_regions {
        geo_region_at(row, col, regions)
    } else {
        RegionKind::Compute
    };
    rgba_u8(region_color(kind))
}

fn rgba_u8(c: Rgba) -> [u8; 4] {
    [
        (c.r * 255.0).round().clamp(0.0, 255.0) as u8,
        (c.g * 255.0).round().clamp(0.0, 255.0) as u8,
        (c.b * 255.0).round().clamp(0.0, 255.0) as u8,
        (c.a * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

/// Write an `[r, g, b, a]` color into a 4-byte BGRA pixel slot.
fn write_bgra(px: &mut [u8], [r, g, b, a]: [u8; 4]) {
    px[0] = b;
    px[1] = g;
    px[2] = r;
    px[3] = a;
}

fn stroke_rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, width: f32, color: Rgba) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let mut b = PathBuilder::stroke(px(width));
    b.move_to(point(px(x), px(y)));
    b.line_to(point(px(x + w), px(y)));
    b.line_to(point(px(x + w), px(y + h)));
    b.line_to(point(px(x), px(y + h)));
    b.line_to(point(px(x), px(y)));
    if let Ok(path) = b.build() {
        window.paint_path(path, color);
    }
}

fn region_color(kind: RegionKind) -> Rgba {
    match kind {
        RegionKind::Input => region_input(),
        RegionKind::Readout => region_readout(),
        RegionKind::Recon => region_recon(),
        RegionKind::Compute => region_compute(),
    }
}

fn region_label(kind: RegionKind) -> Option<&'static str> {
    match kind {
        RegionKind::Input => Some("INPUT"),
        RegionKind::Readout => Some("READOUT"),
        RegionKind::Recon => Some("RECON"),
        RegionKind::Compute => None,
    }
}

/// Floating region-name chips positioned over the canvas at each region's top-left corner,
/// using fractional offsets so we don't need the canvas pixel bounds at render time.
fn region_labels(regions: &[RegionRect], cols: u32, total_rows: u32) -> Vec<gpui::AnyElement> {
    let cols = cols.max(1) as f32;
    let total_rows = total_rows.max(1) as f32;
    regions
        .iter()
        .filter_map(|r| {
            let label = region_label(r.kind)?;
            let left = (r.c0 / cols).clamp(0.0, 0.92);
            let top = (r.r0 / total_rows).clamp(0.0, 0.94);
            Some(
                div()
                    .absolute()
                    .left(relative(left))
                    .top(relative(top))
                    .px(px(3.))
                    .py(px(1.))
                    .bg(rgba(0x0a0c10e0))
                    .border_1()
                    .border_color(region_color(r.kind))
                    .text_size(px(8.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(region_color(r.kind))
                    .child(label)
                    .into_any_element(),
            )
        })
        .collect()
}

/// Bounding rectangles (plane-row / column space) for the labeled regions. Prefer the
/// `VizGeometry.regions` spans; otherwise derive boxes from the per-cell FAB regions.
fn region_rects(
    view: &TopologyView,
    cols: u32,
    band_rows: u32,
    planes: u32,
) -> Vec<RegionRect> {
    let cols = cols.max(1);
    let total_rows = substrate_total_rows(view, cols, band_rows, planes) as f32;

    if let Some(geo) = view.viz_meta.as_ref().and_then(|m| m.geometry.as_ref()) {
        let regions = display_regions(
            geo,
            view.viz_meta
                .as_ref()
                .and_then(|m| m.viz_kind.as_ref()),
        );
        let mut out = Vec::new();
        for kind in present_region_kinds(&regions) {
            let reg = match kind {
                RegionKind::Input => &regions.input,
                RegionKind::Compute => &regions.compute,
                RegionKind::Readout => &regions.readout,
                RegionKind::Recon => &regions.recon,
            };
            if let Some(r) = reg {
                out.push(RegionRect {
                    kind,
                    r0: (r.r0.max(0) as f32).min(total_rows),
                    r1: (r.r1.max(0) as f32).min(total_rows),
                    c0: (r.c0.max(0) as f32).min(cols as f32),
                    c1: (r.c1.max(0) as f32).min(cols as f32),
                });
            }
        }
        return out.into_iter().filter(|r| r.r1 > r.r0 && r.c1 > r.c0).collect();
    }

    if let Some(topo) = &view.topo_data {
        return region_kinds_in_cells(&topo.regions)
            .into_iter()
            .filter_map(|kind| bbox_for_kind(topo, cols, band_rows, planes, kind))
            .collect();
    }

    Vec::new()
}

/// Scan the FAB per-cell regions for the plane-row / column bounding box of `kind` (plane 0).
fn bbox_for_kind(
    topo: &TopoDataInMemory,
    cols: u32,
    band_rows: u32,
    _planes: u32,
    kind: RegionKind,
) -> Option<RegionRect> {
    let (mut r0, mut r1, mut c0, mut c1) = (u32::MAX, 0u32, u32::MAX, 0u32);
    let mut any = false;
    for (idx, &cell_kind) in topo.regions.iter().enumerate() {
        if cell_kind != kind {
            continue;
        }
        let row = (idx as u32 / cols) % band_rows;
        let col = idx as u32 % cols;
        r0 = r0.min(row);
        r1 = r1.max(row + 1);
        c0 = c0.min(col);
        c1 = c1.max(col + 1);
        any = true;
    }
    if !any {
        return None;
    }
    Some(RegionRect {
        kind,
        r0: r0 as f32,
        r1: r1 as f32,
        c0: c0 as f32,
        c1: c1 as f32,
    })
}
