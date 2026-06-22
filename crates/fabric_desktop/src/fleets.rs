//! Fleet deck — LEFT roster · CENTER relay graph board · RIGHT ops rail.
//!
//! `FleetsView` owns all fleet state and the network channel; rendering is split
//! across [`crate::fleet_canvas`] (graph board + drag/drop) and
//! [`crate::fleet_ops`] (ops rail). Box assignment uses the native GPUI
//! drag-and-drop API: chips carry a [`BoxDrag`] payload and the board is the
//! drop target, so no manual cross-panel mouse tracking is required.

use crate::fleet_canvas::{fit_scale, fleet_board};
use crate::fleet_layout::{layout_tree, TreeLayout};
use crate::fleet_ops::ops_rail;
use crate::network::{FleetsMsg, NetworkCommand};
use crate::theme::Theme;
use fabric_live::{
    insert_box, merge_instances_reconcile, patch_box, patch_fleet, patch_job, patch_tree_node,
    PatchOutcome, TreePatchOutcome,
};
use fabric_types::{BoxProgressResp, Fleet, FleetsResp, GpuSearchResp, Instance, InstancesResp, Job, TreeResp};
use futures::channel::mpsc::UnboundedSender;
use gpui::{div, prelude::*, px, Context, MouseButton, Render, SharedString, Window};
use serde_json::json;
use std::collections::{HashMap, HashSet};

const DEFAULT_BRANCH: u32 = 8;

/// Drag payload carried from an ops-rail box chip to the graph board drop zone.
#[derive(Clone, Debug)]
pub struct BoxDrag {
    pub contract: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct TreeFetchKey {
    fleet: String,
    branch: u32,
    phased: bool,
}

pub struct FleetsView {
    pub fleets: Option<FleetsResp>,
    pub instances: Option<InstancesResp>,
    pub tree: Option<TreeResp>,
    pub jobs: Vec<Job>,
    pub selected_fleet: String,
    pub branch: u32,
    pub error: Option<SharedString>,
    pub action_msg: Option<SharedString>,
    pub refreshing: bool,
    pub tree_loading: bool,
    pub cmd_tx: Option<UnboundedSender<NetworkCommand>>,

    pub layout: TreeLayout,
    pub fleet_size: i64,
    pub board_scale: f32,

    pub selected_node: Option<String>,
    pub selected_box: Option<String>,
    pub selected_job: Option<String>,

    /// SSE stream connection state, mirrored from the live event stream.
    pub live: bool,
    /// `job_id` → index into `jobs`, rebuilt whenever the job list is replaced.
    job_index: HashMap<String, usize>,
    /// `box id` → index into `instances`, rebuilt whenever the roster is replaced.
    box_index: HashMap<String, usize>,
    /// `fleet id` → index into `fleets`, rebuilt whenever the roster is replaced.
    fleet_index: HashMap<String, usize>,
    /// `node tag` → index into `tree.nodes`, rebuilt whenever the tree is replaced.
    node_index: HashMap<String, usize>,
    tree_pending: HashSet<TreeFetchKey>,
    /// Remaining tree responses expected from an in-flight phased fetch (2 → 0).
    tree_phased_remaining: u8,
    pub progress: Option<BoxProgressResp>,
    progress_contract: Option<String>,
    operator_email: Option<String>,
    pub(crate) rent_open: bool,
    pub(crate) rent_gpus_per_box: u32,
    pub(crate) rent_gpu_filter: String,
    pub(crate) rent_count: u32,
    pub(crate) rent_disk_gb: u32,
    pub(crate) gpu_offers: Option<GpuSearchResp>,
    pub(crate) gpu_search_loading: bool,
}

impl FleetsView {
    pub fn new() -> Self {
        Self {
            fleets: None,
            instances: None,
            tree: None,
            jobs: Vec::new(),
            selected_fleet: load_selected_fleet(),
            branch: DEFAULT_BRANCH,
            error: None,
            action_msg: None,
            refreshing: false,
            tree_loading: false,
            cmd_tx: None,
            layout: TreeLayout::default(),
            fleet_size: 0,
            board_scale: 1.0,
            selected_node: None,
            selected_box: None,
            selected_job: None,
            live: false,
            job_index: HashMap::new(),
            box_index: HashMap::new(),
            fleet_index: HashMap::new(),
            node_index: HashMap::new(),
            tree_pending: HashSet::new(),
            tree_phased_remaining: 0,
            progress: None,
            progress_contract: None,
            operator_email: None,
            rent_open: false,
            rent_gpus_per_box: 1,
            rent_gpu_filter: String::new(),
            rent_count: 1,
            rent_disk_gb: 200,
            gpu_offers: None,
            gpu_search_loading: false,
        }
    }

