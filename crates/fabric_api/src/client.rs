use fabric_types::{
    BoxProgressResp, FleetsResp, InstancesResp, JobsResp, RunSeries, RunsSummary,
    SERVICE_TOKEN_HEADER, TreeResp,
};
use reqwest::StatusCode;
use std::time::Duration;
use thiserror::Error;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("portal returned {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base_url: String,
    token: String,
}

impl Client {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent("fabric-app/0.1")
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client"),
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub async fn fetch_runs_summary(&self) -> Result<RunsSummary, ClientError> {
        self.get_json("/api/runs/summary").await
    }

    /// Lazy per-run series (`web_app/src/api.ts::fetchRunSeries`).
    pub async fn fetch_run_series(
        &self,
        pod: &str,
        name: &str,
        max_points: u32,
    ) -> Result<RunSeries, ClientError> {
        let path = format!(
            "/api/runs/series?pod={}&name={}&max={}",
            url_encode(pod),
            url_encode(name),
            max_points
        );
        self.get_json(&path).await
    }

    pub async fn fetch_fleets(&self) -> Result<FleetsResp, ClientError> {
        self.get_json("/api/fleets").await
    }

    pub async fn fetch_instances(&self) -> Result<InstancesResp, ClientError> {
        self.get_json("/api/boxes/instances").await
    }

    /// Provisioning progress for a GPU box (`web_app` assign/rent flow).
    pub async fn fetch_box_progress(&self, contract: &str) -> Result<BoxProgressResp, ClientError> {
        let path = format!(
            "/api/boxes/progress?contract={}",
            url_encode(contract)
        );
        self.get_json(&path).await
    }

    pub async fn fetch_jobs(&self, fleet: &str) -> Result<JobsResp, ClientError> {
        let path = if fleet.is_empty() {
            "/api/jobs".to_string()
        } else {
            format!("/api/jobs?fleet={}", url_encode(fleet))
        };
        self.get_json(&path).await
    }

    pub async fn fetch_tree(
        &self,
        branch: u32,
        fleet: &str,
        probe: bool,
    ) -> Result<TreeResp, ClientError> {
        let probe_q = if probe { "1" } else { "0" };
        let mut path = format!("/api/tree?branch={branch}&probe={probe_q}");
        if !fleet.is_empty() {
            path.push_str(&format!("&fleet={}", url_encode(fleet)));
        }
        self.get_json(&path).await
    }

    pub async fn fleet_action(
        &self,
        action: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let mut body = payload.as_object().cloned().unwrap_or_default();
        body.insert("action".into(), serde_json::Value::String(action.into()));
        self.post_json("/api/fleets", serde_json::Value::Object(body))
            .await
    }

    pub async fn health_check(&self) -> Result<(), ClientError> {
        let (_status, _body) = self.get_raw("/api/fleets").await?;
        Ok(())
    }

    pub fn sse_url(&self) -> String {
        format!("{}/api/events", self.base_url)
    }

    /// Raw GET for streaming endpoints (SSE). Caller owns the response body stream.
    pub async fn raw_get(&self, path: &str) -> Result<reqwest::Response, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        Ok(self
            .http
            .get(&url)
            .header(SERVICE_TOKEN_HEADER, &self.token)
            .header("Accept", "text/event-stream")
            .send()
            .await?)
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .http
            .post(&url)
            .header(SERVICE_TOKEN_HEADER, &self.token)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_else(|_| {
            serde_json::json!({ "error": "non-JSON response from portal" })
        });
        if !status.is_success() {
            let message = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("request failed")
                .to_string();
            return Err(ClientError::Api { status, message });
        }
        Ok(serde_json::from_value(body)?)
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ClientError> {
        let (status, body) = self.get_raw(path).await?;
        if !status.is_success() {
            let message = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("request failed")
                .to_string();
            return Err(ClientError::Api { status, message });
        }
        Ok(serde_json::from_value(body)?)
    }

    async fn get_raw(&self, path: &str) -> Result<(StatusCode, serde_json::Value), ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .http
            .get(&url)
            .header(SERVICE_TOKEN_HEADER, &self.token)
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.unwrap_or_else(|_| {
            serde_json::json!({ "error": "non-JSON response from portal" })
        });
        Ok((status, body))
    }
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
