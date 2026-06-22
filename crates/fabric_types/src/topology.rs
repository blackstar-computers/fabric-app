//! Topology manifest + FAB header types — mirrors `fabric-visualizer/web_app/src/types.ts` and
//! `web_app/src/routes/Visualizer/topology/fab.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TopoEntry {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_file: Option<String>,
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viewable: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "R")]
    pub r: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "C")]
    pub c: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_planes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "E")]
    pub e: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub e_view: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TopoWeight {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive_file: Option<String>,
    pub run: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psnr: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TopoManifestResp {
    #[serde(default)]
    pub topologies: Vec<TopoEntry>,
    #[serde(default)]
    pub weights: Vec<TopoWeight>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FabRegionSpan {
    pub input: [i64; 2],
    pub readout: [i64; 2],
    pub recon: [i64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FabArrayDesc {
    pub name: String,
    pub dtype: String,
    pub len: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FabHeader {
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "R")]
    pub r: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "C")]
    pub c: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band_rows: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_planes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_rows: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_row0: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regions: Option<FabRegionSpan>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "N")]
    pub n: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "E")]
    pub e: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absmax: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_psnr: Option<f64>,
    #[serde(default)]
    pub arrays: Vec<FabArrayDesc>,
}

/// Parsed `.topo` FAB substrate in flat cell order (`i = r * cols + c`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TopoDataInMemory {
    pub n: usize,
    pub cols: u32,
    pub band_rows: u32,
    pub planes: u32,
    pub regions: Vec<crate::viz::RegionKind>,
}

impl TopoDataInMemory {
    pub fn cell_count(&self) -> usize {
        (self.planes * self.band_rows * self.cols) as usize
    }

    pub fn region_of(&self, cell: usize) -> crate::viz::RegionKind {
        self.regions
            .get(cell)
            .copied()
            .unwrap_or(crate::viz::RegionKind::Compute)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_topo_manifest_fixture() {
        let json = include_str!("../../../fixtures/topo_manifest.json");
        let manifest: TopoManifestResp = serde_json::from_str(json).expect("fixture parses");
        assert!(!manifest.topologies.is_empty());
    }
}
