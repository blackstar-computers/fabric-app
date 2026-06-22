//! Visualizer wire types — mirrors `fabric-visualizer/web_app/src/lib/viz.ts` and checkpoint/viz
//! shapes from `web_app/src/types.ts`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RegionKind {
    Input,
    Compute,
    Readout,
    Recon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizRegion {
    pub r0: i64,
    pub r1: i64,
    pub c0: i64,
    pub c1: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizGeometryRegions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<VizRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compute: Option<VizRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readout: Option<VizRegion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recon: Option<VizRegion>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizGeometry {
    pub kind: String,
    #[serde(rename = "R")]
    pub r: i64,
    #[serde(rename = "C")]
    pub c: i64,
    pub planes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band_rows: Option<i64>,
    #[serde(default)]
    pub regions: VizGeometryRegions,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_size: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VizStateMeta {
    #[serde(rename = "R")]
    pub r: i64,
    #[serde(rename = "C")]
    pub c: i64,
    pub scale: f64,
    pub zero: i64,
    pub signed: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VizKind {
    ReconRollout,
    ClassifyLogits,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DriveType {
    Image,
    Builtin,
    Pixels,
    Zero,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type")]
pub enum DriveSpec {
    #[serde(rename = "image")]
    Image { idx: i64 },
    #[serde(rename = "builtin")]
    Builtin { dataset: String, idx: i64 },
    #[serde(rename = "pixels")]
    Pixels { data: String },
    #[serde(rename = "zero")]
    Zero,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizLoadMeta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loaded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viz_kind: Option<VizKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub has_dataset: Option<bool>,
    #[serde(default)]
    pub builtins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub def_ticks: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub val_start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resid: Option<bool>,
    #[serde(default)]
    pub drive_types: Vec<DriveType>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_classes: Option<i64>,
    #[serde(default)]
    pub class_names: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "R")]
    pub r: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[serde(rename = "C")]
    pub c: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psnr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub geometry: Option<VizGeometry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ReconStepResp {
    pub kind: String,
    pub tick: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_idx: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_fmt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_meta: Option<VizStateMeta>,
    #[serde(default)]
    pub recon: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recon_fmt: Option<String>,
    #[serde(default)]
    pub psnr: Vec<Option<f64>>,
    #[serde(default)]
    pub state: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ClassifyStepResp {
    pub kind: String,
    pub tick: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drive_idx: Option<Option<i64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub src: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_fmt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_meta: Option<VizStateMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub n_classes: Option<i64>,
    #[serde(default)]
    pub class_names: Vec<String>,
    #[serde(default)]
    pub logits: Vec<Vec<f64>>,
    #[serde(default)]
    pub probs: Vec<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pred: Option<i64>,
    #[serde(default)]
    pub state: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind")]
pub enum VizStepResp {
    #[serde(rename = "recon")]
    Recon(ReconStepResp),
    #[serde(rename = "classify")]
    Classify(ClassifyStepResp),
}

/// One dataset thumbnail returned by `/viz/default/api/gallery` — mirrors the gallery item
/// shape consumed by `InputPicker.tsx` (`{ idx, png }`). The base64 PNG field name varies
/// across portal versions, so accept the common aliases.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizGalleryItem {
    #[serde(default)]
    pub idx: i64,
    #[serde(
        default,
        alias = "img",
        alias = "thumb",
        alias = "b64",
        alias = "data",
        skip_serializing_if = "Option::is_none"
    )]
    pub png: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// Response from `/viz/default/api/gallery?dataset=&start=&count=&size=` — a paginated batch of
/// dataset thumbnails. All scalar fields are optional so older portals deserialize cleanly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizGalleryResp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dataset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub val_start: Option<i64>,
    #[serde(default)]
    pub items: Vec<VizGalleryItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CheckpointFile {
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psnr: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CheckpointRun {
    pub fleet: String,
    pub pod: String,
    pub run: String,
    pub n_files: i64,
    pub bytes: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_epoch: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated: Option<String>,
    #[serde(default)]
    pub files: Vec<CheckpointFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CheckpointsResp {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default)]
    pub runs: Vec<CheckpointRun>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizStatusResp {
    pub configured: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#box: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tunnel: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizOpenResp {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ckpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#box: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VizOpenRequest {
    pub fleet: String,
    pub pod: String,
    pub run: String,
    pub file: String,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VizStepRequest {
    pub ticks: i64,
    pub drive: DriveSpec,
    #[serde(default = "default_want_state")]
    pub want_state: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_fmt: Option<String>,
}

fn default_want_state() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_drive_spec_variants() {
        let image: DriveSpec = serde_json::from_str(r#"{"type":"image","idx":3}"#).unwrap();
        assert!(matches!(image, DriveSpec::Image { idx: 3 }));

        let builtin: DriveSpec =
            serde_json::from_str(r#"{"type":"builtin","dataset":"mnist","idx":7}"#).unwrap();
        assert!(matches!(builtin, DriveSpec::Builtin { .. }));

        let zero: DriveSpec = serde_json::from_str(r#"{"type":"zero"}"#).unwrap();
        assert!(matches!(zero, DriveSpec::Zero));
    }

    #[test]
    fn deserializes_viz_step_request() {
        let req: VizStepRequest = serde_json::from_str(
            r#"{"ticks":4,"drive":{"type":"builtin","dataset":"mnist","idx":0},"state_fmt":"quant8"}"#,
        )
        .unwrap();
        assert_eq!(req.ticks, 4);
        assert_eq!(req.state_fmt.as_deref(), Some("quant8"));
        assert!(req.want_state);
    }

    #[test]
    fn deserializes_recon_step_resp() {
        let json = include_str!("../../../fixtures/viz_recon_step.json");
        let resp: ReconStepResp = serde_json::from_str(json).expect("fixture parses");
        assert_eq!(resp.kind, "recon");
        assert_eq!(resp.state.len(), 1);
        assert!(resp.state_meta.is_some());
    }

    #[test]
    fn deserializes_checkpoints_fixture() {
        let json = include_str!("../../../fixtures/checkpoints.json");
        let resp: CheckpointsResp = serde_json::from_str(json).expect("fixture parses");
        assert!(resp.enabled);
        assert_eq!(resp.runs.len(), 1);
    }
}
