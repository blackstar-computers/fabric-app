use fabric_types::{RunsSummary, SERVICE_TOKEN_HEADER};
use reqwest::StatusCode;
use thiserror::Error;

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
