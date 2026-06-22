//! SSE v2 cache patching — port of `web_app/src/lib/ssePatch.ts`.

use fabric_types::{
    Fleet, FleetsResp, Instance, InstancesResp, Job, RunScalars, RunsSummary, SseBoxEvent,
    SseFleetEvent, SseJobEvent, SseNodeEvent, SseRunEvent, TreeNode, TreeResp,
};
use std::collections::HashMap;

const DEFAULT_MAX_POINTS: usize = 600;

/// Result of applying a box or fleet delta to a cached collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchOutcome {
    /// An existing row matched the event id and was patched in place.
    Updated,
    /// No matching row was found; the caller should refetch the collection.
    NotFound,
}

/// Result of applying a node delta to a cached topology tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreePatchOutcome {
    /// An existing node matched the event tag and was patched in place.
    Updated,
    /// No node matched the tag, so a new node was appended.
    Inserted,
}

/// Patch the matching run row in place. Returns `true` when the run was found and updated.
pub fn patch_summary(summary: &mut RunsSummary, ev: &SseRunEvent) -> bool {
    let Some(idx) = summary
        .runs
        .iter()
        .position(|r| r.pod == ev.pod && r.name == ev.run)
    else {
        return false;
    };

    let row = &mut summary.runs[idx];
    apply_scalars(row, &ev.scalars);
    if let Some(headline) = &ev.headline {
        if headline.best {
            row.best = Some(headline.value);
        }
    }
    true
}

fn apply_scalars(row: &mut RunScalars, scalars: &HashMap<String, serde_json::Value>) {
    for (key, val) in scalars {
        if val.is_null() {
            continue;
        }
        match key.as_str() {
            "status" => row.status = val.as_str().map(str::to_string),
            "metric" => row.metric = val.as_str().map(str::to_string),
            "label" => row.label = val.as_str().map(str::to_string),
            "grid" => row.grid = val.as_str().map(str::to_string),
            "dataset" => row.dataset = val.as_str().map(str::to_string),
            "last_epoch" => row.last_epoch = val.as_i64(),
            "n" => row.n = val.as_i64(),
            "eta_sec" => row.eta_sec = val.as_f64(),
            "sec_per_epoch" => row.sec_per_epoch = val.as_f64(),
            "last_top1" => row.last_top1 = val.as_f64(),
            "last_tgt" => row.last_tgt = val.as_f64(),
            "last_lr" => row.last_lr = val.as_f64(),
            "best" => row.best = val.as_f64(),
            "total_epochs" => row.total_epochs = val.as_i64(),
            "created" => row.created = val.as_f64().or_else(|| val.as_i64().map(|n| n as f64)),
            "sweep" => row.sweep = val.as_str().map(str::to_string),
            _ => {
                row.extra.insert(key.clone(), val.clone());
            }
        }
    }
}

/// Append one SSE point onto a metric series map (`epoch` + metric keys).
pub fn append_point(
    epochs: &mut Vec<i64>,
    metrics: &mut HashMap<String, Vec<f64>>,
    point: &HashMap<String, serde_json::Value>,
    cap: usize,
) {
    let Some(epoch) = point.get("epoch").and_then(|v| v.as_i64()) else {
        return;
    };
    epochs.push(epoch);
    trim_vec(epochs, cap);

    for (key, val) in point {
        if key == "epoch" {
            continue;
        }
        if let Some(entry) = metrics.get_mut(key) {
            if let Some(n) = val.as_f64().or_else(|| val.as_i64().map(|v| v as f64)) {
                entry.push(n);
                trim_vec(entry, cap);
            }
        }
    }
}

fn trim_vec<T>(arr: &mut Vec<T>, cap: usize) {
    if cap > 0 && arr.len() > cap {
        let drop = arr.len() - cap;
        arr.drain(0..drop);
    }
}

pub fn default_max_points() -> usize {
    DEFAULT_MAX_POINTS
}

