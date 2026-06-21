//! Fleet / GPU / topology types — mirrors `web_app/src/types.ts` Phase 3 endpoints.

use serde::{Deserialize, Serialize};

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
            || self
                .state
                .as_deref()
                .is_some_and(|s| matches!(s, "ready" | "error" | "failed" | "done"))
    }
}
