//! UI → network command channel + background fetch/SSE loop.
//!
//! HTTP runs on the dedicated Tokio runtime (`fabric_api::spawn_network`), never on
//! the GPUI thread. Auth is in [`crate::auth`]. Results post back through `AppUiMsg` and
//! a single UI bridge in [`crate::app::FabricApp`].

use crate::detail::SERIES_MAX_POINTS;
use fabric_api::{spawn_network, Client, ClientError};
use fabric_live::{run_sse_loop, LiveMessage, SseLoopExit};
use fabric_types::{
    BoxProgressResp, CheckpointsResp, FleetsResp, GpuSearchResp, InstancesResp, JobsResp,
    RunSeries, RunsSummary, SseBoxEvent, SseFleetEvent, SseJobEvent, SseNodeEvent,
    TopoManifestResp, TreeResp, VizGalleryResp, VizOpenRequest, VizStatusResp, VizStepRequest,
};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// Table sparklines only need a short tail — display downsamples to 56 vertices.
pub const SPARKLINE_MAX_POINTS: u32 = 64;

const PROGRESS_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_PROGRESS_POLLS: u32 = 120;

const RECONCILE_POLL_INTERVAL: Duration = Duration::from_secs(3);
const MAX_RECONCILE_POLLS: u32 = 60;

const VIZ_POLL_INTERVAL: Duration = Duration::from_millis(1500);
const MAX_VIZ_POLLS: u32 = 240;

/// Commands from GPUI views to the Tokio network task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkCommand {
    RefreshSummary,
    FetchSeries { pod: String, name: String },
    FetchSparkline { pod: String, name: String },
    /// Parallel fetch: fleets + instances + jobs + tree skeleton.
    RefreshFleetDeck {
        fleet: String,
        branch: u32,
        probe_tree_after: bool,
    },
    /// Single-resource refresh (kept for granular invalidation).
    RefreshFleets,
    RefreshInstances,
    FetchTree {
        fleet: String,
        branch: u32,
        probe: bool,
    },
    /// Fast cached tree, then live probed tree (sequential, not duplicate parallel).
    FetchTreePhased { fleet: String, branch: u32 },
    FetchJobs { fleet: String },
    FleetAction { action: String, payload: Value },
    /// Nebius GPU platform search for the rent flow.
    FetchGpuSearch { num_gpus: u32 },
    /// Poll provisioning progress until terminal or cap.
    PollBoxProgress { contract: String },
    /// Poll the box roster while rent/provision boxes are still in-flight, so
    /// locally-inserted pending chips reconcile with the portal's real rows.
    PollInstancesReconcile,
    /// Parallel fetch: runs summary + checkpoints + topo manifest.
    TopologyRefreshDeck,
    TopologyVizOpen {
        fleet: String,
        pod: String,
        run: String,
        file: String,
        force: bool,
    },
    TopologyPollVizReady { ckpt: String },
    TopologyFetchVizState,
    TopologyVizStep { body: Value },
    /// Zero daemon substrate state when the input drive changes (before the next RUN).
    TopologyVizReset,
    TopologyFetchTopoFab { run: String, file: String },
    TopologyFetchGallery {
        dataset: String,
        start: u32,
        count: u32,
        size: u32,
    },
    /// Single dataset image when the gallery batch does not cover the active idx.
    TopologyFetchInputImage {
        dataset: String,
        idx: u32,
        size: u32,
    },
}

#[derive(Debug)]
pub enum DashboardMsg {
    Summary(Result<RunsSummary, ClientError>),
    Live(LiveMessage),
    RefreshStarted,
    Series {
        pod: String,
        name: String,
        result: Result<RunSeries, ClientError>,
    },
    Sparkline {
        pod: String,
        name: String,
        result: Result<RunSeries, ClientError>,
    },
}

