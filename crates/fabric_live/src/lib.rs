//! Background SSE client for `/api/events`.

mod patch;

pub use patch::{append_point, default_max_points, patch_summary};

use anyhow::{Context, Result};
use fabric_api::Client;
use fabric_types::SseRunEvent;
use futures::StreamExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, warn};

#[derive(Debug, Clone)]
pub enum LiveMessage {
    Connected,
    Disconnected,
    RunEvent(SseRunEvent),
}

pub struct SseClient;

impl SseClient {
    /// Spawn a background task that maintains an SSE connection and forwards parsed events.
    pub fn spawn(client: Client) -> mpsc::UnboundedReceiver<LiveMessage> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            loop {
                if let Err(e) = stream_events(&client, &tx).await {
                    warn!("sse stream ended: {e:#}");
                }
                let _ = tx.send(LiveMessage::Disconnected);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
        rx
    }
}

async fn stream_events(client: &Client, tx: &mpsc::UnboundedSender<LiveMessage>) -> Result<()> {
    let response = client
        .raw_get("/api/events")
        .await
        .context("open sse")?;
    let _ = tx.send(LiveMessage::Connected);

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
                match serde_json::from_str::<SseRunEvent>(data) {
                    Ok(ev) if ev.kind == "run" => {
                        let _ = tx.send(LiveMessage::RunEvent(ev));
                    }
                    Ok(_) => debug!("ignored sse event"),
                    Err(e) => debug!("malformed sse json: {e}"),
                }
            }
        }
    }

    Ok(())
}