    pub fn attach(&mut self, cmd_tx: UnboundedSender<NetworkCommand>) {
        self.cmd_tx = Some(cmd_tx);
    }

    pub fn detach(&mut self) {
        self.cmd_tx = None;
        self.refreshing = false;
        self.rent_open = false;
        self.gpu_search_loading = false;
    }

    pub fn set_operator_email(&mut self, email: Option<String>) {
        self.operator_email = email;
    }

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        self.refresh_deck(true, cx);
    }

    pub fn on_visible(&mut self, cx: &mut Context<Self>) {
        if self.fleets.is_none() || self.instances.is_none() || self.tree.is_none() {
            self.refresh_all(cx);
        }
    }

    pub(crate) fn send(&self, cmd: NetworkCommand) {
        if let Some(tx) = &self.cmd_tx {
            let _ = tx.unbounded_send(cmd);
        }
    }

    fn refresh_deck(&mut self, probe_tree_after: bool, cx: &mut Context<Self>) {
        if !self.selected_fleet.is_empty() {
            self.tree_phased_remaining = if probe_tree_after { 2 } else { 1 };
            self.tree_loading = true;
        }
        self.send(NetworkCommand::RefreshFleetDeck {
            fleet: self.selected_fleet.clone(),
            branch: self.branch,
            probe_tree_after,
        });
        cx.notify();
    }

    fn fetch_tree(&mut self, probe: bool, cx: &mut Context<Self>) {
        if self.selected_fleet.is_empty() {
            return;
        }
        let key = TreeFetchKey {
            fleet: self.selected_fleet.clone(),
            branch: self.branch,
            phased: false,
        };
        if self.tree_pending.contains(&key) {
            return;
        }
        self.tree_pending.insert(key);
        self.tree_phased_remaining = 0;
        self.tree_loading = true;
        self.send(NetworkCommand::FetchTree {
            fleet: self.selected_fleet.clone(),
            branch: self.branch,
            probe,
        });
        cx.notify();
    }

    fn fetch_tree_phased(&mut self, cx: &mut Context<Self>) {
        if self.selected_fleet.is_empty() {
            return;
        }
        let key = TreeFetchKey {
            fleet: self.selected_fleet.clone(),
            branch: self.branch,
            phased: true,
        };
        if self.tree_pending.contains(&key) {
            return;
        }
        self.tree_pending.insert(key);
        self.tree_phased_remaining = 2;
        self.tree_loading = true;
        self.send(NetworkCommand::FetchTreePhased {
            fleet: self.selected_fleet.clone(),
            branch: self.branch,
        });
        cx.notify();
    }

    fn clear_tree_pending_for(&mut self, fleet: &str, branch: u32) {
        self.tree_pending
            .retain(|k| k.fleet != fleet || k.branch != branch);
    }

    fn fetch_jobs(&mut self, cx: &mut Context<Self>) {
        if self.selected_fleet.is_empty() {
            return;
        }
        self.send(NetworkCommand::FetchJobs {
            fleet: self.selected_fleet.clone(),
        });
        cx.notify();
    }

    /// Refetch the box roster only — used when a live box delta misses the cache.
    fn fetch_instances(&mut self, cx: &mut Context<Self>) {
        self.send(NetworkCommand::RefreshInstances);
        cx.notify();
    }

    /// Refetch the fleet roster only — used when a live fleet delta misses the cache.
    fn refresh_fleets(&mut self, cx: &mut Context<Self>) {
        self.send(NetworkCommand::RefreshFleets);
        cx.notify();
    }

    pub fn select_fleet(&mut self, id: String, cx: &mut Context<Self>) {
        if self.selected_fleet == id {
            return;
        }
        self.selected_fleet = id.clone();
        save_selected_fleet(&id);
        self.selected_node = None;
        self.selected_job = None;
        self.clear_tree_pending_for(&id, self.branch);
        self.fetch_tree_phased(cx);
        self.fetch_jobs(cx);
        cx.notify();
    }

    pub fn select_node(&mut self, tag: String, cx: &mut Context<Self>) {
        self.selected_node = if self.selected_node.as_deref() == Some(tag.as_str()) {
            None
        } else {
            Some(tag)
        };
        cx.notify();
    }

    pub fn select_box(&mut self, contract: String, cx: &mut Context<Self>) {
        self.selected_box = if self.selected_box.as_deref() == Some(contract.as_str()) {
            None
        } else {
            Some(contract)
        };
        cx.notify();
    }

    pub fn select_job(&mut self, job_id: String, cx: &mut Context<Self>) {
        self.selected_job = if self.selected_job.as_deref() == Some(job_id.as_str()) {
            None
        } else {
            Some(job_id)
        };
        cx.notify();
    }