#[derive(Debug)]
pub enum FleetsMsg {
    Fleets(Result<FleetsResp, ClientError>),
    Instances {
        result: Result<InstancesResp, ClientError>,
        /// When true, merge in-flight rows from the cached roster that the portal
        /// has not returned yet (reconcile poll after rent / SSE booting rows).
        reconcile: bool,
    },
    Tree {
        result: Result<TreeResp, ClientError>,
    },
    Jobs {
        result: Result<JobsResp, ClientError>,
    },
    Action {
        action: String,
        result: Result<Value, ClientError>,
    },
    BoxProgress {
        contract: String,
        result: Result<BoxProgressResp, ClientError>,
    },
    RefreshStarted,
    /// Live job delta from the portal SSE stream, patched in place by `job_id`.
    JobDelta(SseJobEvent),
    /// SSE stream connection state (connected/disconnected) for the live pill.
    LiveConnected(bool),
    GpuSearch {
        result: Result<GpuSearchResp, ClientError>,
    },
    /// Live box/instance delta from the portal SSE stream.
    BoxDelta(SseBoxEvent),
    /// Live fleet delta from the portal SSE stream.
    FleetDelta(SseFleetEvent),
    /// Live topology-node delta from the portal SSE stream.
    NodeDelta(SseNodeEvent),
}

#[derive(Debug)]
pub enum TopologyMsg {
    Deck {
        summary: Result<RunsSummary, ClientError>,
        checkpoints: Result<CheckpointsResp, ClientError>,
        manifest: Result<TopoManifestResp, ClientError>,
    },
    VizOpen(Result<Value, ClientError>),
    VizReady(Result<VizStatusResp, ClientError>),
    VizState(Result<Value, ClientError>),
    VizStep(Result<Value, ClientError>),
    TopoFab {
        run: String,
        file: String,
        result: Result<Vec<u8>, ClientError>,
    },
    Gallery(Result<VizGalleryResp, ClientError>),
    InputImage {
        idx: u32,
        result: Result<Vec<u8>, ClientError>,
    },
    RefreshStarted,
}

#[derive(Debug)]
pub enum AppUiMsg {
    Dashboard(DashboardMsg),
    Fleets(FleetsMsg),
    Topology(TopologyMsg),
    Auth(crate::auth::AuthMsg),
    /// Session or service token rejected by the portal — return to login.
    Unauthorized(String),
}

/// Handle for stopping background fetch/SSE tasks on sign-out or auth failure.
pub struct NetworkHandle {
    shutdown: Arc<AtomicBool>,
    cmd_tx: UnboundedSender<NetworkCommand>,
}

impl NetworkHandle {
    pub fn new(
        shutdown: Arc<AtomicBool>,
        cmd_tx: UnboundedSender<NetworkCommand>,
    ) -> Self {
        Self { shutdown, cmd_tx }
    }

    pub fn cmd(&self) -> UnboundedSender<NetworkCommand> {
        self.cmd_tx.clone()
    }

