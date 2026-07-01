use crate::types::{BridgeConfig, HeartbeatResponse, WorkResponse, WorkSecret};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use serde_json::json;
use std::fmt;
use std::future::Future;
use std::sync::Arc;

const BETA_HEADER: &str = "environments-2025-11-01";

#[async_trait]
pub trait BridgeAuthProvider: Send + Sync {
    fn access_token(&self) -> Option<String>;

    async fn refresh_after_unauthorized(&self, _stale_access_token: &str) -> Result<bool> {
        Ok(false)
    }
}

#[derive(Debug)]
struct StaticBridgeAuthProvider {
    access_token: String,
}

#[async_trait]
impl BridgeAuthProvider for StaticBridgeAuthProvider {
    fn access_token(&self) -> Option<String> {
        Some(self.access_token.clone())
    }
}

pub struct BridgeApiClient {
    client: Client,
    base_url: String,
    auth: Arc<dyn BridgeAuthProvider>,
}

impl BridgeApiClient {
    pub fn new(base_url: String, access_token: String) -> Self {
        Self::with_auth_provider(
            base_url,
            Arc::new(StaticBridgeAuthProvider { access_token }),
        )
    }

    pub fn with_auth_provider(base_url: String, auth: Arc<dyn BridgeAuthProvider>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
        }
    }

    pub async fn register_environment(&self, config: &BridgeConfig) -> Result<(String, String)> {
        let url = format!("{}/v1/environments/bridge", self.base_url);
        let body = json!({
            "machine_name": config.machine_name,
            "directory": config.dir,
            "branch": config.branch,
            "git_repo_url": config.git_repo_url,
            "max_sessions": config.max_sessions,
            "metadata": {
                "worker_type": config.worker_type
            },
            "environment_id": config.environment_id,
        });

        let response = self
            .with_oauth_retry(
                |access_token| {
                    add_bridge_headers(self.client.post(&url), &access_token)
                        .json(&body)
                        .send()
                },
                "Registration",
            )
            .await?;
        let response = ensure_success(response, "Registration").await?;

        let data: serde_json::Value = response.json().await?;
        let environment_id = data
            .get("environment_id")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("bridge registration response missing environment_id"))?;
        let environment_secret = data
            .get("environment_secret")
            .and_then(|value| value.as_str())
            .ok_or_else(|| anyhow!("bridge registration response missing environment_secret"))?;

        Ok((environment_id.to_string(), environment_secret.to_string()))
    }

    pub async fn poll_for_work(
        &self,
        environment_id: &str,
        environment_secret: &str,
    ) -> Result<Option<WorkResponse>> {
        validate_bridge_id(environment_id, "environment_id")?;
        let url = format!(
            "{}/v1/environments/{}/work/poll",
            self.base_url, environment_id
        );

        let response = self
            .client
            .get(&url)
            .apply_bridge_headers(environment_secret)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?;

        if response.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        let response = ensure_success(response, "Poll").await?;
        let text = response.text().await?;
        if text.trim().is_empty() || text.trim() == "null" {
            return Ok(None);
        }
        let work: WorkResponse = serde_json::from_str(&text)?;
        Ok(Some(work))
    }

    pub async fn acknowledge_work(
        &self,
        environment_id: &str,
        work_id: &str,
        session_token: &str,
    ) -> Result<()> {
        validate_bridge_id(environment_id, "environment_id")?;
        validate_bridge_id(work_id, "work_id")?;
        let url = format!(
            "{}/v1/environments/{}/work/{}/ack",
            self.base_url, environment_id, work_id
        );

        self.client
            .post(&url)
            .apply_bridge_headers(session_token)
            .send()
            .await?
            .ensure_bridge_success("Acknowledge")
            .await?;

        Ok(())
    }

    pub async fn stop_work(&self, environment_id: &str, work_id: &str, force: bool) -> Result<()> {
        validate_bridge_id(environment_id, "environment_id")?;
        validate_bridge_id(work_id, "work_id")?;
        let url = format!(
            "{}/v1/environments/{}/work/{}/stop",
            self.base_url, environment_id, work_id
        );

        let body = json!({ "force": force });
        let response = self
            .with_oauth_retry(
                |access_token| {
                    add_bridge_headers(self.client.post(&url), &access_token)
                        .json(&body)
                        .send()
                },
                "StopWork",
            )
            .await?;
        ensure_success(response, "StopWork").await?;

        Ok(())
    }

    pub async fn heartbeat_work(
        &self,
        environment_id: &str,
        work_id: &str,
        session_token: &str,
    ) -> Result<HeartbeatResponse> {
        validate_bridge_id(environment_id, "environment_id")?;
        validate_bridge_id(work_id, "work_id")?;
        let url = format!(
            "{}/v1/environments/{}/work/{}/heartbeat",
            self.base_url, environment_id, work_id
        );

        let response = self
            .client
            .post(&url)
            .json(&json!({}))
            .apply_bridge_headers(session_token)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .ensure_bridge_success("Heartbeat")
            .await?;

        Ok(response.json().await?)
    }

    pub async fn archive_session(&self, session_id: &str) -> Result<()> {
        validate_bridge_id(session_id, "session_id")?;
        let url = format!("{}/v1/sessions/{}/archive", self.base_url, session_id);

        let response = self
            .with_oauth_retry(
                |access_token| {
                    add_bridge_headers(self.client.post(&url), &access_token)
                        .json(&json!({}))
                        .send()
                },
                "ArchiveSession",
            )
            .await?;
        if response.status() == StatusCode::CONFLICT {
            return Ok(());
        }
        ensure_success(response, "ArchiveSession").await?;

        Ok(())
    }

    pub async fn reconnect_session(&self, environment_id: &str, session_id: &str) -> Result<()> {
        validate_bridge_id(environment_id, "environment_id")?;
        validate_bridge_id(session_id, "session_id")?;
        let url = format!(
            "{}/v1/environments/{}/bridge/reconnect",
            self.base_url, environment_id
        );

        let body = json!({ "session_id": session_id });
        let response = self
            .with_oauth_retry(
                |access_token| {
                    add_bridge_headers(self.client.post(&url), &access_token)
                        .json(&body)
                        .send()
                },
                "ReconnectSession",
            )
            .await?;
        ensure_success(response, "ReconnectSession").await?;

        Ok(())
    }

    pub async fn deregister_environment(&self, environment_id: &str) -> Result<()> {
        validate_bridge_id(environment_id, "environment_id")?;
        let url = format!(
            "{}/v1/environments/bridge/{}",
            self.base_url, environment_id
        );

        let response = self
            .with_oauth_retry(
                |access_token| add_bridge_headers(self.client.delete(&url), &access_token).send(),
                "Deregister",
            )
            .await?;
        ensure_success(response, "Deregister").await?;

        Ok(())
    }

    async fn with_oauth_retry<F, Fut>(&self, mut request: F, context: &str) -> Result<Response>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = reqwest::Result<Response>>,
    {
        let access_token = self
            .auth
            .access_token()
            .ok_or_else(|| anyhow!("{} requires a bridge access token", context))?;
        let response = request(access_token.clone()).await?;
        if response.status() != StatusCode::UNAUTHORIZED {
            return Ok(response);
        }

        if !self
            .auth
            .refresh_after_unauthorized(&access_token)
            .await
            .with_context(|| format!("{} token refresh failed", context))?
        {
            return Ok(response);
        }

        let refreshed_token = self.auth.access_token().ok_or_else(|| {
            anyhow!(
                "{} token refresh did not provide a new access token",
                context
            )
        })?;
        Ok(request(refreshed_token).await?)
    }
}

