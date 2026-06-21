//! UI → network command channel + background fetch/SSE loop.

use crate::detail::SERIES_MAX_POINTS;
use fabric_api::{spawn_network, Client, ClientError};
use fabric_live::{run_sse_loop, LiveMessage};
use fabric_types::{RunSeries, RunsSummary};
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use futures::StreamExt;
use tracing::{info, warn};

/// Commands from the GPUI view to the Tokio network task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkCommand {
    RefreshSummary,
    FetchSeries { pod: String, name: String },
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
}

pub fn spawn_dashboard_network(
    client: Client,
    ui_tx: UnboundedSender<DashboardMsg>,
    mut cmd_rx: UnboundedReceiver<NetworkCommand>,
) {
    spawn_network(async move {
        let sse_client = client.clone();
        let sse_tx = ui_tx.clone();
        spawn_network(async move {
            run_sse_loop(sse_client, |msg| {
                let _ = sse_tx.unbounded_send(DashboardMsg::Live(msg));
            })
            .await;
        });

        fetch_summary(&client, &ui_tx).await;

        while let Some(cmd) = cmd_rx.next().await {
            match cmd {
                NetworkCommand::RefreshSummary => {
                    let _ = ui_tx.unbounded_send(DashboardMsg::RefreshStarted);
                    fetch_summary(&client, &ui_tx).await;
                }
                NetworkCommand::FetchSeries { pod, name } => {
                    fetch_series(&client, &ui_tx, &pod, &name).await;
                }
            }
        }
    });
}

async fn fetch_summary(client: &Client, ui_tx: &UnboundedSender<DashboardMsg>) {
    info!(portal = %client.base_url(), "fetching runs summary");
    let summary = client.fetch_runs_summary().await;
    match &summary {
        Ok(s) => info!(runs = s.runs.len(), "runs summary fetched"),
        Err(e) => warn!("runs summary fetch failed: {e}"),
    }
    let _ = ui_tx.unbounded_send(DashboardMsg::Summary(summary));
}

async fn fetch_series(
    client: &Client,
    ui_tx: &UnboundedSender<DashboardMsg>,
    pod: &str,
    name: &str,
) {
    info!(pod, name, "fetching run series");
    let result = client
        .fetch_run_series(pod, name, SERIES_MAX_POINTS)
        .await;
    match &result {
        Ok(s) => info!(points = s.epochs.len(), "run series fetched"),
        Err(e) => warn!("run series fetch failed: {e}"),
    }
    let _ = ui_tx.unbounded_send(DashboardMsg::Series {
        pod: pod.to_string(),
        name: name.to_string(),
        result,
    });
}
