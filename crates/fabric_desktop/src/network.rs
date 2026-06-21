//! UI → network command channel + background fetch/SSE loop.
//!
//! HTTP runs on the dedicated Tokio runtime (`fabric_api::spawn_network`), never on
//! the GPUI thread. Auth is in [`crate::auth`]. Results post back through `AppUiMsg` and
//! a single UI bridge in [`crate::app::FabricApp`].

use crate::detail::SERIES_MAX_POINTS;
use fabric_api::{spawn_network, Client, ClientError};
use fabric_live::{run_sse_loop, LiveMessage};
use fabric_types::{
    BoxProgressResp, FleetsResp, InstancesResp, JobsResp, RunSeries, RunsSummary, TreeResp,
};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use serde_json::Value;
use std::time::Duration;
use tracing::{info, warn};

/// Table sparklines only need a short tail — display downsamples to 56 vertices.
pub const SPARKLINE_MAX_POINTS: u32 = 64;

const PROGRESS_POLL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_PROGRESS_POLLS: u32 = 120;

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
    #[allow(dead_code)]
    RefreshFleets,
    #[allow(dead_code)]
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
    /// Poll provisioning progress until terminal or cap.
    PollBoxProgress { contract: String },
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
    Instances(Result<InstancesResp, ClientError>),
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
    JobLive,
}

#[derive(Debug)]
pub enum AppUiMsg {
    Dashboard(DashboardMsg),
    Fleets(FleetsMsg),
    Auth(crate::auth::AuthMsg),
}

pub fn spawn_app_network(
    client: Client,
    ui_tx: UnboundedSender<AppUiMsg>,
    mut cmd_rx: UnboundedReceiver<NetworkCommand>,
) {
    spawn_network(async move {
        let sse_client = client.clone();
        let sse_tx = ui_tx.clone();
        spawn_network(async move {
            run_sse_loop(sse_client, |msg| {
                match &msg {
                    LiveMessage::JobEvent(_) => {
                        let _ = sse_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::JobLive));
                    }
                    _ => {}
                }
                let _ = sse_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::Live(msg)));
            })
            .await;
        });

        fetch_summary(&client, &ui_tx).await;
        fetch_fleets(&client, &ui_tx).await;

        while let Some(cmd) = cmd_rx.next().await {
            match cmd {
                NetworkCommand::RefreshSummary => {
                    let _ = ui_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::RefreshStarted));
                    fetch_summary(&client, &ui_tx).await;
                }
                NetworkCommand::FetchSeries { pod, name } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    spawn_network(async move {
                        fetch_series(&client, &ui_tx, &pod, &name).await;
                    });
                }
                NetworkCommand::FetchSparkline { pod, name } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    spawn_network(async move {
                        fetch_sparkline(&client, &ui_tx, &pod, &name).await;
                    });
                }
                NetworkCommand::RefreshFleetDeck {
                    fleet,
                    branch,
                    probe_tree_after,
                } => {
                    refresh_fleet_deck(&client, &ui_tx, &fleet, branch, probe_tree_after).await;
                }
                NetworkCommand::RefreshFleets => {
                    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::RefreshStarted));
                    fetch_fleets(&client, &ui_tx).await;
                }
                NetworkCommand::RefreshInstances => {
                    fetch_instances(&client, &ui_tx).await;
                }
                NetworkCommand::FetchTree {
                    fleet,
                    branch,
                    probe,
                } => {
                    fetch_tree(&client, &ui_tx, &fleet, branch, probe).await;
                }
                NetworkCommand::FetchTreePhased { fleet, branch } => {
                    fetch_tree_phased(&client, &ui_tx, &fleet, branch).await;
                }
                NetworkCommand::FetchJobs { fleet } => {
                    fetch_jobs(&client, &ui_tx, &fleet).await;
                }
                NetworkCommand::FleetAction { action, payload } => {
                    fleet_action(&client, &ui_tx, &action, payload).await;
                }
                NetworkCommand::PollBoxProgress { contract } => {
                    let client = client.clone();
                    let ui_tx = ui_tx.clone();
                    spawn_network(async move {
                        poll_box_progress(&client, &ui_tx, &contract).await;
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
    probe_tree_after: bool,
) {
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::RefreshStarted));

    let (fleets_r, instances_r, jobs_r, tree_r) = tokio::join!(
        client.fetch_fleets(),
        client.fetch_instances(),
        client.fetch_jobs(fleet),
        client.fetch_tree(branch, fleet, false),
    );

    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Fleets(fleets_r)));
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Instances(instances_r)));
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Jobs { result: jobs_r }));
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Tree { result: tree_r }));

    if probe_tree_after && !fleet.is_empty() {
        let probed = client.fetch_tree(branch, fleet, true).await;
        let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Tree { result: probed }));
    }
}

async fn fetch_tree_phased(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    fleet: &str,
    branch: u32,
) {
    let skeleton = client.fetch_tree(branch, fleet, false).await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Tree { result: skeleton }));

    if fleet.is_empty() {
        return;
    }
    let probed = client.fetch_tree(branch, fleet, true).await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Tree { result: probed }));
}

async fn poll_box_progress(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    contract: &str,
) {
    for attempt in 0..MAX_PROGRESS_POLLS {
        let result = client.fetch_box_progress(contract).await;
        let terminal = result
            .as_ref()
            .ok()
            .is_some_and(|r| r.is_terminal());
        let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::BoxProgress {
            contract: contract.to_string(),
            result,
        }));
        if terminal {
            return;
        }
        if attempt + 1 < MAX_PROGRESS_POLLS {
            tokio::time::sleep(PROGRESS_POLL_INTERVAL).await;
        }
    }
}

async fn fetch_summary(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    info!(portal = %client.base_url(), "fetching runs summary");
    let summary = client.fetch_runs_summary().await;
    match &summary {
        Ok(s) => info!(runs = s.runs.len(), "runs summary fetched"),
        Err(e) => warn!("runs summary fetch failed: {e}"),
    }
    let _ = ui_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::Summary(summary)));
}

async fn fetch_fleets(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    let result = client.fetch_fleets().await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Fleets(result)));
}

async fn fetch_instances(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>) {
    let result = client.fetch_instances().await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Instances(result)));
}

async fn fetch_tree(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    fleet: &str,
    branch: u32,
    probe: bool,
) {
    let result = client.fetch_tree(branch, fleet, probe).await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Tree { result }));
}

async fn fetch_jobs(client: &Client, ui_tx: &UnboundedSender<AppUiMsg>, fleet: &str) {
    let result = client.fetch_jobs(fleet).await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Jobs { result }));
}

async fn fleet_action(
    client: &Client,
    ui_tx: &UnboundedSender<AppUiMsg>,
    action: &str,
    payload: Value,
) {
    let result = client.fleet_action(action, payload).await;
    let _ = ui_tx.unbounded_send(AppUiMsg::Fleets(FleetsMsg::Action {
        action: action.to_string(),
        result,
    }));
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
    let _ = ui_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::Series {
        pod: pod.to_string(),
        name: name.to_string(),
        result,
    }));
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
    let _ = ui_tx.unbounded_send(AppUiMsg::Dashboard(DashboardMsg::Sparkline {
        pod: pod.to_string(),
        name: name.to_string(),
        result,
    }));
}