fn validate_bridge_id(id: &str, label: &str) -> Result<()> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    {
        return Err(anyhow!("invalid {}: contains unsafe characters", label));
    }
    Ok(())
}

trait BridgeRequestBuilderExt {
    fn apply_bridge_headers(self, bearer_token: &str) -> Self;
}

impl BridgeRequestBuilderExt for RequestBuilder {
    fn apply_bridge_headers(self, bearer_token: &str) -> Self {
        add_bridge_headers(self, bearer_token)
    }
}

trait BridgeResponseExt {
    async fn ensure_bridge_success(self, context: &str) -> Result<Response>;
}

impl BridgeResponseExt for Response {
    async fn ensure_bridge_success(self, context: &str) -> Result<Response> {
        ensure_success(self, context).await
    }
}

fn add_bridge_headers(builder: RequestBuilder, bearer_token: &str) -> RequestBuilder {
    builder
        .header("Authorization", format!("Bearer {}", bearer_token))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", BETA_HEADER)
}

#[derive(Debug)]
pub struct BridgeHttpError {
    context: String,
    status: StatusCode,
    detail: String,
}

impl BridgeHttpError {
    pub fn status(&self) -> StatusCode {
        self.status
    }
}

impl fmt::Display for BridgeHttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{} failed: HTTP {}", self.context, self.status)
        } else {
            write!(
                f,
                "{} failed: HTTP {}: {}",
                self.context, self.status, self.detail
            )
        }
    }
}