/// Patch the matching instance in place from a `box` SSE delta.
///
/// Only fields present on the event override the cached row, keeping the patch
/// forward-compatible with portal events that omit fields. Removes the row when
/// `provision_state` is `"destroyed"` (pending rent slots replaced by real ids).
pub fn patch_box(resp: &mut InstancesResp, ev: &SseBoxEvent) -> PatchOutcome {
    if ev.provision_state.as_deref() == Some("destroyed") {
        let id = ev.id_str();
        let before = resp.instances.len();
        resp.instances.retain(|i| i.id_str() != id);
        if resp.instances.len() < before {
            resp.total = resp.instances.len() as i64;
            return PatchOutcome::Updated;
        }
        return PatchOutcome::NotFound;
    }
    let id = ev.id_str();
    let Some(row) = resp.instances.iter_mut().find(|i| i.id_str() == id) else {
        return PatchOutcome::NotFound;
    };
    patch_instance(row, ev);
    PatchOutcome::Updated
}

/// Insert a new instance row from a box SSE delta (rent/assign in-flight).
pub fn insert_box(resp: &mut InstancesResp, ev: &SseBoxEvent) {
    let fleet_id = ev.fleet_id.as_ref().and_then(|inner| inner.clone());
    resp.instances.push(Instance {
        id: ev.id.clone(),
        label: ev.label.clone(),
        provider: "nebius".into(),
        gpu_name: ev.gpu_name.clone(),
        num_gpus: ev.num_gpus,
        status: ev.status.clone(),
        fleet_id,
        pod_name: ev.pod_name.clone(),
        provision_state: ev.provision_state.clone(),
        assignable: ev.assignable,
        error: ev.error.clone(),
        ..Default::default()
    });
    if let Some(row) = resp.instances.last_mut() {
        apply_untracked_assignable(row);
    }
    resp.total = resp.instances.len() as i64;
    resp.unassigned = resp
        .instances
        .iter()
        .filter(|i| i.fleet_id.is_none())
        .count() as i64;
}

fn instance_in_flight(row: &Instance) -> bool {
    row.id_str().starts_with("pending-")
        || matches!(
            row.provision_state.as_deref(),
            Some("booting" | "provisioning" | "creating" | "pending")
        )
        || matches!(
            row.status.as_deref(),
            Some("booting" | "provisioning" | "creating")
        )
}

/// Merge a reconcile poll with the cached roster, preserving local in-flight rows
/// the portal has not surfaced yet (pending rent slots, SSE booting rows, etc.).
pub fn merge_instances_reconcile(cached: &InstancesResp, mut fetched: InstancesResp) -> InstancesResp {
    let fetched_ids: std::collections::HashSet<_> =
        fetched.instances.iter().map(|i| i.id_str()).collect();
    for row in &cached.instances {
        if fetched_ids.contains(&row.id_str()) || !instance_in_flight(row) {
            continue;
        }
        fetched.instances.push(row.clone());
    }
    fetched.total = fetched.instances.len() as i64;
    fetched.unassigned = fetched
        .instances
        .iter()
        .filter(|i| i.fleet_id.is_none())
        .count() as i64;
    fetched
}

fn patch_instance(row: &mut Instance, ev: &SseBoxEvent) {
    if let Some(fleet_id) = &ev.fleet_id {
        row.fleet_id = fleet_id.clone();
    }
    if ev.status.is_some() {
        row.status = ev.status.clone();
    }
    if ev.provision_state.is_some() {
        row.provision_state = ev.provision_state.clone();
    }
    if ev.assignable.is_some() {
        row.assignable = ev.assignable;
    }
    if ev.error.is_some() {
        row.error = ev.error.clone();
    }
    if ev.gpu_name.is_some() {
        row.gpu_name = ev.gpu_name.clone();
    }
    if ev.num_gpus.is_some() {
        row.num_gpus = ev.num_gpus;
    }
    if ev.pod_name.is_some() {
        row.pod_name = ev.pod_name.clone();
    }
    if ev.label.is_some() {
        row.label = ev.label.clone();
    }
    apply_untracked_assignable(row);
}

/// An `untracked` box is back in the free pool and can be (re)assigned, even if
/// the portal omitted `assignable` on the delta.
fn apply_untracked_assignable(row: &mut Instance) {
    if row.provision_state.as_deref() == Some("untracked") {
        row.assignable = Some(true);
    }
}