    pub fn stop(&self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

fn post_unauthorized(ui_tx: &UnboundedSender<AppUiMsg>, err: &ClientError) -> bool {
    if err.is_unauthorized() {
        let _ = ui_tx.unbounded_send(AppUiMsg::Unauthorized(
            "Session expired — sign in again".into(),
        ));
        true
    } else {
        false
    }
}

fn send_dashboard(ui_tx: &UnboundedSender<AppUiMsg>, msg: DashboardMsg) {
    if let DashboardMsg::Summary(Err(ref e)) = msg {
        if post_unauthorized(ui_tx, e) {
            return;
        }
    }
    if let DashboardMsg::Series { result: Err(ref e), .. } = msg {
        if post_unauthorized(ui_tx, e) {
            return;
        }
    }
    let _ = ui_tx.unbounded_send(AppUiMsg::Dashboard(msg));
}

fn send_fleets(ui_tx: &UnboundedSender<AppUiMsg>, msg: FleetsMsg) {
    let unauthorized = match &msg {
        FleetsMsg::Fleets(Err(e))
        | FleetsMsg::Instances { result: Err(e), .. }
        | FleetsMsg::Tree { result: Err(e) }
        | FleetsMsg::Jobs { result: Err(e) }
        | FleetsMsg::Action { result: Err(e), .. }
        | FleetsMsg::BoxProgress { result: Err(e), .. } => post_unauthorized(ui_tx, e),
        _ => false,
    };
    if unauthorized {
        return;
    }
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(msg));
}

fn send_topology(ui_tx: &UnboundedSender<AppUiMsg>, msg: TopologyMsg) {
    let unauthorized = match &msg {
        TopologyMsg::Deck {
            summary: Err(e), ..
        }
        | TopologyMsg::VizOpen(Err(e))
        | TopologyMsg::VizReady(Err(e))
        | TopologyMsg::VizState(Err(e))
        | TopologyMsg::VizStep(Err(e))
        | TopologyMsg::TopoFab { result: Err(e), .. }
        | TopologyMsg::Gallery(Err(e))
        | TopologyMsg::InputImage { result: Err(e), .. } => post_unauthorized(ui_tx, e),
        _ => false,
    };
    if unauthorized {
        return;
    }
    let _ = ui_tx.unbounded_send(AppUiMsg::Topology(msg));
}

pub fn spawn_app_network(
    client: Client,
    ui_tx: UnboundedSender<AppUiMsg>,
    mut cmd_rx: UnboundedReceiver<NetworkCommand>,
    shutdown: Arc<AtomicBool>,
) {
    spawn_network(async move {
        let sse_client = client.clone();
        let sse_tx = ui_tx.clone();
        let sse_shutdown = shutdown.clone();
        spawn_network(async move {
            let exit = run_sse_loop(
                sse_client,
                |msg| {
                    match &msg {
                        LiveMessage::Connected => {
                            let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(
                                FleetsMsg::LiveConnected(true),
                            ));
                        }
                        LiveMessage::Disconnected => {
                            let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(
                                FleetsMsg::LiveConnected(false),
                            ));
                        }
                        LiveMessage::JobEvent(ev) => {
                            let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(
                                FleetsMsg::JobDelta(ev.clone()),
                            ));
                        }
                        LiveMessage::BoxEvent(ev) => {
                            let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(
                                FleetsMsg::BoxDelta(ev.clone()),
                            ));
                        }
                        LiveMessage::FleetEvent(ev) => {
                            let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(
                                FleetsMsg::FleetDelta(ev.clone()),
                            ));
                        }
                        LiveMessage::NodeEvent(ev) => {
                            let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(
                                FleetsMsg::NodeDelta(ev.clone()),
                            ));
                        }
                        _ => {}
                    }
                    let _ = sse_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::Live(msg)));
                },
                || !sse_shutdown.load(Ordering::Relaxed),
            )
            .await;
            if exit == SseLoopExit::Unauthorized {
                let _ = sse_tx.unbounded_send(AppUiMsg::Unauthorized(
                    "Session expired — sign in again".into(),
                ));
            }
        });

        fetch_summary(&client, &ui_tx).await;
        fetch_fleets(&client, &ui_tx).await;

        while let Some(cmd) = cmd_rx.next().await {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            match cmd {
                NetworkCommand::RefreshSummary => {
                    let _ = ui_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::RefreshStarted));
                    fetch_summary(&client, &ui_tx).await;
                }
                NetworkCommand::FetchSeries { pod, name } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_series(&client, &ui_tx, &pod, &name).await;
                    });
                }
                NetworkCommand::FetchSparkline { pod, name } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_sparkline(&client, &ui_tx, &pod, &name).await;
                    });
                }
                NetworkCommand::RefreshFleetDeck {
                    fleet,
                    branch,
                    probe_tree_after,
                } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        refresh_fleet_deck(&client, &ui_tx, &fleet, branch).await;
                        if probe_tree_after {
                            spawn_tree_probe(&client, &ui_tx, &shutdown, fleet, branch);
                        }
                    });
                }
                NetworkCommand::RefreshFleets => {
                    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::RefreshStarted));
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_fleets(&client, &ui_tx).await;
                    });
                }
                NetworkCommand::RefreshInstances => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_instances(&client, &ui_tx).await;
                    });
                }
                NetworkCommand::FetchTree {
                    fleet,
                    branch,
                    probe,
                } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_tree(&client, &ui_tx, &fleet, branch, probe).await;
                    });
                }
                NetworkCommand::FetchTreePhased { fleet, branch } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_tree_phased(&client, &ui_tx, &fleet, branch).await;
                    });
                }
                NetworkCommand::FetchJobs { fleet } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_jobs(&client, &ui_tx, &fleet).await;
                    });
                }
                NetworkCommand::FleetAction { action, payload } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fleet_action(&client, &ui_tx, &action, payload).await;
                    });
                }
                NetworkCommand::FetchGpuSearch { num_gpus } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_gpu_search(&client, &ui_tx, num_gpus).await;
                    });
                }
                NetworkCommand::PollBoxProgress { contract } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        poll_box_progress(&client, &ui_tx, &shutdown, &contract).await;
                    });
                }
                NetworkCommand::PollInstancesReconcile => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        poll_instances_reconcile(&client, &ui_tx, &shutdown).await;
                    });
                }
                NetworkCommand::TopologyRefreshDeck => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        refresh_topology_deck(&client, &ui_tx).await;
                    });
                }
                NetworkCommand::TopologyVizOpen {
                    fleet,
                    pod,
                    run,
                    file,
                    force,
                } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        topology_viz_open(&client, &ui_tx, &fleet, &pod, &run, &file, force).await;
                    });
                }
                NetworkCommand::TopologyPollVizReady { ckpt } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        poll_viz_ready(&client, &ui_tx, &shutdown, &ckpt).await;
                    });
                }
                NetworkCommand::TopologyFetchVizState => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_viz_state(&client, &ui_tx).await;
                    });
                }
                NetworkCommand::TopologyVizStep { body } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        viz_step(&client, &ui_tx, body).await;
                    });
                }
                NetworkCommand::TopologyVizReset => {
                    let client = client.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        let _ = client.viz_reset().await;
                    });
                }
                NetworkCommand::TopologyFetchTopoFab { run, file } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_topo_fab(&client, &ui_tx, &run, &file).await;
                    });
                }
                NetworkCommand::TopologyFetchGallery {
                    dataset,
                    start,
                    count,
                    size,
                } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_viz_gallery(&client, &ui_tx, &dataset, start, count, size).await;
                    });
                }
                NetworkCommand::TopologyFetchInputImage {
                    dataset,
                    idx,
                    size,
                } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    let shutdown = shutdown.clone();
                    spawn_network(async move {
                        if shutdown.load(Ordering::Relaxed) {
                            return;
                        }
                        fetch_viz_input_image(&client, &ui_tx, &dataset, idx, size).await;
                    });
                }
            }
        }
    });
}