impl std::error::Error for BridgeHttpError {}

pub fn bridge_error_status(error: &anyhow::Error) -> Option<StatusCode> {
    error
        .downcast_ref::<BridgeHttpError>()
        .map(BridgeHttpError::status)
}

async fn ensure_success(response: Response, context: &str) -> Result<Response> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    Err(BridgeHttpError {
        context: context.to_string(),
        status,
        detail: body.trim().to_string(),
    }
    .into())
}

pub fn decode_work_secret(encoded: &str) -> Result<WorkSecret> {
    use base64::Engine;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(encoded)
        .context("failed to base64url-decode work secret")?;
    let json_str = String::from_utf8(decoded).context("work secret is not valid UTF-8")?;
    let secret: WorkSecret = serde_json::from_str(&json_str).context("work secret is not JSON")?;
    Ok(secret)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SpawnMode;
    use serde_json::Value;
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug)]
    struct MockResponse {
        status: u16,
        body: String,
    }

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        body: String,
    }

    #[derive(Debug)]
    struct RefreshingAuthProvider {
        token: Mutex<String>,
        refresh_count: AtomicUsize,
    }

    #[async_trait]
    impl BridgeAuthProvider for RefreshingAuthProvider {
        fn access_token(&self) -> Option<String> {
            Some(self.token.lock().unwrap().clone())
        }

        async fn refresh_after_unauthorized(&self, stale_access_token: &str) -> Result<bool> {
            self.refresh_count.fetch_add(1, Ordering::SeqCst);
            if stale_access_token == "stale-token" {
                *self.token.lock().unwrap() = "fresh-token".to_string();
                Ok(true)
            } else {
                Ok(false)
            }
        }
    }

    fn bridge_config() -> BridgeConfig {
        BridgeConfig {
            dir: "/workspace/project".to_string(),
            machine_name: "machine".to_string(),
            branch: "main".to_string(),
            git_repo_url: Some("https://example.test/repo.git".to_string()),
            max_sessions: 2,
            spawn_mode: SpawnMode::Worktree,
            bridge_id: "bridge-1".to_string(),
            worker_type: "kiana_code".to_string(),
            environment_id: "env-reuse".to_string(),
            api_base_url: "http://unused.test".to_string(),
            session_ingress_url: "wss://unused.test".to_string(),
            heartbeat_interval_ms: 60_000,
            session_timeout_ms: 24 * 60 * 60 * 1000,
            ccr_v2_sse_reconnect_give_up_ms: Some(0),
            ccr_v2_sse_liveness_timeout_ms: Some(0),
            debug_file: None,
            permission_mode: None,
        }
    }

    #[tokio::test]
    async fn register_environment_refreshes_token_once_on_401() {
        let (base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(401, r#"{"error":"expired"}"#),
            MockResponse::json(
                200,
                r#"{"environment_id":"env-new","environment_secret":"secret-new"}"#,
            ),
        ])
        .await;
        let auth = Arc::new(RefreshingAuthProvider {
            token: Mutex::new("stale-token".to_string()),
            refresh_count: AtomicUsize::new(0),
        });
        let api = BridgeApiClient::with_auth_provider(base_url, auth.clone());

        let (environment_id, environment_secret) =
            api.register_environment(&bridge_config()).await.unwrap();

        assert_eq!(environment_id, "env-new");
        assert_eq!(environment_secret, "secret-new");
        assert_eq!(auth.refresh_count.load(Ordering::SeqCst), 1);

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/environments/bridge");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer stale-token")
        );
        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer fresh-token")
        );
        let body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(body["environment_id"], json!("env-reuse"));
        assert_eq!(body["max_sessions"], json!(2));
        assert_eq!(body["metadata"]["worker_type"], json!("kiana_code"));
    }

    #[tokio::test]
    async fn poll_for_work_reports_non_success_status() {
        let (base_url, requests) =
            spawn_mock_server(vec![MockResponse::json(401, r#"{"error":"expired"}"#)]).await;
        let api = BridgeApiClient::new(base_url, "api-token".to_string());

        let error = api
            .poll_for_work("env-1", "environment-secret")
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("Poll failed: HTTP 401"));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer environment-secret")
        );
    }

    #[tokio::test]
    async fn heartbeat_work_posts_with_session_token_and_parses_response() {
        let (base_url, requests) = spawn_mock_server(vec![MockResponse::json(
            200,
            r#"{"lease_extended":true,"state":"running","last_heartbeat":"now","ttl_seconds":300}"#,
        )])
        .await;
        let api = BridgeApiClient::new(base_url, "api-token".to_string());

        let response = api
            .heartbeat_work("env-1", "work-1", "session-token")
            .await
            .unwrap();

        assert!(response.lease_extended);
        assert_eq!(response.state, "running");
        assert_eq!(response.ttl_seconds, Some(300));
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(
            requests[0].path,
            "/v1/environments/env-1/work/work-1/heartbeat"
        );
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer session-token")
        );
    }

    #[tokio::test]
    async fn heartbeat_error_preserves_http_status_for_reconnect_decisions() {
        let (base_url, _requests) =
            spawn_mock_server(vec![MockResponse::json(403, r#"{"error":"forbidden"}"#)]).await;
        let api = BridgeApiClient::new(base_url, "api-token".to_string());

        let error = api
            .heartbeat_work("env-1", "work-1", "session-token")
            .await
            .unwrap_err();

        assert_eq!(bridge_error_status(&error), Some(StatusCode::FORBIDDEN));
        assert!(error.to_string().contains("Heartbeat failed: HTTP 403"));
    }

    #[tokio::test]
    async fn archive_session_posts_with_oauth_and_treats_conflict_as_success() {
        let (base_url, requests) = spawn_mock_server(vec![MockResponse::json(
            409,
            r#"{"error":"already archived"}"#,
        )])
        .await;
        let api = BridgeApiClient::new(base_url, "api-token".to_string());

        api.archive_session("session-1").await.unwrap();

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/sessions/session-1/archive");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer api-token")
        );
        assert_eq!(requests[0].body, "{}");
    }

    #[tokio::test]
    async fn reconnect_session_posts_session_id_and_refreshes_oauth_token() {
        let (base_url, requests) = spawn_mock_server(vec![
            MockResponse::json(401, r#"{"error":"expired"}"#),
            MockResponse::json(200, r#"{}"#),
        ])
        .await;
        let auth = Arc::new(RefreshingAuthProvider {
            token: Mutex::new("stale-token".to_string()),
            refresh_count: AtomicUsize::new(0),
        });
        let api = BridgeApiClient::with_auth_provider(base_url, auth.clone());

        api.reconnect_session("env-1", "session-1").await.unwrap();

        assert_eq!(auth.refresh_count.load(Ordering::SeqCst), 1);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].path, "/v1/environments/env-1/bridge/reconnect");
        assert_eq!(
            requests[0].authorization.as_deref(),
            Some("Bearer stale-token")
        );
        assert_eq!(
            requests[1].authorization.as_deref(),
            Some("Bearer fresh-token")
        );
        let body: Value = serde_json::from_str(&requests[1].body).unwrap();
        assert_eq!(body["session_id"], json!("session-1"));
    }

    #[test]
    fn validate_bridge_id_rejects_unsafe_path_segments() {
        assert!(validate_bridge_id("env_1-work", "environment_id").is_ok());
        for unsafe_id in ["", "../env", "env/one", "env.one", "env%2Fone"] {
            let error = validate_bridge_id(unsafe_id, "environment_id")
                .unwrap_err()
                .to_string();
            assert!(error.contains("invalid environment_id"));
        }
    }

    #[tokio::test]
    async fn api_methods_reject_unsafe_ids_before_http_request() {
        let api = BridgeApiClient::new("http://127.0.0.1:9".to_string(), "api-token".to_string());

        let error = api
            .poll_for_work("env/one", "environment-secret")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid environment_id"));

        let error = api
            .heartbeat_work("env-one", "../work", "session-token")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid work_id"));

        let error = api
            .archive_session("session/one")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid session_id"));

        let error = api
            .reconnect_session("env-one", "../session")
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("invalid session_id"));
    }

    async fn spawn_mock_server(
        responses: Vec<MockResponse>,
    ) -> (String, Arc<Mutex<Vec<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let shared_requests = requests.clone();
        let responses = Arc::new(Mutex::new(VecDeque::from(responses)));
        let shared_responses = responses.clone();

        tokio::spawn(async move {
            loop {
                let response = {
                    let mut responses = shared_responses.lock().unwrap();
                    responses.pop_front()
                };
                let Some(response) = response else {
                    break;
                };
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                match read_http_request(&mut stream).await {
                    Ok(request) => shared_requests.lock().unwrap().push(request),
                    Err(_) => break,
                }
                let _ = write_http_response(&mut stream, response).await;
            }
        });

        (format!("http://{}", address), requests)
    }

    async fn read_http_request(stream: &mut TcpStream) -> std::io::Result<RecordedRequest> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        let header_end = loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "connection closed before headers",
                ));
            }
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(index) = find_header_end(&buffer) {
                break index;
            }
        };

        let header_text = String::from_utf8_lossy(&buffer[..header_end]);
        let mut lines = header_text.lines();
        let request_line = lines.next().unwrap_or_default();
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts.next().unwrap_or_default().to_string();
        let path = request_parts.next().unwrap_or_default().to_string();
        let mut authorization = None;
        let mut content_length = 0_usize;

        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.trim().to_string());
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or_default();
            }
        }

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let body = String::from_utf8_lossy(
            &buffer[body_start..buffer.len().min(body_start + content_length)],
        )
        .to_string();

        Ok(RecordedRequest {
            method,
            path,
            authorization,
            body,
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        response: MockResponse,
    ) -> std::io::Result<()> {
        let status_text = match response.status {
            200 => "OK",
            204 => "No Content",
            401 => "Unauthorized",
            404 => "Not Found",
            409 => "Conflict",
            500 => "Internal Server Error",
            _ => "Status",
        };
        let payload = response.body.as_bytes();
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.status,
                    status_text,
                    payload.len()
                )
                .as_bytes(),
            )
            .await?;
        stream.write_all(payload).await?;
        stream.flush().await
    }

    impl MockResponse {
        fn json(status: u16, body: &str) -> Self {
            Self {
                status,
                body: body.to_string(),
            }
        }
    }
}
