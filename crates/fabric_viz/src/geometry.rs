//! Substrate layout helpers — ports of `viewer.py::_geometry`, `InputPicker.tsx` gallery
//! dataset selection, and `grid_viz.py` region overlays.

use fabric_types::{RegionKind, VizGeometry, VizGeometryRegions, VizKind, VizLoadMeta, VizRegion};

/// Dataset name passed to `/viz/default/api/gallery` for the active input source.
/// Mirrors `InputPicker.tsx` `galleryDataset` logic.
pub fn gallery_dataset(meta: &VizLoadMeta, source: &str) -> String {
    if meta.has_dataset.unwrap_or(false) {
        if let Some(ds) = meta.dataset.as_ref().filter(|s| !s.is_empty()) {
            if source == ds.as_str() {
                return ds.clone();
            }
        }
    }
    source.to_string()
}

/// Full row count for the flat `[R,C]` activation grid.
pub fn substrate_rows(geo: &VizGeometry) -> u32 {
    let band = geo.band_rows.unwrap_or(geo.r).max(1) as u32;
    let planes = geo.planes.max(1) as u32;
    if geo.r > 0 {
        geo.r.max(1) as u32
    } else {
        band * planes
    }
}

pub fn substrate_cols(geo: &VizGeometry) -> u32 {
    geo.c.max(1) as u32
}

pub fn substrate_planes(geo: &VizGeometry) -> u32 {
    let band = geo.band_rows.unwrap_or(geo.r).max(1) as u32;
    if geo.r > 0 && band > 0 {
        ((geo.r as u32).max(1) / band).max(1)
    } else {
        geo.planes.max(1) as u32
    }
}

pub fn substrate_band_rows(geo: &VizGeometry) -> u32 {
    geo.band_rows.unwrap_or(geo.r).max(1) as u32
}

/// Region rectangles for display — taken verbatim from the loaded run's `geometry.regions`
/// (populated by the viewer from the runspec). When both readout and recon are present,
/// keep the one that matches `viz_kind`.
pub fn display_regions(geo: &VizGeometry, viz_kind: Option<&VizKind>) -> VizGeometryRegions {
    let mut out = geo.regions.clone();
    if out.readout.is_some() && out.recon.is_some() {
        match viz_kind {
            Some(VizKind::ClassifyLogits) => out.recon = None,
            Some(VizKind::ReconRollout) => out.readout = None,
            None if geo.kind == "classify" => out.recon = None,
            None => out.readout = None,
        }
    }
    out
}

/// Iterate labeled region kinds that have a rectangle in `regions`.
pub fn present_region_kinds(regions: &VizGeometryRegions) -> impl Iterator<Item = RegionKind> + '_ {
    [
        (RegionKind::Input, &regions.input),
        (RegionKind::Compute, &regions.compute),
        (RegionKind::Readout, &regions.readout),
        (RegionKind::Recon, &regions.recon),
    ]
    .into_iter()
    .filter_map(|(kind, reg)| reg.as_ref().map(|_| kind))
}

pub fn region_at(row: u32, col: u32, regions: &VizGeometryRegions) -> RegionKind {
    let in_region = |r: &VizRegion| -> bool {
        (row as i64) >= r.r0
            && (row as i64) < r.r1
            && (col as i64) >= r.c0
            && (col as i64) < r.c1
    };
    if regions.input.as_ref().is_some_and(in_region) {
        RegionKind::Input
    } else if regions.readout.as_ref().is_some_and(in_region) {
        RegionKind::Readout
    } else if regions.recon.as_ref().is_some_and(in_region) {
        RegionKind::Recon
    } else {
        RegionKind::Compute
    }
}

/// Build per-cell region tags from FAB column spans (`[c0, width]` per region).
pub fn regions_from_fab_spans(
    n: usize,
    cols: u32,
    band_rows: u32,
    spans: &fabric_types::FabRegionSpan,
) -> Vec<RegionKind> {
    let cols = cols.max(1);
    let _ = band_rows;
    (0..n)
        .map(|idx| {
            let col = idx as u32 % cols;
            if col_in_span(col, spans.input) {
                RegionKind::Input
            } else if col_in_span(col, spans.readout) {
                RegionKind::Readout
            } else if col_in_span(col, spans.recon) {
                RegionKind::Recon
            } else {
                RegionKind::Compute
            }
        })
        .collect()
}

/// Region kinds that appear at least once in a FAB per-cell region vector.
pub fn region_kinds_in_cells(cells: &[RegionKind]) -> Vec<RegionKind> {
    [RegionKind::Input, RegionKind::Readout, RegionKind::Recon]
        .into_iter()
        .filter(|kind| cells.iter().any(|c| c == kind))
        .collect()
}

fn col_in_span(col: u32, span: [i64; 2]) -> bool {
    (col as i64) >= span[0] && (col as i64) < span[0] + span[1]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gallery_dataset_prefers_run_npy_when_selected() {
        let meta = VizLoadMeta {
            has_dataset: Some(true),
            dataset: Some("clip.npy".into()),
            ..Default::default()
        };
        assert_eq!(gallery_dataset(&meta, "clip.npy"), "clip.npy");
        assert_eq!(gallery_dataset(&meta, "mnist"), "mnist");
    }

    #[test]
    fn display_regions_passes_through_viewer_geometry() {
        let geo = VizGeometry {
            kind: "canvas".into(),
            r: 64,
            c: 64,
            planes: 1,
            band_rows: Some(64),
            img_size: Some(28),
            regions: VizGeometryRegions {
                recon: Some(VizRegion {
                    r0: 10,
                    r1: 38,
                    c0: 20,
                    c1: 48,
                }),
                ..Default::default()
            },
        };
        let disp = display_regions(&geo, Some(&VizKind::ReconRollout));
        assert!(disp.input.is_none());
        assert!(disp.recon.is_some());
        assert!(disp.readout.is_none());
    }

    #[test]
    fn display_regions_prefers_readout_for_classify() {
        let geo = VizGeometry {
            kind: "classify".into(),
            r: 32,
            c: 32,
            planes: 1,
            regions: VizGeometryRegions {
                input: Some(VizRegion {
                    r0: 0,
                    r1: 28,
                    c0: 0,
                    c1: 28,
                }),
                readout: Some(VizRegion {
                    r0: 0,
                    r1: 32,
                    c0: 24,
                    c1: 32,
                }),
                recon: Some(VizRegion {
                    r0: 0,
                    r1: 28,
                    c0: 10,
                    c1: 20,
                }),
                ..Default::default()
            },
            ..Default::default()
        };
        let disp = display_regions(&geo, Some(&VizKind::ClassifyLogits));
        assert!(disp.readout.is_some());
        assert!(disp.recon.is_none());
    }
}
