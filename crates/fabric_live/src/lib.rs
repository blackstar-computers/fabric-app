//! Background SSE client for `/api/events`.

mod patch;

pub use patch::{
    append_point, default_max_points, insert_box, merge_instances_reconcile, patch_box, patch_fleet,
    patch_job, patch_summary, patch_tree_node, PatchOutcome, TreePatchOutcome,
};

use fabric_api::ClientError;
use fabric_api::Client;
use fabric_types::{SseBoxEvent, SseFleetEvent, SseJobEvent, SseNodeEvent, SseRunEvent};
use futures::StreamExt;
use reqwest::StatusCode;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub enum LiveMessage {
    Connected,
    Disconnected,
    RunEvent(SseRunEvent),
    JobEvent(SseJobEvent),
    BoxEvent(SseBoxEvent),
    FleetEvent(SseFleetEvent),
    NodeEvent(SseNodeEvent),
}

/// Parse one SSE `data:` JSON payload into a live message.
///
/// Returns `None` for empty payloads, unknown event types, or malformed JSON.
pub fn parse_sse_payload(v: serde_json::Value) -> Option<LiveMessage> {
    let kind = v.get("type")?.as_str()?;
    match kind {
        "run" => serde_json::from_value::<SseRunEvent>(v)
            .ok()
            .map(LiveMessage::RunEvent),
        "job" => serde_json::from_value::<SseJobEvent>(v)
            .ok()
            .map(LiveMessage::JobEvent),
        "box" | "instance" => serde_json::from_value::<SseBoxEvent>(v)
            .ok()
            .map(LiveMessage::BoxEvent),
        "fleet" => serde_json::from_value::<SseFleetEvent>(v)
            .ok()
            .map(LiveMessage::FleetEvent),
        "node" | "tree" => serde_json::from_value::<SseNodeEvent>(v)
            .ok()
            .map(LiveMessage::NodeEvent),
        _ => {
            debug!("ignored sse event type={kind}");
            None
        }
    }
}

/// Read one SSE session until the stream closes. Calls `on_event` synchronously for each message.
pub async fn stream_events(
    client: &Client,
    mut on_event: impl FnMut(LiveMessage),
) -> Result<(), ClientError> {
    let response = client.raw_get("/api/events").await?;
    if !response.status().is_success() {
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        return Err(ClientError::Api {
            status: response.status(),
            message: "sse connect failed".into(),
        });
    }
    on_event(LiveMessage::Connected);

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(ClientError::Http)?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        let mut lines = Vec::new();
        while let Some(nl) = buffer.find('\n') {
            let line = buffer[..nl].trim_end_matches('\r').to_string();
            buffer.drain(..=nl);
            lines.push(line);
        }

        for line in lines {
            if let Some(data) = line.strip_prefix("data: ") {
                if data.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<serde_json::Value>(data) {
                    Ok(v) => {
                        if let Some(msg) = parse_sse_payload(v) {
                            on_event(msg);
                        }
                    }
                    Err(e) => debug!("malformed sse json: {e}"),
                }
            }
        }
    }

    Ok(())
}

/// Why the SSE reconnect loop stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseLoopExit {
    Shutdown,
    Unauthorized,
}

/// Maintain SSE with reconnect backoff until shutdown or auth failure.
pub async fn run_sse_loop(
    client: Client,
    mut on_event: impl FnMut(LiveMessage),
    mut should_continue: impl FnMut() -> bool,
) -> SseLoopExit {
    loop {
        if !should_continue() {
            return SseLoopExit::Shutdown;
        }
        match stream_events(&client, &mut on_event).await {
            Ok(()) => {}
            Err(e) if e.is_unauthorized() => {
                warn!("sse auth failed — stopping reconnect loop");
                on_event(LiveMessage::Disconnected);
                return SseLoopExit::Unauthorized;
            }
            Err(e) => warn!("sse stream ended: {e}"),
        }
        if !should_continue() {
            return SseLoopExit::Shutdown;
        }
        on_event(LiveMessage::Disconnected);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_payload_dispatches_job() {
        let v = serde_json::json!({
            "type": "job",
            "v": 2,
            "job_id": "j1",
            "fleet": "f1",
            "state": "running",
            "pod": "n1"
        });
        let msg = parse_sse_payload(v).expect("job event");
        match msg {
            LiveMessage::JobEvent(ev) => {
                assert_eq!(ev.job_id, "j1");
                assert_eq!(ev.fleet, "f1");
            }
            other => panic!("expected JobEvent, got {other:?}"),
        }
    }

    #[test]
    fn parse_sse_payload_dispatches_box_and_aliases() {
        for kind in ["box", "instance"] {
            let v = serde_json::json!({
                "type": kind,
                "v": 1,
                "id": "box-1",
                "status": "ready"
            });
            assert!(matches!(
                parse_sse_payload(v),
                Some(LiveMessage::BoxEvent(_))
            ));
        }
    }

    #[test]
    fn parse_sse_payload_ignores_unknown_type() {
        let v = serde_json::json!({ "type": "future_thing", "id": 1 });
        assert!(parse_sse_payload(v).is_none());
    }

    #[test]
    fn parse_sse_payload_box_fleet_id_null() {
        let v = serde_json::json!({
            "type": "box",
            "id": "box-1",
            "fleet_id": null
        });
        let msg = parse_sse_payload(v).expect("box event");
        let LiveMessage::BoxEvent(ev) = msg else {
            panic!("expected BoxEvent");
        };
        assert_eq!(ev.fleet_id, Some(None));
    }
}
