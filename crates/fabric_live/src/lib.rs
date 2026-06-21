//! Background SSE client for `/api/events`.

mod patch;

pub use patch::{append_point, default_max_points, patch_summary};

use anyhow::{Context, Result};
use fabric_api::Client;
use fabric_types::{SseJobEvent, SseRunEvent};
use futures::StreamExt;
use std::time::Duration;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub enum LiveMessage {
    Connected,
    Disconnected,
    RunEvent(SseRunEvent),
    JobEvent(SseJobEvent),
}

/// Read one SSE session until the stream closes. Calls `on_event` synchronously for each message.
pub async fn stream_events(
    client: &Client,
    mut on_event: impl FnMut(LiveMessage),
) -> Result<()> {
    let response = client.raw_get("/api/events").await.context("open sse")?;
    on_event(LiveMessage::Connected);

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("sse chunk")?;
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
                        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
                        match kind {
                            "run" => {
                                if let Ok(ev) = serde_json::from_value::<SseRunEvent>(v) {
                                    on_event(LiveMessage::RunEvent(ev));
                                }
                            }
                            "job" => {
                                if let Ok(ev) = serde_json::from_value::<SseJobEvent>(v) {
                                    on_event(LiveMessage::JobEvent(ev));
                                }
                            }
                            _ => debug!("ignored sse event type={kind}"),
                        }
                    }
                    Err(e) => debug!("malformed sse json: {e}"),
                }
            }
        }
    }

    Ok(())
}

/// Maintain SSE with reconnect backoff. Runs until the task is cancelled.
pub async fn run_sse_loop(
    client: Client,
    mut on_event: impl FnMut(LiveMessage),
) {
    loop {
        if let Err(e) = stream_events(&client, &mut on_event).await {
            warn!("sse stream ended: {e:#}");
        }
        on_event(LiveMessage::Disconnected);
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