async fn refresh_fleet_deck(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    fleet: &str,
    branch: u32,
) {
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::RefreshStarted));

    let (fleets_r, instances_r, jobs_r, tree_r) = tokio::join!(
        client.fetch_fleets(),
        client.fetch_instances(),
        client.fetch_jobs(fleet),
        client.fetch_tree(branch, fleet, false),
    );

    send_fleets(ui_tx, FleetsMsg::Fleets(fleets_r));
    send_fleets(
        ui_tx,
        FleetsMsg::Instances {
            result: instances_r,
            reconcile: false,
        },
    );
    send_fleets(ui_tx, FleetsMsg::Jobs { result: jobs_r });
    send_fleets(ui_tx, FleetsMsg::Tree { result: tree_r });
}

/// Spawn the live-probed tree fetch on its own task so the deck batch above is
/// never blocked waiting on the (slower) probe round-trip.
fn spawn_tree_probe(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    shutdown: &Arc<AtomicBool>,
    fleet: String,
    branch: u32,
) {
    if fleet.is_empty() {
        return;
    }
    let client = client.clone();
    let ui_tx = ui_tx.clone();
    let shutdown = shutdown.clone();
    spawn_network(async move {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        fetch_tree(&client, &ui_tx, &fleet, branch, true).await;
    });
}

async fn fetch_tree_phased(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    fleet: &str,
    branch: u32,
) {
    let skeleton = client.fetch_tree(branch, fleet, false).await;
    send_fleets(ui_tx, FleetsMsg::Tree { result: skeleton });

    if fleet.is_empty() {
        return;
    }
    let probed = client.fetch_tree(branch, fleet, true).await;
    send_fleets(ui_tx, FleetsMsg::Tree { result: probed });
}

async fn poll_box_progress(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    shutdown: &Arc<AtomicBool>,
    contract: &str,
) {
    for attempt in 0..MAX_PROGRESS_POLLS {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        let result = client.fetch_box_progress(contract).await;
        if let Some(err) = result.as_ref().err() {
            if err.is_unauthorized() {
                post_unauthorized(ui_tx, err);
                return;
            }
        }
        let terminal = result
            .as_ref()
            .ok()
            .is_some_and(|r| r.is_terminal());
        send_fleets(
            ui_tx,
            FleetsMsg::BoxProgress {
                contract: contract.to_string(),
                result,
            },
        );
        if terminal {
            return;
        }
        if attempt + 1 < MAX_PROGRESS_POLLS {
            tokio::time::sleep(PROGRESS_POLL_INTERVAL).await;
        }
    }
}