/// Patch the matching fleet in place from a `fleet` SSE delta.
pub fn patch_fleet(resp: &mut FleetsResp, ev: &SseFleetEvent) -> PatchOutcome {
    let Some(row) = resp.fleets.iter_mut().find(|f| f.id == ev.id) else {
        return PatchOutcome::NotFound;
    };
    patch_fleet_row(row, ev);
    PatchOutcome::Updated
}

fn patch_fleet_row(row: &mut Fleet, ev: &SseFleetEvent) {
    if let Some(status) = &ev.status {
        row.status = status.clone();
    }
    if let Some(n_pods) = ev.n_pods {
        row.n_pods = n_pods;
    }
    if let Some(active) = ev.active {
        row.active = active;
    }
}

/// Patch the matching job in place by `job_id` from a `job` SSE delta.
///
/// Updates the job's overall state and, when the event names a pod, that pod's
/// row. Returns [`PatchOutcome::NotFound`] when no cached job matches, signalling
/// the caller to refetch the job list (e.g. a brand-new job appeared).
pub fn patch_job(jobs: &mut [Job], ev: &SseJobEvent) -> PatchOutcome {
    let Some(job) = jobs.iter_mut().find(|j| j.job_id == ev.job_id) else {
        return PatchOutcome::NotFound;
    };
    job.state = ev.state.clone();
    if let Some(pod) = &ev.pod {
        if let Some(row) = job.pods.iter_mut().find(|p| &p.pod == pod) {
            row.state = ev.state.clone();
        }
    }
    PatchOutcome::Updated
}

/// Patch the matching topology node in place from a `node` SSE delta, or append
/// a new node when the tag is not yet present in the cached tree.
pub fn patch_tree_node(resp: &mut TreeResp, ev: &SseNodeEvent) -> TreePatchOutcome {
    if let Some(node) = resp.nodes.iter_mut().find(|n| n.tag == ev.tag) {
        patch_node(node, ev);
        TreePatchOutcome::Updated
    } else {
        let mut node = TreeNode {
            tag: ev.tag.clone(),
            ..Default::default()
        };
        patch_node(&mut node, ev);
        resp.nodes.push(node);
        TreePatchOutcome::Inserted
    }
}

