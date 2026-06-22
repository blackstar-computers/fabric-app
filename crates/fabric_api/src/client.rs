use fabric_types::{
    BoxProgressResp, CheckpointsResp, FleetsResp, GpuSearchResp, InstancesResp, JobsResp,
    RunSeries, RunsSummary, SERVICE_TOKEN_HEADER, TopoManifestResp, TreeResp, VizGalleryResp,
    VizLoadMeta, VizOpenRequest, VizOpenResp, VizStatusResp, VizStepRequest,
};
use reqwest::StatusCode;
use std::time::Duration;
use thiserror::Error;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const AUTHORIZATION_HEADER: &str = "Authorization";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthKind {
    ServiceToken,
    SessionBearer,
}

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("portal returned {status}: {message}")]
    Api { status: StatusCode, message: String },
    #[error("invalid json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("session expired")]
    Unauthorized,
}

impl ClientError {
    pub fn is_unauthorized(&self) -> bool {
        match self {
            ClientError::Unauthorized => true,
            ClientError::Api { status, .. } => *status == StatusCode::UNAUTHORIZED,
            _ => false,
        }
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::Api {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::Api {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

fn api_error(status: StatusCode, message: String) -> ClientError {
    if status == StatusCode::UNAUTHORIZED {
        ClientError::Unauthorized
    } else {
        ClientError::Api { status, message }
    }
}

fn build_http(timeout: Option<Duration>) -> Result<reqwest::Client, ClientError> {
    let mut builder = reqwest::Client::builder().user_agent("fabric-app/0.1");
    if let Some(t) = timeout {
        builder = builder.timeout(t);
    }
    builder.build().map_err(ClientError::Http)
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    sse: reqwest::Client,
    base_url: String,
    token: String,
    auth_kind: AuthKind,
}

impl Client {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self::try_with_auth(base_url, token, AuthKind::ServiceToken).expect("reqwest client")
    }

    pub fn with_session(base_url: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self::try_with_auth(base_url, access_token, AuthKind::SessionBearer).expect("reqwest client")
    }

    pub fn try_new(base_url: impl Into<String>, token: impl Into<String>) -> Result<Self, ClientError> {
        Self::try_with_auth(base_url, token, AuthKind::ServiceToken)
    }

    pub fn try_with_session(
        base_url: impl Into<String>,
        access_token: impl Into<String>,
    ) -> Result<Self, ClientError> {
        Self::try_with_auth(base_url, access_token, AuthKind::SessionBearer)
    }

    fn try_with_auth(
        base_url: impl Into<String>,
        token: impl Into<String>,
        auth_kind: AuthKind,
    ) -> Result<Self, ClientError> {
        Ok(Self {
            http: build_http(Some(REQUEST_TIMEOUT))?,
            sse: build_http(None)?,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            token: token.into(),
            auth_kind,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn auth_kind(&self) -> AuthKind {
        self.auth_kind.clone()
    }

    fn attach_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.auth_kind {
            AuthKind::ServiceToken => req.header(SERVICE_TOKEN_HEADER, &self.token),
            AuthKind::SessionBearer => {
                req.header(AUTHORIZATION_HEADER, format!("Bearer {}", self.token))
            }
        }
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

    pub async fn fetch_gpu_search(&self, num_gpus: u32) -> Result<GpuSearchResp, ClientError> {
        let path = format!("/api/fleets/search?num_gpus={num_gpus}");
        self.get_json(&path).await
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

    pub async fn fetch_checkpoints(&self, fleet: &str) -> Result<CheckpointsResp, ClientError> {
        let path = if fleet.is_empty() {
            "/api/checkpoints".to_string()
        } else {
            format!("/api/checkpoints?fleet={}", url_encode(fleet))
        };
        self.get_json(&path).await
    }

    pub async fn fetch_topo_manifest(&self) -> Result<TopoManifestResp, ClientError> {
        self.get_json("/api/topology/manifest").await
    }

    /// Raw GET (e.g. `/api/topology/file?run=…&file=…` for `.fab` topology blobs).
    pub async fn fetch_bytes(&self, path: &str) -> Result<Vec<u8>, ClientError> {
        self.get_bytes(path).await
    }

    /// Kick off background checkpoint sync + viewer warm (`web_app/src/api.ts::vizOpen`).
    pub async fn viz_open(&self, body: &VizOpenRequest) -> Result<VizOpenResp, ClientError> {
        let mut json = serde_json::to_value(body)?;
        if let Some(obj) = json.as_object_mut() {
            obj.insert("background".into(), serde_json::Value::Bool(true));
        }
        self.post_json("/api/viz/open", json).await
    }

    pub async fn viz_status(&self, ckpt: &str) -> Result<VizStatusResp, ClientError> {
        let path = if ckpt.is_empty() {
            "/api/viz/status".to_string()
        } else {
            format!("/api/viz/status?ckpt={}", url_encode(ckpt))
        };
        self.get_json(&path).await
    }

    /// Loaded-model meta from the proxied viewer daemon (`fetchVizState`).
    pub async fn viz_state(&self) -> Result<VizLoadMeta, ClientError> {
        self.get_json("/viz/default/api/state").await
    }

    pub async fn viz_step(&self, body: &VizStepRequest) -> Result<serde_json::Value, ClientError> {
        let json = serde_json::to_value(body)?;
        self.post_json("/viz/default/api/step", json).await
    }

    /// Zero the substrate activation state (`viewer.py` `Session.reset()`). Recon rollouts
    /// accumulate `self.a` across `/api/step` calls; reset before each RUN so tick 0 reflects
    /// a fresh drive injection for the newly selected input.
    pub async fn viz_reset(&self) -> Result<serde_json::Value, ClientError> {
        self.post_json("/viz/default/api/reset", serde_json::json!({}))
            .await
    }

    /// Paginated dataset thumbnails from the proxied viewer daemon — backs the input gallery
    /// (`InputPicker.tsx` grid). `size` is the requested thumbnail edge in pixels.
    pub async fn fetch_viz_gallery(
        &self,
        dataset: &str,
        start: u32,
        count: u32,
        size: u32,
    ) -> Result<VizGalleryResp, ClientError> {
        let path = format!(
            "/viz/default/api/gallery?dataset={}&start={}&count={}&size={}",
            url_encode(dataset),
            start,
            count,
            size
        );
        self.get_json(&path).await
    }

    /// Path of a single dataset image on the proxied viewer daemon. Path-only helper so callers
    /// can build a URL or feed it to [`Self::fetch_viz_image_bytes`].
    pub fn viz_image_path(&self, dataset: &str, idx: u32, size: u32) -> String {
        format!(
            "/viz/default/api/image?idx={}&dataset={}&size={}",
            idx,
            url_encode(dataset),
            size
        )
    }

    /// Raw PNG bytes for a single dataset image (`/viz/default/api/image?idx=&dataset=&size=`).
    pub async fn fetch_viz_image_bytes(
        &self,
        dataset: &str,
        idx: u32,
        size: u32,
    ) -> Result<Vec<u8>, ClientError> {
        let path = self.viz_image_path(dataset, idx, size);
        self.get_bytes(&path).await
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
        let response = self
            .attach_auth(
                self.sse
                    .get(&url)
                    .header("Accept", "text/event-stream"),
            )
            .send()
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        Ok(response)
    }

    async fn post_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: serde_json::Value,
    ) -> Result<T, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .attach_auth(
                self.http
                    .post(&url)
                    .header("Accept", "application/json")
                    .header("Content-Type", "application/json"),
            )
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
            return Err(api_error(status, message));
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
            return Err(api_error(status, message));
        }
        Ok(serde_json::from_value(body)?)
    }

    async fn get_raw(&self, path: &str) -> Result<(StatusCode, serde_json::Value), ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let response = self
            .attach_auth(
                self.http
                    .get(&url)
                    .header("Accept", "application/json"),
            )
            .send()
            .await?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        let body: serde_json::Value = response.json().await.unwrap_or_else(|_| {
            serde_json::json!({ "error": "non-JSON response from portal" })
        });
        Ok((status, body))
    }

    async fn get_bytes(&self, path: &str) -> Result<Vec<u8>, ClientError> {
        let url = format!("{}{}", self.base_url, path);
        let response = self.attach_auth(self.http.get(&url)).send().await?;
        let status = response.status();
        if status == StatusCode::UNAUTHORIZED {
            return Err(ClientError::Unauthorized);
        }
        if !status.is_success() {
            let message = response
                .json::<serde_json::Value>()
                .await
                .ok()
                .and_then(|body| {
                    body.get("error")
                        .and_then(|v| v.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_else(|| format!("request failed ({status})"));
            return Err(api_error(status, message));
        }
        Ok(response.bytes().await?.to_vec())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unauthorized_detection() {
        assert!(ClientError::Unauthorized.is_unauthorized());
        assert!(
            ClientError::Api {
                status: StatusCode::UNAUTHORIZED,
                message: "nope".into(),
            }
            .is_unauthorized()
        );
        assert!(
            !ClientError::Api {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                message: "boom".into(),
            }
            .is_unauthorized()
        );
    }

    #[test]
    fn api_error_maps_401_to_unauthorized() {
        let err = api_error(StatusCode::UNAUTHORIZED, "Google SSO required".into());
        assert!(matches!(err, ClientError::Unauthorized));
    }
}