/// Refetch the box roster on a fixed cadence so locally-inserted pending chips
/// (from a `rentgpus` response) reconcile with the portal's authoritative rows.
///
/// Stops early once the portal has surfaced the in-flight boxes and they have
/// all settled (no row still booting/provisioning), otherwise caps at
/// [`MAX_RECONCILE_POLLS`] to bound the background work.
async fn poll_instances_reconcile(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    shutdown: &Arc<AtomicBool>,
) {
    let mut seen_in_flight = false;
    for attempt in 0..MAX_RECONCILE_POLLS {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        let result = client.fetch_instances().await;
        if let Some(err) = result.as_ref().err() {
            if err.is_unauthorized() {
                post_unauthorized(ui_tx, err);
                return;
            }
        }
        let in_flight = result.as_ref().ok().is_some_and(instances_in_flight);
        seen_in_flight |= in_flight;
        send_fleets(
            ui_tx,
            FleetsMsg::Instances {
                result,
                reconcile: true,
            },
        );
        // Once the portal has shown the in-flight boxes and they have settled,
        // there is nothing left to reconcile.
        if seen_in_flight && !in_flight {
            return;
        }
        if attempt + 1 < MAX_RECONCILE_POLLS {
            tokio::time::sleep(RECONCILE_POLL_INTERVAL).await;
        }
    }
}

fn instances_in_flight(resp: &InstancesResp) -> bool {
    resp.instances.iter().any(|i| {
        matches!(
            i.provision_state.as_deref(),
            Some("booting" | "provisioning" | "creating" | "pending")
        ) || matches!(
            i.status.as_deref(),
            Some("booting" | "provisioning" | "creating")
        )
    })
}

async fn fetch_summary(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    info!(portal = %client.base_url(), "fetching runs summary");
    let summary = client.fetch_runs_summary().await;
    match &summary {
        Ok(s) => info!(runs = s.runs.len(), "runs summary fetched"),
        Err(e) => warn!("runs summary fetch failed: {e}"),
    }
    send_dashboard(ui_tx, DashboardMsg::Summary(summary));
}

async fn fetch_fleets(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    let result = client.fetch_fleets().await;
    send_fleets(ui_tx, FleetsMsg::Fleets(result));
}

async fn fetch_instances(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    let result = client.fetch_instances().await;
    send_fleets(
        ui_tx,
        FleetsMsg::Instances {
            result,
            reconcile: false,
        },
    );
}

async fn fetch_tree(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    fleet: &str,
    branch: u32,
    probe: bool,
) {
    let result = client.fetch_tree(branch, fleet, probe).await;
    send_fleets(ui_tx, FleetsMsg::Tree { result });
}

async fn fetch_jobs(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>, fleet: &str) {
    let result = client.fetch_jobs(fleet).await;
    send_fleets(ui_tx, FleetsMsg::Jobs { result });
}

async fn fetch_gpu_search(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    num_gpus: u32,
) {
    let result = client.fetch_gpu_search(num_gpus).await;
    send_fleets(ui_tx, FleetsMsg::GpuSearch { result });
}

async fn fleet_action(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    action: &str,
    payload: Value,
) {
    let result = client.fleet_action(action, payload).await;
    send_fleets(
        ui_tx,
        FleetsMsg::Action {
            action: action.to_string(),
            result,
        },
    );
}

async fn fetch_series(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    pod: &str,
    name: &str,
) {
    info!(pod, name, "fetching run series");
    let result = client
        .fetch_run_series(pod, name, SERIES_MAX_POINTS)
        .await;
    send_dashboard(
        ui_tx,
        DashboardMsg::Series {
            pod: pod.to_string(),
            name: name.to_string(),
            result,
        },
    );
}

async fn fetch_sparkline(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    pod: &str,
    name: &str,
) {
    let result = client
        .fetch_run_series(pod, name, SPARKLINE_MAX_POINTS)
        .await;
    send_dashboard(
        ui_tx,
        DashboardMsg::Sparkline {
            pod: pod.to_string(),
            name: name.to_string(),
            result,
        },
    );
}