    fn start_progress_poll(&mut self, contract: &str) {
        self.progress_contract = Some(contract.to_string());
        self.progress = None;
        self.send(NetworkCommand::PollBoxProgress {
            contract: contract.to_string(),
        });
    }

    fn clear_progress(&mut self) {
        self.progress = None;
        self.progress_contract = None;
    }

    /// Assign a GPU box (by contract id) to the active fleet.
    pub fn assign_box(&mut self, contract: &str, cx: &mut Context<Self>) {
        if self.selected_fleet.is_empty() {
            self.action_msg = Some("ASSIGN — pick a fleet first".into());
            cx.notify();
            return;
        }
        self.optimistic_assign(contract);
        self.send(NetworkCommand::FleetAction {
            action: "assign".into(),
            payload: json!({ "contract": contract, "fleet": self.selected_fleet }),
        });
        self.action_msg = Some(format!("ASSIGN {contract} → {}…", self.selected_fleet).into());
        self.start_progress_poll(contract);
        if self.selected_box.as_deref() == Some(contract) {
            self.selected_box = None;
        }
        cx.notify();
    }

    pub fn assign_selected_box(&mut self, cx: &mut Context<Self>) {
        if let Some(contract) = self.selected_box.clone() {
            self.assign_box(&contract, cx);
        }
    }

    pub fn new_fleet(&mut self, cx: &mut Context<Self>) {
        self.send(NetworkCommand::FleetAction {
            action: "newfleet".into(),
            payload: json!({ "name": format!("fleet-{}", chrono_timestamp()) }),
        });
        self.action_msg = Some("NEW FLEET…".into());
        cx.notify();
    }

    pub fn stop_selected_job(&mut self, cx: &mut Context<Self>) {
        if self.selected_fleet.is_empty() {
            self.action_msg = Some("STOP — select a fleet first".into());
            cx.notify();
            return;
        }
        self.send(NetworkCommand::FleetAction {
            action: "stopjob".into(),
            payload: json!({ "fleet": self.selected_fleet }),
        });
        self.action_msg = Some(format!("STOP {}…", self.selected_fleet).into());
        cx.notify();
    }

    pub fn toggle_rent_panel(&mut self, cx: &mut Context<Self>) {
        self.rent_open = !self.rent_open;
        if self.rent_open {
            self.gpu_search_loading = true;
            self.send(NetworkCommand::FetchGpuSearch {
                num_gpus: self.rent_gpus_per_box,
            });
        }
        cx.notify();
    }

    pub fn set_rent_gpus_per_box(&mut self, n: u32, cx: &mut Context<Self>) {
        self.rent_gpus_per_box = n.max(1);
        if self.rent_open {
            self.gpu_search_loading = true;
            self.send(NetworkCommand::FetchGpuSearch {
                num_gpus: self.rent_gpus_per_box,
            });
        }
        cx.notify();
    }

    pub fn bump_rent_gpus_per_box(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = (self.rent_gpus_per_box as i32 + delta).max(1) as u32;
        self.set_rent_gpus_per_box(n, cx);
    }

    pub fn bump_rent_count(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = (self.rent_count as i32 + delta).max(1) as u32;
        self.set_rent_count(n, cx);
    }

    pub fn bump_rent_disk_gb(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = (self.rent_disk_gb as i32 + delta).max(20) as u32;
        self.set_rent_disk_gb(n, cx);
    }

    pub fn select_rent_gpu(&mut self, filter: String, cx: &mut Context<Self>) {
        self.rent_gpu_filter = filter;
        cx.notify();
    }

    pub fn set_rent_count(&mut self, n: u32, cx: &mut Context<Self>) {
        self.rent_count = n.max(1);
        cx.notify();
    }

    pub fn set_rent_disk_gb(&mut self, n: u32, cx: &mut Context<Self>) {
        self.rent_disk_gb = n.max(20);
        cx.notify();
    }

    pub fn submit_rent(&mut self, cx: &mut Context<Self>) {
        let gpu = self.rent_gpu_filter.trim();
        if gpu.is_empty() {
            self.action_msg = Some("RENT — pick a GPU platform".into());
            cx.notify();
            return;
        }
        self.send(NetworkCommand::FleetAction {
            action: "rentgpus".into(),
            payload: json!({
                "gpu_name": gpu,
                "count": self.rent_count,
                "gpus": self.rent_gpus_per_box,
                "disk": self.rent_disk_gb,
            }),
        });
        self.action_msg = Some(format!("RENT {gpu} ×{}…", self.rent_count).into());
        self.rent_open = false;
        cx.notify();
    }

