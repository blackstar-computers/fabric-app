//! Pure-Rust visualizer helpers — ports of `fabric-visualizer/web_app/src/lib/viz.ts` and related
//! topology parsing.

mod colormap;
mod drive;
mod eligible;
mod fab;
mod geometry;
mod quant8;

pub use colormap::diverging_rgba;
pub use drive::{build_drive, default_viz_source, viz_sources, BuildDriveOpts};
pub use geometry::{
    display_regions, gallery_dataset, present_region_kinds, region_at, region_kinds_in_cells,
    regions_from_fab_spans, substrate_band_rows, substrate_cols, substrate_planes, substrate_rows,
};
pub use eligible::{is_visualizer_checkpoint_file, viz_eligible_checkpoint};
pub use fab::parse_fab1;
pub use quant8::decode_quant8_frame;
