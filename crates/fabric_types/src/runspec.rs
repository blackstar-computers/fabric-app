//! Frozen RunSpec envelope (docs/15) — mirrors `web_app/src/types.ts`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Headline {
    pub key: String,
    #[serde(default)]
    pub goal: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunMetricDef {
    pub key: String,
    pub label: Option<String>,
    #[serde(default)]
    pub lower_better: bool,
    #[serde(default)]
    pub priority: Option<i64>,
    #[serde(default)]
    pub pct: bool,
    #[serde(default)]
    pub log_y: bool,
    #[serde(default)]
    pub overlay: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunMediaDef {
    pub key: String,
    pub label: Option<String>,
    #[serde(default)]
    pub layout: Option<String>,
    #[serde(default)]
    pub hero: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunOutput {
    pub kind: Option<String>,
    pub checkpoint_kind: Option<String>,
    pub headline: Option<Headline>,
    #[serde(default)]
    pub metrics: Vec<RunMetricDef>,
    #[serde(default)]
    pub media: Vec<RunMediaDef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunContext {
    #[serde(default, flatten)]
    pub fields: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct UiCapabilities {
    #[serde(default)]
    pub media_shown: Vec<String>,
    #[serde(default)]
    pub media_hidden: Vec<String>,
    #[serde(default)]
    pub interactive_viz: bool,
    #[serde(default)]
    pub topology_link: bool,
    #[serde(default)]
    pub metrics_charted: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct RunSpecEnvelope {
    pub schema: Option<String>,
    pub schema_version: Option<String>,
    pub name: Option<String>,
    pub trainer: Option<String>,
    pub substrate_kind: Option<String>,
    #[serde(default)]
    pub data_deps: Vec<String>,
    pub output: Option<RunOutput>,
    pub context: Option<RunContext>,
    pub ui_capabilities: Option<UiCapabilities>,
}

impl RunSpecEnvelope {
    pub fn substrate_is_lm(&self) -> bool {
        self.substrate_kind.as_deref() == Some("lm")
    }
}