async fn refresh_topology_deck(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    let _ = ui_tx.unbounded_send(AppUiMsg::Topology(TopologyMsg::RefreshStarted));

    let (summary_r, checkpoints_r, manifest_r) = tokio::join!(
        client.fetch_runs_summary(),
        client.fetch_checkpoints(""),
        client.fetch_topo_manifest(),
    );

    send_topology(
        ui_tx,
        TopologyMsg::Deck {
            summary: summary_r,
            checkpoints: checkpoints_r,
            manifest: manifest_r,
        },
    );
}

async fn topology_viz_open(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    fleet: &str,
    pod: &str,
    run: &str,
    file: &str,
    force: bool,
) {
    let body = VizOpenRequest {
        fleet: fleet.to_string(),
        pod: pod.to_string(),
        run: run.to_string(),
        file: file.to_string(),
        background: true,
        force,
    };
    let result = client
        .viz_open(&body)
        .await
        .map(|resp| serde_json::to_value(resp).unwrap_or(Value::Null));
    send_topology(ui_tx, TopologyMsg::VizOpen(result));
}

async fn poll_viz_ready(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    shutdown: &Arc<AtomicBool>,
    ckpt: &str,
) {
    for attempt in 0..MAX_VIZ_POLLS {
        if shutdown.load(Ordering::Relaxed) {
            return;
        }
        let result = client.viz_status(ckpt).await;
        if let Some(err) = result.as_ref().err() {
            if err.is_unauthorized() {
                post_unauthorized(ui_tx, err);
                return;
            }
        }
        let ready = result
            .as_ref()
            .ok()
            .is_some_and(viz_status_ready);
        let failed = result
            .as_ref()
            .ok()
            .and_then(viz_status_error)
            .is_some();
        if ready || failed {
            send_topology(ui_tx, TopologyMsg::VizReady(result));
            return;
        }
        if attempt + 1 < MAX_VIZ_POLLS {
            tokio::time::sleep(VIZ_POLL_INTERVAL).await;
        }
    }
    send_topology(
        ui_tx,
        TopologyMsg::VizReady(Err(ClientError::bad_request(
            "viz load timed out after ~6 minutes",
        ))),
    );
}

pub(crate) fn viz_status_ready(status: &VizStatusResp) -> bool {
    status.ready.unwrap_or(false) || status.state.as_deref() == Some("ready")
}

pub(crate) fn viz_status_error(status: &VizStatusResp) -> Option<String> {
    if status.state.as_deref() == Some("error") {
        return status.error.clone();
    }
    status.error.clone()
}

async fn fetch_viz_state(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    let result = client
        .viz_state()
        .await
        .map(|meta| serde_json::to_value(meta).unwrap_or(Value::Null));
    send_topology(ui_tx, TopologyMsg::VizState(result));
}

async fn viz_step(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>, body: Value) {
    let result = match serde_json::from_value::<VizStepRequest>(body) {
        Ok(req) => {
            if let Err(e) = client.viz_reset().await {
                Err(e)
            } else {
                client.viz_step(&req).await
            }
        }
        Err(e) => Err(ClientError::Json(e)),
    };
    send_topology(ui_tx, TopologyMsg::VizStep(result));
}

async fn fetch_topo_fab(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    run: &str,
    file: &str,
) {
    let path = format!(
        "/api/topology/file?run={}&file={}",
        url_encode(run),
        url_encode(file)
    );
    let result = client.fetch_bytes(&path).await;
    send_topology(
        ui_tx,
        TopologyMsg::TopoFab {
            run: run.to_string(),
            file: file.to_string(),
            result,
        },
    );
}

async fn fetch_viz_gallery(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    dataset: &str,
    start: u32,
    count: u32,
    size: u32,
) {
    let result = client.fetch_viz_gallery(dataset, start, count, size).await;
    send_topology(ui_tx, TopologyMsg::Gallery(result));
}

async fn fetch_viz_input_image(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    dataset: &str,
    idx: u32,
    size: u32,
) {
    let result = client.fetch_viz_image_bytes(dataset, idx, size).await;
    send_topology(
        ui_tx,
        TopologyMsg::InputImage {
            idx,
            result,
        },
    );
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
