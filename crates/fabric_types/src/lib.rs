//! Portal API types — kept in sync with `web_app/src/types.ts`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_PORTAL_URL: &str = "https://agents.fabric.blackstar.inc";
pub const SERVICE_TOKEN_HEADER: &str = "X-Fleet-Token";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Headline {
    pub key: String,
    pub goal: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RunScalars {
    pub pod: String,
    pub name: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub fleet: String,
    pub metric: Option<String>,
    pub best: Option<f64>,
    pub last_epoch: Option<i64>,
    pub last_top1: Option<f64>,
    pub last_tgt: Option<f64>,
    pub last_lr: Option<f64>,
    pub n: Option<i64>,
    pub total_epochs: Option<i64>,
    pub eta_sec: Option<f64>,
    pub sec_per_epoch: Option<f64>,
    pub created: Option<f64>,
    pub status: Option<String>,
    pub label: Option<String>,
    pub grid: Option<String>,
    pub dataset: Option<String>,
    pub sweep: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GpuDetail {
    pub name: String,
    pub active: Option<i64>,
    pub total: Option<i64>,
    pub online: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GpuSummary {
    pub active: Option<i64>,
    pub total: Option<i64>,
    #[serde(default)]
    pub detail: Vec<GpuDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct RunsSummary {
    pub updated: Option<String>,
    pub title: Option<String>,
    pub unit: Option<String>,
    pub scale: Option<f64>,
    pub metric: Option<String>,
    #[serde(default)]
    pub runs: Vec<RunScalars>,
    #[serde(default)]
    pub gpus: GpuSummary,
    pub stale: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SseHeadline {
    pub key: String,
    pub value: f64,
    #[serde(default)]
    pub best: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct SseRunEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub pod: String,
    pub run: String,
    pub v: Option<i64>,
    pub ts: Option<f64>,
    pub headline: Option<SseHeadline>,
    #[serde(default)]
    pub scalars: HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub point: HashMap<String, serde_json::Value>,
}

impl SseRunEvent {
    pub fn is_run_v2(&self) -> bool {
        self.kind == "run" && self.v == Some(2)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Best,
    Epoch,
    Status,
    Created,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortState {
    pub column: SortColumn,
    pub direction: SortDirection,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            column: SortColumn::Created,
            direction: SortDirection::Desc,
        }
    }
}

pub fn sort_runs(runs: &mut [RunScalars], sort: SortState) {
    runs.sort_by(|a, b| {
        let ord = match sort.column {
            SortColumn::Name => a.name.cmp(&b.name),
            SortColumn::Best => cmp_opt_f64(a.best, b.best),
            SortColumn::Epoch => cmp_opt_i64(a.last_epoch, b.last_epoch),
            SortColumn::Status => a
                .status
                .as_deref()
                .unwrap_or("")
                .cmp(b.status.as_deref().unwrap_or("")),
            SortColumn::Created => cmp_opt_f64(a.created, b.created),
        };
        match sort.direction {
            SortDirection::Asc => ord,
            SortDirection::Desc => ord.reverse(),
        }
    });
}

fn cmp_opt_f64(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
    }
}

fn cmp_opt_i64(a: Option<i64>, b: Option<i64>) -> std::cmp::Ordering {
    match (a, b) {
        (None, None) => std::cmp::Ordering::Equal,
        (None, Some(_)) => std::cmp::Ordering::Less,
        (Some(_), None) => std::cmp::Ordering::Greater,
        (Some(a), Some(b)) => a.cmp(&b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_summary_fixture() {
        let json = include_str!("../../../fixtures/runs_summary.json");
        let summary: RunsSummary = serde_json::from_str(json).expect("fixture parses");
        assert!(!summary.runs.is_empty());
    }
}