    pub fn unassign_box(&mut self, contract: &str, cx: &mut Context<Self>) {
        self.send(NetworkCommand::FleetAction {
            action: "unassign".into(),
            payload: json!({ "contract": contract }),
        });
        self.action_msg = Some(format!("UNASSIGN {contract}…").into());
        cx.notify();
    }

    pub fn destroy_box(&mut self, contract: &str, cx: &mut Context<Self>) {
        self.send(NetworkCommand::FleetAction {
            action: "destroybox".into(),
            payload: json!({ "contract": contract }),
        });
        self.action_msg = Some(format!("DESTROY {contract}…").into());
        cx.notify();
    }

    pub fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let fleet = self.selected_fleet.clone();
        let branch = self.branch;
        self.clear_tree_pending_for(&fleet, branch);
        self.fetch_tree(true, cx);
        self.action_msg = Some("REFRESH TREE…".into());
        cx.notify();
    }

    /// Replace the job list and rebuild the `job_id → index` lookup.
    fn set_jobs(&mut self, jobs: Vec<Job>) {
        self.job_index = jobs
            .iter()
            .enumerate()
            .map(|(i, j)| (j.job_id.clone(), i))
            .collect();
        self.jobs = jobs;
    }

    /// Replace the box roster and rebuild the `box id → index` lookup.
    fn set_instances(&mut self, resp: InstancesResp) {
        self.rebuild_box_index(&resp.instances);
        self.instances = Some(resp);
    }

    fn rebuild_box_index(&mut self, instances: &[Instance]) {
        self.box_index = instances
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id_str(), i))
            .collect();
    }

    /// Show the box immediately under the target fleet while the portal stages it.
    fn optimistic_assign(&mut self, contract: &str) {
        let Some(resp) = self.instances.as_mut() else {
            return;
        };
        let Some(idx) = self.box_index.get(contract).copied() else {
            return;
        };
        let fleet_name = self
            .fleets
            .as_ref()
            .and_then(|f| {
                f.fleets
                    .iter()
                    .find(|fl| fl.id == self.selected_fleet)
                    .map(|fl| fl.name.clone())
            });
        let row = &mut resp.instances[idx];
        row.fleet_id = Some(self.selected_fleet.clone());
        row.fleet_name = fleet_name;
        row.assignable = Some(false);
        row.provision_state = Some("booting".into());
        row.status = Some("booting".into());
    }

    fn apply_box_delta(&mut self, ev: &fabric_types::SseBoxEvent, cx: &mut Context<Self>) {
        if self.instances.is_none() {
            let mut resp = InstancesResp {
                ok: true,
                configured: true,
                ..Default::default()
            };
            insert_box(&mut resp, ev);
            self.set_instances(resp);
        } else {
            {
                let Some(resp) = self.instances.as_mut() else {
                    return;
                };
                match patch_box(resp, ev) {
                    PatchOutcome::Updated => {}
                    PatchOutcome::NotFound => insert_box(resp, ev),
                }
            }
            if let Some(instances) = self.instances.as_ref().map(|r| r.instances.clone()) {
                self.rebuild_box_index(&instances);
            }
        }
        self.on_box_lifecycle(ev, cx);
    }

    /// React to a box reaching a settled lifecycle state: drop the provisioning
    /// bar for that contract and refresh the relay tree so the new node appears.
    fn on_box_lifecycle(&mut self, ev: &fabric_types::SseBoxEvent, cx: &mut Context<Self>) {
        let id = ev.id_str();
        let provisioned = matches!(
            ev.provision_state.as_deref(),
            Some("assigned" | "untracked" | "ready")
        ) || ev.status.as_deref() == Some("ready");
        let failed = matches!(
            ev.provision_state.as_deref(),
            Some("error" | "failed" | "destroyed")
        );

        if self.progress_contract.as_deref() == Some(id.as_str()) && (provisioned || failed) {
            self.clear_progress();
        }
        if provisioned && !self.selected_fleet.is_empty() {
            let targets_fleet = ev
                .fleet_id
                .as_ref()
                .and_then(|inner| inner.as_ref())
                .is_some_and(|fid| fid == &self.selected_fleet);
            if targets_fleet {
                self.fetch_tree_phased(cx);
            }
        }
    }

    fn refresh_after_action(&mut self, action: &str, cx: &mut Context<Self>) {
        match action {
            // Pending chips are inserted locally and a reconcile poll catches up,
            // so an immediate (racing) fetch_instances would just return stale rows.
            "rentgpus" => {}
            // Optimistic booting + SSE/progress poll own the assign lifecycle.
            "assign" => {}
            // Unassign/destroy also reshape the fleet's relay tree.
            "unassign" | "destroybox" => {
                self.fetch_instances(cx);
                if !self.selected_fleet.is_empty() {
                    self.fetch_tree_phased(cx);
                }
            }
            _ => self.refresh_deck(true, cx),
        }
    }

    /// Handle a successful `rentgpus` response: seed provisioning chips from the
    /// `pending` array locally (immediate feedback) and start a background poll
    /// that reconciles them against the portal's real box rows.
    fn handle_rent_ok(&mut self, v: &serde_json::Value, cx: &mut Context<Self>) {
        let gpu_name = v
            .get("gpu_name")
            .and_then(|g| g.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .or_else(|| {
                self.gpu_offers.as_ref().and_then(|o| {
                    o.groups
                        .iter()
                        .find(|g| g.gpu_filter == self.rent_gpu_filter)
                        .map(|g| g.gpu_name.clone())
                })
            })
            .unwrap_or_else(|| self.rent_gpu_filter.clone());
        let num_gpus = v
            .get("gpus")
            .and_then(|g| g.as_i64())
            .unwrap_or(self.rent_gpus_per_box as i64);

        if let Some(pending) = v.get("pending").and_then(|p| p.as_array()) {
            if self.instances.is_none() {
                self.instances = Some(InstancesResp {
                    ok: true,
                    configured: true,
                    ..Default::default()
                });
            }
            if let Some(resp) = self.instances.as_mut() {
                for entry in pending {
                    if let Some(id) = entry.as_str() {
                        insert_pending_box_id(resp, id, &gpu_name, num_gpus);
                    } else {
                        insert_pending_box(resp, entry, &gpu_name, num_gpus);
                    }
                }
            }
            if let Some(instances) = self.instances.as_ref().map(|r| r.instances.clone()) {
                self.rebuild_box_index(&instances);
            }
        }
        self.send(NetworkCommand::PollInstancesReconcile);
        cx.notify();
    }

    /// Replace the fleet roster and rebuild the `fleet id → index` lookup.
    fn set_fleets(&mut self, resp: FleetsResp) {
        self.fleet_index = resp
            .fleets
            .iter()
            .enumerate()
            .map(|(i, f)| (f.id.clone(), i))
            .collect();
        self.fleets = Some(resp);
    }

    /// Recompute layout (and the node index) from the cached tree after an
    /// in-place topology patch, reusing the full [`Self::apply_tree`] funnel.
    fn relayout_from_cache(&mut self) {
        if let Some(tree) = self.tree.take() {
            self.apply_tree(tree);
        }
    }

    pub fn handle_msg(&mut self, msg: FleetsMsg, cx: &mut Context<Self>) {
        match msg {
            FleetsMsg::Fleets(Ok(resp)) => {
                self.refreshing = false;
                self.error = None;
                if self.selected_fleet.is_empty() {
                    self.selected_fleet = if !resp.active.is_empty() {
                        resp.active.clone()
                    } else if !resp.default.is_empty() {
                        resp.default.clone()
                    } else {
                        resp.fleets
                            .first()
                            .map(|f| f.id.clone())
                            .unwrap_or_default()
                    };
                    save_selected_fleet(&self.selected_fleet);
                }
                self.set_fleets(resp);
                cx.notify();
            }
            FleetsMsg::Fleets(Err(e)) => {
                self.refreshing = false;
                self.error = Some(format!("FLEETS — {e}").into());
                cx.notify();
            }
            FleetsMsg::Instances { result: Ok(fetched), reconcile } => {
                let resp = if reconcile {
                    if let Some(cached) = self.instances.as_ref() {
                        merge_instances_reconcile(cached, fetched)
                    } else {
                        fetched
                    }
                } else {
                    fetched
                };
                self.set_instances(resp);
                cx.notify();
            }
            FleetsMsg::Instances { result: Err(e), .. } => {
                self.error = Some(format!("BOXES — {e}").into());
                cx.notify();
            }
            FleetsMsg::Tree { result } => {
                let fleet = self.selected_fleet.clone();
                let branch = self.branch;
                match &result {
                    Ok(tree) => self.apply_tree(tree.clone()),
                    Err(e) => {
                        self.tree_phased_remaining = 0;
                        self.tree_loading = false;
                        self.clear_tree_pending_for(&fleet, branch);
                        self.error = Some(format!("TREE — {e}").into());
                    }
                }
                if result.is_ok() {
                    if self.tree_phased_remaining > 0 {
                        self.tree_phased_remaining -= 1;
                        if self.tree_phased_remaining == 0 {
                            self.tree_loading = false;
                            self.clear_tree_pending_for(&fleet, branch);
                        }
                    } else {
                        self.tree_loading = false;
                        self.clear_tree_pending_for(&fleet, branch);
                    }
                }
                cx.notify();
            }
            FleetsMsg::Jobs { result } => {
                match result {
                    Ok(j) => {
                        self.set_jobs(j.jobs);
                        let drop_sel = self
                            .selected_job
                            .as_ref()
                            .is_some_and(|sel| !self.job_index.contains_key(sel));
                        if drop_sel {
                            self.selected_job = None;
                        }
                    }
                    Err(e) => {
                        self.error = Some(format!("JOBS — {e}").into());
                    }
                }
                cx.notify();
            }
            FleetsMsg::GpuSearch { result } => {
                self.gpu_search_loading = false;
                match result {
                    Ok(resp) => {
                        if self.rent_gpu_filter.is_empty() {
                            if let Some(first) = resp.groups.first() {
                                self.rent_gpu_filter = first.gpu_filter.clone();
                            }
                        }
                        self.gpu_offers = Some(resp);
                    }
                    Err(e) => {
                        self.gpu_offers = None;
                        self.action_msg = Some(format!("GPU SEARCH — {e}").into());
                    }
                }
                cx.notify();
            }
            FleetsMsg::Action { action, result } => match result {
                Ok(v) => {
                    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
                        self.action_msg = Some(format!("{action} — {err}").into());
                    } else {
                        self.action_msg = Some(format!("{action} OK").into());
                        if action == "rentgpus" {
                            self.handle_rent_ok(&v, cx);
                        }
                        if action == "newfleet" {
                            if let Some(id) = v.get("id").and_then(|i| i.as_str()) {
                                if !id.is_empty() {
                                    self.selected_fleet = id.to_string();
                                    save_selected_fleet(id);
                                    self.selected_node = None;
                                    self.selected_job = None;
                                }
                            }
                        }
                        self.refresh_after_action(&action, cx);
                    }
                    cx.notify();
                }
                Err(e) => {
                    self.action_msg = Some(format!("{action} — {e}").into());
                    cx.notify();
                }
            },
            FleetsMsg::BoxProgress { contract, result } => {
                if self.progress_contract.as_deref() != Some(contract.as_str()) {
                    return;
                }
                match result {
                    Ok(resp) => {
                        self.progress = Some(resp.clone());
                        if resp.is_terminal() {
                            self.clear_progress();
                            self.fetch_instances(cx);
                            if !self.selected_fleet.is_empty() {
                                self.fetch_tree_phased(cx);
                            }
                        }
                    }
                    Err(e) => {
                        self.progress = None;
                        self.progress_contract = None;
                        self.action_msg = Some(format!("PROVISION — {e}").into());
                    }
                }
                cx.notify();
            }
            FleetsMsg::RefreshStarted => {
                self.refreshing = true;
                cx.notify();
            }
            FleetsMsg::LiveConnected(connected) => {
                if self.live != connected {
                    self.live = connected;
                    cx.notify();
                }
            }
            FleetsMsg::JobDelta(ev) => {
                if ev.fleet != self.selected_fleet {
                    return;
                }
                match patch_job(&mut self.jobs, &ev) {
                    PatchOutcome::Updated => cx.notify(),
                    PatchOutcome::NotFound => self.fetch_jobs(cx),
                }
            }
            FleetsMsg::BoxDelta(ev) => {
                self.apply_box_delta(&ev, cx);
                cx.notify();
            }
            FleetsMsg::FleetDelta(ev) => {
                // Patch the fleet roster row in place; a miss means a new (or
                // dropped) fleet, so refetch the roster only.
                if self.fleet_index.contains_key(&ev.id) {
                    if let Some(resp) = self.fleets.as_mut() {
                        patch_fleet(resp, &ev);
                    }
                    cx.notify();
                } else {
                    self.refresh_fleets(cx);
                }
            }
            FleetsMsg::NodeDelta(ev) => {
                // Topology nodes are scoped to the visible fleet; ignore deltas
                // for any other fleet's tree.
                if ev.fleet != self.selected_fleet {
                    return;
                }
                // No cached tree yet — kick off a debounced probe to populate it.
                if self.tree.is_none() {
                    self.fetch_tree(true, cx);
                    return;
                }
                let topology = ev.parent.is_some() || ev.children.is_some();
                if let Some(tree) = self.tree.as_mut() {
                    let outcome = patch_tree_node(tree, &ev);
                    match outcome {
                        TreePatchOutcome::Inserted => self.relayout_from_cache(),
                        TreePatchOutcome::Updated if topology => self.relayout_from_cache(),
                        TreePatchOutcome::Updated => {}
                    }
                }
                cx.notify();
            }
        }
    }

    fn apply_tree(&mut self, tree: TreeResp) {
        self.node_index = tree
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.tag.clone(), i))
            .collect();
        self.layout = layout_tree(&tree.nodes, tree.root.as_deref());
        self.fleet_size = tree.fleet;
        self.board_scale = fit_scale(&self.layout);
        let drop_node = self
            .selected_node
            .as_ref()
            .is_some_and(|sel| !self.node_index.contains_key(sel));
        if drop_node {
            self.selected_node = None;
        }
        self.tree = Some(tree);
    }

    pub(crate) fn tree_nodes(&self) -> &[fabric_types::TreeNode] {
        self.tree.as_ref().map(|t| t.nodes.as_slice()).unwrap_or(&[])
    }

    pub(crate) fn unassigned_boxes(&self) -> Vec<Instance> {
        self.instances
            .as_ref()
            .map(|i| {
                i.instances
                    .iter()
                    .filter(|b| {
                        b.fleet_id.is_none()
                            && !is_terminal_box(b)
                            && (b.assignable.unwrap_or(false) || is_in_flight_box(b))
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn assigned_boxes(&self) -> Vec<Instance> {
        let fleet = &self.selected_fleet;
        self.instances
            .as_ref()
            .map(|i| {
                i.instances
                    .iter()
                    .filter(|b| {
                        !is_terminal_box(b)
                            && b.fleet_id.as_deref() == Some(fleet.as_str())
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn instances_summary(&self) -> Option<String> {
        self.instances.as_ref().map(|i| {
            format!("{} boxes · {} unassigned", i.total, i.unassigned)
        })
    }

    fn fleet_rows(&self) -> &[Fleet] {
        self.fleets
            .as_ref()
            .map(|f| f.fleets.as_slice())
            .unwrap_or(&[])
    }
}

impl Render for FleetsView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::get(cx);
        let fleets = self.fleet_rows().to_vec();
        let selected = self.selected_fleet.clone();
        let err = self.error.clone();
        let action = self.action_msg.clone();
        let tree_loading = self.tree_loading;
        let progress = self.progress.clone();
        let operator_email = self.operator_email.clone();
        let status_left = action
            .as_ref()
            .map(|m| SharedString::from(format!("OPS │ {m}")))
            .or_else(|| {
                self.instances_summary()
                    .map(SharedString::from)
            })
            .or_else(|| {
                if self.refreshing || tree_loading {
                    Some(SharedString::from("loading…"))
                } else {
                    None
                }
            });

        div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .when_some(err, |el, e| {
                el.child(
                    div()
                        .flex_none()
                        .px(px(8.))
                        .py(px(4.))
                        .bg(gpui::rgb(0x180000))
                        .text_color(theme.warn)
                        .child(format!("■ {e}")),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(roster_panel(cx, &theme, &fleets, &selected, tree_loading))
                    .child(fleet_board(self, &theme, cx))
                    .child(ops_rail(self, &theme, cx)),
            )
            .when_some(progress, |el, p| {
                el.child(progress_bar(&theme, &p))
            })
            .child(theme.status_bar(
                status_left.unwrap_or_else(|| SharedString::from("")),
                operator_email.map(SharedString::from),
            ))
    }
}

/// Build a provisioning chip from one entry of a `rentgpus` `pending` array.
fn insert_pending_box_id(resp: &mut InstancesResp, id: &str, gpu_name: &str, num_gpus: i64) {
    if resp.instances.iter().any(|i| i.id_str() == id) {
        return;
    }
    resp.instances.push(Instance {
        id: serde_json::Value::String(id.into()),
        label: Some("GPU (provisioning)".to_string()),
        provider: "nebius".into(),
        gpu_name: Some(gpu_name.into()),
        num_gpus: Some(num_gpus),
        status: Some("provisioning".into()),
        provision_state: Some("provisioning".into()),
        assignable: Some(false),
        ..Default::default()
    });
    resp.total = resp.instances.len() as i64;
    resp.unassigned = resp
        .instances
        .iter()
        .filter(|i| i.fleet_id.is_none())
        .count() as i64;
}

/// Build a provisioning chip from a JSON object in the `pending` array.
fn insert_pending_box(
    resp: &mut InstancesResp,
    entry: &serde_json::Value,
    gpu_name: &str,
    num_gpus: i64,
) {
    let id = entry
        .get("id")
        .or_else(|| entry.get("contract"))
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if id.is_null() {
        return;
    }
    let id_str = match &id {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        other => other.to_string(),
    };
    if resp.instances.iter().any(|i| i.id_str() == id_str) {
        return;
    }
    let gpu_name = entry
        .get("gpu_name")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| gpu_name.to_string());
    let num_gpus = entry
        .get("num_gpus")
        .or_else(|| entry.get("gpus"))
        .and_then(|v| v.as_i64())
        .unwrap_or(num_gpus);
    let label = entry
        .get("label")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let fleet_id = entry
        .get("fleet_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let provision_state = entry
        .get("provision_state")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| Some("provisioning".into()));

    resp.instances.push(Instance {
        id,
        label,
        provider: "nebius".into(),
        gpu_name: Some(gpu_name),
        num_gpus: Some(num_gpus),
        status: Some("provisioning".into()),
        fleet_id,
        provision_state,
        assignable: Some(false),
        ..Default::default()
    });
    resp.total = resp.instances.len() as i64;
    resp.unassigned = resp
        .instances
        .iter()
        .filter(|i| i.fleet_id.is_none())
        .count() as i64;
}

fn is_in_flight_box(inst: &Instance) -> bool {
    matches!(
        inst.provision_state.as_deref(),
        Some("booting" | "provisioning" | "creating")
    ) || matches!(inst.status.as_deref(), Some("booting" | "provisioning" | "creating"))
}

fn is_terminal_box(inst: &Instance) -> bool {
    inst.provision_state.as_deref() == Some("destroyed")
}

fn progress_bar(theme: &Theme, progress: &BoxProgressResp) -> impl IntoElement {
    let state = progress
        .state
        .as_deref()
        .or(progress.message.as_deref())
        .unwrap_or("provisioning");
    let tail = progress
        .lines
        .last()
        .cloned()
        .unwrap_or_else(|| state.to_string());

    div()
        .flex_none()
        .px(px(8.))
        .py(px(4.))
        .bg(theme.panel_edge)
        .border_t_1()
        .border_color(theme.border)
        .flex()
        .items_center()
        .gap_2()
        .child(
            div()
                .flex_none()
                .text_size(px(10.))
                .text_color(theme.amber)
                .child("PROVISION"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(10.))
                .text_color(theme.text_dim)
                .child(tail),
        )
}

fn roster_panel(
    cx: &mut Context<FleetsView>,
    theme: &Theme,
    fleets: &[Fleet],
    selected: &str,
    tree_loading: bool,
) -> impl IntoElement {
    let fleet_items: Vec<_> = fleets
        .iter()
        .map(|f| {
            let id = f.id.clone();
            let active = f.id == selected;
            let dot = status_dot_color(theme, &f.status);
            div()
                .id(SharedString::from(format!("fleet-{id}")))
                .w_full()
                .px(px(8.))
                .py(px(5.))
                .flex()
                .items_center()
                .gap_2()
                .bg(if active { theme.panel_edge } else { theme.row_a })
                .border_b_1()
                .border_color(theme.border)
                .cursor_pointer()
                .hover(|s| s.bg(theme.panel_edge))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| this.select_fleet(id.clone(), cx)),
                )
                .child(div().flex_none().w(px(6.)).h(px(6.)).bg(dot))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(if active { theme.amber } else { theme.data })
                        .child(f.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(9.))
                        .text_color(theme.text_dim)
                        .child(format!("{}", f.n_pods)),
                )
        })
        .collect();

    div()
        .flex_none()
        .w(px(160.))
        .min_h_0()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme.border)
        .bg(theme.bg)
        .child(
            div()
                .flex_none()
                .px(px(8.))
                .py(px(4.))
                .bg(theme.panel_edge)
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(10.))
                .text_color(theme.amber)
                .child(if tree_loading {
                    format!("FLEETS ({}) …", fleets.len())
                } else {
                    format!("FLEETS ({})", fleets.len())
                }),
        )
        .child(
            div()
                .id("fleet-roster")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .children(fleet_items),
        )
}

pub(crate) fn status_dot_color(theme: &Theme, status: &str) -> gpui::Rgba {
    match status {
        "running" | "ready" | "assigned" => theme.live,
        "preparing" | "starting" | "booting" | "provisioning" => theme.amber,
        "error" => theme.warn,
        _ => theme.idle,
    }
}

fn chrono_timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn load_selected_fleet() -> String {
    std::env::var("HOME")
        .ok()
        .map(|h| {
            std::fs::read_to_string(format!("{h}/.config/fabric/fabricApp.fleet.json"))
                .ok()
                .map(|s| s.trim().trim_matches('"').to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default()
}

fn save_selected_fleet(id: &str) {
    if let Ok(home) = std::env::var("HOME") {
        let dir = format!("{home}/.config/fabric");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(format!("{dir}/fabricApp.fleet.json"), format!("\"{id}\""));
    }
}
