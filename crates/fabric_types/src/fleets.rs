//! Fleet / GPU / topology types — mirrors `web_app/src/types.ts` Phase 3 endpoints.

use serde::{Deserialize, Deserializer, Serialize};

/// Deserialize an optional patch field where JSON `null` means "clear".
///
/// Only invoked when the key is present on the wire; absent keys use `#[serde(default)]`.
fn deserialize_patch_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Some(None)),
        other => T::deserialize(other)
            .map(Some)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Fleet {
    pub id: String,
    pub name: String,
    pub pods_path: String,
    pub n_pods: i64,
    pub active: bool,
    pub status: String,
    #[serde(default)]
    pub pool_profile: Option<String>,
    #[serde(default)]
    pub contracts: Vec<i64>,
    #[serde(default)]
    pub provisioning: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FleetsResp {
    pub fleets: Vec<Fleet>,
    pub active: String,
    pub default: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct JobPod {
    pub pod: String,
    pub state: String,
    #[serde(default)]
    pub updated: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Job {
    pub job_id: String,
    pub fleet: String,
    pub pods_path: String,
    pub run_name: String,
    pub state: String,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub pods: Vec<JobPod>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct JobsResp {
    #[serde(default)]
    pub jobs: Vec<Job>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Instance {
    pub id: serde_json::Value,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub gpu_name: Option<String>,
    #[serde(default)]
    pub num_gpus: Option<i64>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub ssh_host: Option<String>,
    #[serde(default)]
    pub ssh_port: Option<serde_json::Value>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub fleet_id: Option<String>,
    #[serde(default)]
    pub fleet_name: Option<String>,
    #[serde(default)]
    pub pod_name: Option<String>,
    #[serde(default)]
    pub provision_state: Option<String>,
    #[serde(default)]
    pub assignable: Option<bool>,
    #[serde(default)]
    pub error: Option<String>,
}

impl Instance {
    pub fn id_str(&self) -> String {
        match &self.id {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            other => other.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct InstancesResp {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub total: i64,
    #[serde(default)]
    pub unassigned: i64,
    #[serde(default)]
    pub instances: Vec<Instance>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TreeNode {
    pub tag: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Vec<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub up: bool,
    #[serde(default)]
    pub relay: bool,
    #[serde(default)]
    pub stale: bool,
    #[serde(default)]
    pub metric_name: Option<String>,
    #[serde(default)]
    pub metric_value: Option<f64>,
    #[serde(default)]
    pub epoch: Option<i64>,
    #[serde(default)]
    pub gpus_total: Option<i64>,
    #[serde(default)]
    pub gpus_free: Option<i64>,
    /// Gossip nodes merged (consensus denominator numerator); drives live edge styling.
    #[serde(default)]
    pub gnodes: Option<i64>,
    #[serde(default)]
    pub host: Option<String>,
    #[serde(default)]
    pub port: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TreeResp {
    #[serde(default)]
    pub nodes: Vec<TreeNode>,
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub branch: i64,
    #[serde(default)]
    pub depth: i64,
    #[serde(default)]
    pub fleet: i64,
    #[serde(default)]
    pub ts: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SseJobEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub v: Option<i64>,
    pub job_id: String,
    pub fleet: String,
    pub state: String,
    #[serde(default)]
    pub pod: Option<String>,
    #[serde(default)]
    pub ts: Option<f64>,
}

impl SseJobEvent {
    pub fn is_job(&self) -> bool {
        self.kind == "job"
    }
}

/// Live box/instance delta from the portal SSE stream (`type: "box"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SseBoxEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: serde_json::Value,
    #[serde(default, deserialize_with = "deserialize_patch_option")]
    /// Present with `null` clears assignment; absent leaves the row unchanged.
    pub fleet_id: Option<Option<String>>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub provision_state: Option<String>,
    #[serde(default)]
    pub assignable: Option<bool>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub gpu_name: Option<String>,
    #[serde(default)]
    pub num_gpus: Option<i64>,
    #[serde(default)]
    pub pod_name: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub ts: Option<f64>,
    #[serde(default)]
    pub v: Option<i64>,
}

impl SseBoxEvent {
    pub fn is_box(&self) -> bool {
        self.kind == "box"
    }

    pub fn id_str(&self) -> String {
        match &self.id {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Number(n) => n.to_string(),
            other => other.to_string(),
        }
    }
}

/// Live fleet delta from the portal SSE stream (`type: "fleet"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SseFleetEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub n_pods: Option<i64>,
    #[serde(default)]
    pub active: Option<bool>,
    #[serde(default)]
    pub ts: Option<f64>,
    #[serde(default)]
    pub v: Option<i64>,
}

impl SseFleetEvent {
    pub fn is_fleet(&self) -> bool {
        self.kind == "fleet"
    }
}

/// Live topology-node delta from the portal SSE stream (`type: "node"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SseNodeEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub fleet: String,
    pub tag: String,
    #[serde(default)]
    pub pod: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub up: Option<bool>,
    #[serde(default)]
    pub stale: Option<bool>,
    #[serde(default)]
    pub epoch: Option<i64>,
    #[serde(default)]
    pub metric_name: Option<String>,
    #[serde(default)]
    pub metric_value: Option<f64>,
    #[serde(default)]
    pub gpus_free: Option<i64>,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub children: Option<Vec<String>>,
    #[serde(default)]
    pub ts: Option<f64>,
    #[serde(default)]
    pub v: Option<i64>,
}

impl SseNodeEvent {
    pub fn is_node(&self) -> bool {
        self.kind == "node"
    }
}

/// Live provisioning log from `GET /api/boxes/progress?contract=`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BoxProgressResp {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub contract: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub done: Option<bool>,
    #[serde(default)]
    pub lines: Vec<String>,
    #[serde(default)]
    pub error: Option<String>,
}

impl BoxProgressResp {
    pub fn is_terminal(&self) -> bool {
        self.done.unwrap_or(false)
            || self.state.as_deref().is_some_and(|s| {
                matches!(
                    s,
                    "ready" | "error" | "failed" | "done" | "assigned" | "untracked"
                )
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GpuSearchGroup {
    pub gpu_filter: String,
    pub gpu_name: String,
    #[serde(default)]
    pub num_gpus: i64,
    #[serde(default)]
    pub count: Option<serde_json::Value>,
    #[serde(default)]
    pub dph_min: Option<f64>,
    #[serde(default)]
    pub dph_med: Option<f64>,
    #[serde(default)]
    pub dph_max: Option<f64>,
    #[serde(default)]
    pub vram_gb: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct GpuSearchResp {
    #[serde(default)]
    pub ok: bool,
    #[serde(default)]
    pub configured: bool,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub groups: Vec<GpuSearchGroup>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress_with_state(state: &str) -> BoxProgressResp {
        BoxProgressResp {
            state: Some(state.into()),
            ..Default::default()
        }
    }

    #[test]
    fn is_terminal_for_assigned_and_untracked() {
        assert!(progress_with_state("assigned").is_terminal());
        assert!(progress_with_state("untracked").is_terminal());
        assert!(progress_with_state("ready").is_terminal());
        assert!(progress_with_state("error").is_terminal());
        assert!(progress_with_state("failed").is_terminal());
        assert!(progress_with_state("done").is_terminal());
    }

    #[test]
    fn is_terminal_false_for_in_flight() {
        assert!(!progress_with_state("provisioning").is_terminal());
        assert!(!progress_with_state("booting").is_terminal());
        assert!(!BoxProgressResp::default().is_terminal());
    }

    #[test]
    fn is_terminal_honors_done_flag() {
        let resp = BoxProgressResp {
            done: Some(true),
            ..Default::default()
        };
        assert!(resp.is_terminal());
    }
}