fn patch_node(node: &mut TreeNode, ev: &SseNodeEvent) {
    if ev.state.is_some() {
        node.state = ev.state.clone();
    }
    if let Some(up) = ev.up {
        node.up = up;
    }
    if let Some(stale) = ev.stale {
        node.stale = stale;
    }
    if ev.epoch.is_some() {
        node.epoch = ev.epoch;
    }
    if ev.metric_name.is_some() {
        node.metric_name = ev.metric_name.clone();
    }
    if ev.metric_value.is_some() {
        node.metric_value = ev.metric_value;
    }
    if ev.gpus_free.is_some() {
        node.gpus_free = ev.gpus_free;
    }
    if ev.parent.is_some() {
        node.parent = ev.parent.clone();
    }
    if let Some(children) = &ev.children {
        node.children = children.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fabric_types::{GpuSummary, JobPod, SseHeadline};

    fn mk_run(best: f64, epoch: i64) -> RunScalars {
        RunScalars {
            pod: "f:n1".into(),
            name: "r_n1".into(),
            group: "r".into(),
            fleet: "f".into(),
            best: Some(best),
            last_epoch: Some(epoch),
            status: Some("running".into()),
            ..Default::default()
        }
    }

    fn mk_summary(runs: Vec<RunScalars>) -> RunsSummary {
        RunsSummary {
            runs,
            gpus: GpuSummary::default(),
            ..Default::default()
        }
    }

    fn run_event(scalars: HashMap<String, serde_json::Value>, headline: Option<SseHeadline>) -> SseRunEvent {
        SseRunEvent {
            kind: "run".into(),
            pod: "f:n1".into(),
            run: "r_n1".into(),
            v: Some(2),
            ts: None,
            headline,
            scalars,
            point: HashMap::new(),
        }
    }

    #[test]
    fn patch_updates_best_and_scalars() {
        let mut summary = mk_summary(vec![mk_run(60.0, 10)]);
        let mut scalars = HashMap::new();
        scalars.insert("last_epoch".into(), serde_json::json!(12));
        scalars.insert("eta_sec".into(), serde_json::json!(3600));
        scalars.insert("status".into(), serde_json::json!("running"));
        let headline = SseHeadline {
            key: "top1".into(),
            value: 64.47,
            best: true,
        };
        assert!(patch_summary(
            &mut summary,
            &run_event(scalars, Some(headline))
        ));
        let row = &summary.runs[0];
        assert_eq!(row.best, Some(64.47));
        assert_eq!(row.last_epoch, Some(12));
        assert_eq!(row.eta_sec, Some(3600.0));
    }

    #[test]
    fn patch_updates_total_epochs_and_created() {
        let mut summary = mk_summary(vec![mk_run(60.0, 10)]);
        let mut scalars = HashMap::new();
        scalars.insert("total_epochs".into(), serde_json::json!(500));
        scalars.insert("created".into(), serde_json::json!(1718880000));
        scalars.insert("sweep".into(), serde_json::json!("lr_sweep"));
        assert!(patch_summary(&mut summary, &run_event(scalars, None)));
        let row = &summary.runs[0];
        assert_eq!(row.total_epochs, Some(500));
        assert_eq!(row.created, Some(1718880000.0));
        assert_eq!(row.sweep.as_deref(), Some("lr_sweep"));
    }

    #[test]
    fn patch_returns_false_for_unknown_run() {
        let mut summary = mk_summary(vec![mk_run(1.0, 1)]);
        let ev = SseRunEvent {
            kind: "run".into(),
            pod: "f:n2".into(),
            run: "other".into(),
            v: Some(2),
            ts: None,
            headline: None,
            scalars: HashMap::new(),
            point: HashMap::new(),
        };
        assert!(!patch_summary(&mut summary, &ev));
    }

    fn mk_instance(id: &str) -> Instance {
        Instance {
            id: serde_json::json!(id),
            status: Some("provisioning".into()),
            provision_state: Some("pending".into()),
            assignable: Some(false),
            ..Default::default()
        }
    }

    #[test]
    fn patch_box_updates_matching_instance() {
        let mut resp = InstancesResp {
            instances: vec![mk_instance("box-1")],
            ..Default::default()
        };
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("box-1"),
            status: Some("ready".into()),
            provision_state: Some("done".into()),
            assignable: Some(true),
            fleet_id: Some(Some("f1".into())),
            ..Default::default()
        };
        assert_eq!(patch_box(&mut resp, &ev), PatchOutcome::Updated);
        let row = &resp.instances[0];
        assert_eq!(row.status.as_deref(), Some("ready"));
        assert_eq!(row.provision_state.as_deref(), Some("done"));
        assert_eq!(row.assignable, Some(true));
        assert_eq!(row.fleet_id.as_deref(), Some("f1"));
    }

    #[test]
    fn patch_box_clears_fleet_id_on_unassign() {
        let mut resp = InstancesResp {
            instances: vec![Instance {
                id: serde_json::json!("box-1"),
                fleet_id: Some("f1".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("box-1"),
            fleet_id: Some(None),
            ..Default::default()
        };
        assert_eq!(patch_box(&mut resp, &ev), PatchOutcome::Updated);
        assert!(resp.instances[0].fleet_id.is_none());
    }

    #[test]
    fn patch_box_matches_numeric_id() {
        let mut resp = InstancesResp {
            instances: vec![Instance {
                id: serde_json::json!(42),
                ..Default::default()
            }],
            ..Default::default()
        };
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!(42),
            status: Some("error".into()),
            error: Some("boom".into()),
            ..Default::default()
        };
        assert_eq!(patch_box(&mut resp, &ev), PatchOutcome::Updated);
        assert_eq!(resp.instances[0].status.as_deref(), Some("error"));
        assert_eq!(resp.instances[0].error.as_deref(), Some("boom"));
    }

    #[test]
    fn insert_box_adds_pending_rent_row() {
        let mut resp = InstancesResp::default();
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("pending-1"),
            provision_state: Some("provisioning".into()),
            gpu_name: Some("H100".into()),
            num_gpus: Some(8),
            ..Default::default()
        };
        insert_box(&mut resp, &ev);
        assert_eq!(resp.instances.len(), 1);
        assert_eq!(resp.instances[0].provision_state.as_deref(), Some("provisioning"));
    }

    #[test]
    fn patch_box_removes_destroyed() {
        let mut resp = InstancesResp {
            instances: vec![mk_instance("box-1")],
            total: 1,
            ..Default::default()
        };
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("box-1"),
            provision_state: Some("destroyed".into()),
            ..Default::default()
        };
        assert_eq!(patch_box(&mut resp, &ev), PatchOutcome::Updated);
        assert!(resp.instances.is_empty());
    }

    #[test]
    fn patch_box_untracked_sets_assignable() {
        let mut resp = InstancesResp {
            instances: vec![Instance {
                id: serde_json::json!("box-1"),
                fleet_id: Some("f1".into()),
                assignable: Some(false),
                provision_state: Some("assigned".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("box-1"),
            fleet_id: Some(None),
            provision_state: Some("untracked".into()),
            ..Default::default()
        };
        assert_eq!(patch_box(&mut resp, &ev), PatchOutcome::Updated);
        let row = &resp.instances[0];
        assert_eq!(row.provision_state.as_deref(), Some("untracked"));
        assert_eq!(row.assignable, Some(true));
        assert!(row.fleet_id.is_none());
    }

    #[test]
    fn insert_box_untracked_is_assignable() {
        let mut resp = InstancesResp::default();
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("box-9"),
            provision_state: Some("untracked".into()),
            ..Default::default()
        };
        insert_box(&mut resp, &ev);
        assert_eq!(resp.instances[0].assignable, Some(true));
    }

    #[test]
    fn merge_instances_reconcile_keeps_local_in_flight() {
        let cached = InstancesResp {
            instances: vec![
                Instance {
                    id: serde_json::json!("pending-1"),
                    provision_state: Some("provisioning".into()),
                    ..Default::default()
                },
                Instance {
                    id: serde_json::json!("vm-42"),
                    provision_state: Some("booting".into()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let fetched = InstancesResp {
            instances: vec![Instance {
                id: serde_json::json!("vm-other"),
                provision_state: Some("untracked".into()),
                assignable: Some(true),
                ..Default::default()
            }],
            total: 1,
            unassigned: 1,
            ..Default::default()
        };
        let merged = merge_instances_reconcile(&cached, fetched);
        assert_eq!(merged.instances.len(), 3);
        assert!(merged.instances.iter().any(|i| i.id_str() == "pending-1"));
        assert!(merged.instances.iter().any(|i| i.id_str() == "vm-42"));
    }

    #[test]
    fn patch_box_returns_not_found() {
        let mut resp = InstancesResp {
            instances: vec![mk_instance("box-1")],
            ..Default::default()
        };
        let ev = SseBoxEvent {
            kind: "box".into(),
            id: serde_json::json!("box-2"),
            status: Some("ready".into()),
            ..Default::default()
        };
        assert_eq!(patch_box(&mut resp, &ev), PatchOutcome::NotFound);
        assert_eq!(resp.instances[0].status.as_deref(), Some("provisioning"));
    }

    #[test]
    fn patch_fleet_updates_matching_fleet() {
        let mut resp = FleetsResp {
            fleets: vec![Fleet {
                id: "f1".into(),
                status: "idle".into(),
                n_pods: 2,
                active: false,
                ..Default::default()
            }],
            ..Default::default()
        };
        let ev = SseFleetEvent {
            kind: "fleet".into(),
            id: "f1".into(),
            status: Some("running".into()),
            n_pods: Some(5),
            active: Some(true),
            ..Default::default()
        };
        assert_eq!(patch_fleet(&mut resp, &ev), PatchOutcome::Updated);
        let row = &resp.fleets[0];
        assert_eq!(row.status, "running");
        assert_eq!(row.n_pods, 5);
        assert!(row.active);
    }

    #[test]
    fn patch_fleet_returns_not_found() {
        let mut resp = FleetsResp::default();
        let ev = SseFleetEvent {
            kind: "fleet".into(),
            id: "missing".into(),
            status: Some("running".into()),
            ..Default::default()
        };
        assert_eq!(patch_fleet(&mut resp, &ev), PatchOutcome::NotFound);
    }

    fn mk_job(job_id: &str, state: &str, pods: Vec<JobPod>) -> Job {
        Job {
            job_id: job_id.into(),
            fleet: "f1".into(),
            state: state.into(),
            pods,
            ..Default::default()
        }
    }

    fn job_event(job_id: &str, state: &str, pod: Option<&str>) -> SseJobEvent {
        SseJobEvent {
            kind: "job".into(),
            v: Some(1),
            job_id: job_id.into(),
            fleet: "f1".into(),
            state: state.into(),
            pod: pod.map(str::to_string),
            ts: None,
        }
    }

    #[test]
    fn patch_job_updates_state() {
        let mut jobs = vec![mk_job("j1", "running", Vec::new())];
        assert_eq!(
            patch_job(&mut jobs, &job_event("j1", "done", None)),
            PatchOutcome::Updated
        );
        assert_eq!(jobs[0].state, "done");
    }

    #[test]
    fn patch_job_updates_pod_state() {
        let mut jobs = vec![mk_job(
            "j1",
            "running",
            vec![
                JobPod {
                    pod: "f:n1".into(),
                    state: "running".into(),
                    ..Default::default()
                },
                JobPod {
                    pod: "f:n2".into(),
                    state: "running".into(),
                    ..Default::default()
                },
            ],
        )];
        assert_eq!(
            patch_job(&mut jobs, &job_event("j1", "error", Some("f:n2"))),
            PatchOutcome::Updated
        );
        assert_eq!(jobs[0].state, "error");
        assert_eq!(jobs[0].pods[0].state, "running");
        assert_eq!(jobs[0].pods[1].state, "error");
    }

    #[test]
    fn patch_job_returns_not_found() {
        let mut jobs = vec![mk_job("j1", "running", Vec::new())];
        assert_eq!(
            patch_job(&mut jobs, &job_event("missing", "done", None)),
            PatchOutcome::NotFound
        );
        assert_eq!(jobs[0].state, "running");
    }

    #[test]
    fn patch_tree_node_updates_existing() {
        let mut resp = TreeResp {
            nodes: vec![TreeNode {
                tag: "n1".into(),
                up: false,
                state: Some("down".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let ev = SseNodeEvent {
            kind: "node".into(),
            fleet: "f1".into(),
            tag: "n1".into(),
            state: Some("up".into()),
            up: Some(true),
            gpus_free: Some(4),
            children: Some(vec!["n2".into()]),
            ..Default::default()
        };
        assert_eq!(patch_tree_node(&mut resp, &ev), TreePatchOutcome::Updated);
        let node = &resp.nodes[0];
        assert_eq!(node.state.as_deref(), Some("up"));
        assert!(node.up);
        assert_eq!(node.gpus_free, Some(4));
        assert_eq!(node.children, vec!["n2".to_string()]);
        assert_eq!(resp.nodes.len(), 1);
    }

    #[test]
    fn patch_tree_node_inserts_unknown() {
        let mut resp = TreeResp::default();
        let ev = SseNodeEvent {
            kind: "node".into(),
            fleet: "f1".into(),
            tag: "new-node".into(),
            up: Some(true),
            parent: Some("root".into()),
            ..Default::default()
        };
        assert_eq!(patch_tree_node(&mut resp, &ev), TreePatchOutcome::Inserted);
        assert_eq!(resp.nodes.len(), 1);
        assert_eq!(resp.nodes[0].tag, "new-node");
        assert!(resp.nodes[0].up);
        assert_eq!(resp.nodes[0].parent.as_deref(), Some("root"));
    }
}
