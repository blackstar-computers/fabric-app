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
use fabric_types::{BoxProgressResp, Fleet, FleetsResp, Instance, InstancesResp, Job, TreeResp};
use futures::channel::mpsc::UnboundedSender;
use gpui::{div, prelude::*, px, Context, MouseButton, Render, SharedString, Window};
use serde_json::json;
use std::collections::HashSet;
use std::time::{Duration, Instant};

const DEFAULT_BRANCH: u32 = 8;
/// Coalesce job SSE bursts — mirrors dashboard [`LIVE_NOTIFY_MIN`].
const JOB_LIVE_MIN: Duration = Duration::from_millis(150);

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

    last_job_live_refresh: Option<Instant>,
    tree_pending: HashSet<TreeFetchKey>,
    /// Remaining tree responses expected from an in-flight phased fetch (2 → 0).
    tree_phased_remaining: u8,
    pub progress: Option<BoxProgressResp>,
    progress_contract: Option<String>,
    operator_email: Option<String>,
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
            last_job_live_refresh: None,
            tree_pending: HashSet::new(),
            tree_phased_remaining: 0,
            progress: None,
            progress_contract: None,
            operator_email: None,
        }
    }

    pub fn attach(&mut self, cmd_tx: UnboundedSender<NetworkCommand>) {
        self.cmd_tx = Some(cmd_tx);
    }

    pub fn set_operator_email(&mut self, email: Option<String>) {
        self.operator_email = email;
    }

    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        self.refreshing = true;
        self.refresh_deck(true, cx);
    }

    pub fn on_visible(&mut self, cx: &mut Context<Self>) {
        if self.fleets.is_none() {
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

    pub fn refresh_tree(&mut self, cx: &mut Context<Self>) {
        let fleet = self.selected_fleet.clone();
        let branch = self.branch;
        self.clear_tree_pending_for(&fleet, branch);
        self.fetch_tree(true, cx);
        self.action_msg = Some("REFRESH TREE…".into());
        cx.notify();
    }

    fn flush_job_live(&mut self, cx: &mut Context<Self>) {
        self.last_job_live_refresh = Some(Instant::now());
        self.refresh_deck(false, cx);
    }

    fn on_job_live(&mut self, cx: &mut Context<Self>) {
        let now = Instant::now();
        if self
            .last_job_live_refresh
            .is_some_and(|t| now.duration_since(t) < JOB_LIVE_MIN)
        {
            return;
        }
        self.flush_job_live(cx);
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
                self.fleets = Some(resp);
                cx.notify();
            }
            FleetsMsg::Fleets(Err(e)) => {
                self.refreshing = false;
                self.error = Some(format!("FLEETS — {e}").into());
                cx.notify();
            }
            FleetsMsg::Instances(Ok(resp)) => {
                self.instances = Some(resp);
                cx.notify();
            }
            FleetsMsg::Instances(Err(e)) => {
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
                if let Ok(j) = result {
                    self.jobs = j.jobs;
                    if let Some(sel) = &self.selected_job {
                        if !self.jobs.iter().any(|j| &j.job_id == sel) {
                            self.selected_job = None;
                        }
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
                        self.refresh_deck(true, cx);
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
                            self.refresh_deck(true, cx);
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
            FleetsMsg::JobLive => {
                self.on_job_live(cx);
            }
        }
    }

    fn apply_tree(&mut self, tree: TreeResp) {
        self.layout = layout_tree(&tree.nodes, tree.root.as_deref());
        self.fleet_size = tree.fleet;
        self.board_scale = fit_scale(&self.layout);
        if let Some(sel) = &self.selected_node {
            if !tree.nodes.iter().any(|n| &n.tag == sel) {
                self.selected_node = None;
            }
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
                    .filter(|b| b.fleet_id.is_none() && b.assignable.unwrap_or(false))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
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
            .map(|m| SharedString::from(format!("OPS │ {m}")));
        let show_status = status_left.is_some() || operator_email.is_some();

        div()
            .flex_1()
            .min_h_0()
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
            .when(show_status, |el| {
                el.child(theme.status_bar(
                    status_left.unwrap_or_else(|| SharedString::from("")),
                    operator_email.map(SharedString::from),
                ))
            })
    }
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
