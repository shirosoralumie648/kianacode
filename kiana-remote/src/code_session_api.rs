use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;
use thiserror::Error;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const CODE_SESSION_API_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteCredentials {
    pub worker_jwt: String,
    pub api_base_url: String,
    pub expires_in: u64,
    pub worker_epoch: u64,
}

#[derive(Debug, Error)]
pub enum CodeSessionApiError {
    #[error("{0}")]
    Message(String),
    #[error("{action} failed: {status}{message}")]
    HttpStatus {
        action: String,
        status: StatusCode,
        message: String,
    },
    #[error("code session request failed: {0}")]
    Request(#[from] reqwest::Error),
}

impl CodeSessionApiError {
    pub fn http_status(&self) -> Option<StatusCode> {
        match self {
            Self::HttpStatus { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn is_auth_failure_status(&self) -> bool {
        matches!(
            self.http_status(),
            Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
        )
    }
}

pub async fn create_code_session(
    api_base_url: &str,
    access_token: &str,
    title: &str,
    tags: &[String],
) -> Result<String, CodeSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    let url = format!("{base_url}/v1/code/sessions");
    let body = json!({
        "title": title,
        "bridge": {},
    });
    let body = if tags.is_empty() {
        body
    } else {
        let mut body = body;
        body["tags"] = Value::Array(tags.iter().cloned().map(Value::String).collect());
        body
    };

    let response = add_code_session_headers(reqwest::Client::new().post(url), access_token)
        .timeout(CODE_SESSION_API_TIMEOUT)
        .json(&body)
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK
        && response.status() != reqwest::StatusCode::CREATED
    {
        return Err(code_session_error(response, "create code session").await);
    }

    let value = response.json::<Value>().await?;
    let session_id = value
        .get("session")
        .and_then(|session| session.get("id"))
        .and_then(Value::as_str)
        .filter(|id| id.starts_with("cse_"))
        .ok_or_else(|| {
            CodeSessionApiError::Message(
                "code session create response missing cse_* session.id".to_string(),
            )
        })?;
    Ok(session_id.to_string())
}

pub async fn fetch_remote_credentials(
    api_base_url: &str,
    session_id: &str,
    access_token: &str,
    trusted_device_token: Option<&str>,
) -> Result<RemoteCredentials, CodeSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    if session_id.trim().is_empty() {
        return Err(CodeSessionApiError::Message(
            "fetch_remote_credentials requires a session ID".to_string(),
        ));
    }
    let url = format!("{base_url}/v1/code/sessions/{session_id}/bridge");
    let mut request = add_code_session_headers(reqwest::Client::new().post(url), access_token);
    if let Some(token) = trusted_device_token
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        request = request.header("X-Trusted-Device-Token", token);
    }
    let response = request
        .timeout(CODE_SESSION_API_TIMEOUT)
        .json(&json!({}))
        .send()
        .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(code_session_error(response, "fetch remote credentials").await);
    }

    let value = response.json::<Value>().await?;
    parse_remote_credentials(value)
}

pub fn build_ccr_v2_sdk_url(
    api_base_url: &str,
    session_id: &str,
) -> Result<String, CodeSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    if session_id.trim().is_empty() {
        return Err(CodeSessionApiError::Message(
            "build_ccr_v2_sdk_url requires a session ID".to_string(),
        ));
    }
    Ok(format!("{base_url}/v1/code/sessions/{session_id}"))
}

fn parse_remote_credentials(value: Value) -> Result<RemoteCredentials, CodeSessionApiError> {
    let worker_jwt = value
        .get("worker_jwt")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| CodeSessionApiError::Message("missing worker_jwt".to_string()))?
        .to_string();
    let api_base_url = value
        .get("api_base_url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .ok_or_else(|| CodeSessionApiError::Message("missing api_base_url".to_string()))?
        .to_string();
    let expires_in = value
        .get("expires_in")
        .and_then(Value::as_u64)
        .ok_or_else(|| CodeSessionApiError::Message("missing expires_in".to_string()))?;
    let worker_epoch = match value.get("worker_epoch") {
        Some(Value::Number(number)) => number.as_u64(),
        Some(Value::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
    .ok_or_else(|| CodeSessionApiError::Message("invalid worker_epoch".to_string()))?;

    Ok(RemoteCredentials {
        worker_jwt,
        api_base_url,
        expires_in,
        worker_epoch,
    })
}

fn normalized_api_base_url(api_base_url: &str) -> Result<String, CodeSessionApiError> {
    let base_url = api_base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        Err(CodeSessionApiError::Message(
            "code session API base URL is empty".to_string(),
        ))
    } else {
        Ok(base_url)
    }
}

fn add_code_session_headers(
    builder: reqwest::RequestBuilder,
    access_token: &str,
) -> reqwest::RequestBuilder {
    builder
        .bearer_auth(access_token)
        .header("content-type", "application/json")
        .header("anthropic-version", ANTHROPIC_VERSION)
}

async fn code_session_error(response: reqwest::Response, action: &str) -> CodeSessionApiError {
    let status = response.status();
    let api_message = response
        .json::<Value>()
        .await
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .filter(|message| !message.trim().is_empty());
    CodeSessionApiError::HttpStatus {
        action: action.to_string(),
        status,
        message: api_message
            .map(|message| format!(": {message}"))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        anthropic_version: Option<String>,
        content_type: Option<String>,
        trusted_device_token: Option<String>,
        body: String,
    }

    #[tokio::test]
    async fn create_code_session_posts_reference_body_and_headers() {
        let (base_url, request) = spawn_mock_code_session_server(
            201,
            json!({ "session": { "id": "cse_session_1" } }).to_string(),
        )
        .await;

        let session_id = create_code_session(
            &base_url,
            "access-token",
            "Remote bridge",
            &["ccr-mirror".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(session_id, "cse_session_1");
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/code/sessions");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(
            request.anthropic_version.as_deref(),
            Some(ANTHROPIC_VERSION)
        );
        assert!(request
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["title"], "Remote bridge");
        assert_eq!(body["bridge"], json!({}));
        assert_eq!(body["tags"][0], "ccr-mirror");
    }

    #[tokio::test]
    async fn create_code_session_rejects_non_cse_response_id() {
        let (base_url, _request) = spawn_mock_code_session_server(
            200,
            json!({ "session": { "id": "session_1" } }).to_string(),
        )
        .await;

        let error = create_code_session(&base_url, "access-token", "Remote bridge", &[])
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("missing cse_* session.id"));
    }

    #[tokio::test]
    async fn fetch_remote_credentials_posts_bridge_and_parses_string_epoch() {
        let (base_url, request) = spawn_mock_code_session_server(
            200,
            json!({
                "worker_jwt": "worker-token",
                "api_base_url": "https://worker.example",
                "expires_in": 3600,
                "worker_epoch": "42"
            })
            .to_string(),
        )
        .await;

        let credentials = fetch_remote_credentials(
            &base_url,
            "cse_session_1",
            "access-token",
            Some("trusted-token"),
        )
        .await
        .unwrap();

        assert_eq!(
            credentials,
            RemoteCredentials {
                worker_jwt: "worker-token".to_string(),
                api_base_url: "https://worker.example".to_string(),
                expires_in: 3600,
                worker_epoch: 42,
            }
        );
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/code/sessions/cse_session_1/bridge");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(
            request.trusted_device_token.as_deref(),
            Some("trusted-token")
        );
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body, json!({}));
    }

    #[tokio::test]
    async fn fetch_remote_credentials_reports_typed_unauthorized_status() {
        let (base_url, _request) = spawn_mock_code_session_server(
            401,
            json!({ "error": { "message": "Authentication failed" } }).to_string(),
        )
        .await;

        let error = fetch_remote_credentials(&base_url, "cse_session_1", "stale-token", None)
            .await
            .unwrap_err();

        assert_eq!(error.http_status(), Some(StatusCode::UNAUTHORIZED));
        assert!(error.is_auth_failure_status());
        let message = error.to_string();
        assert!(message.contains("fetch remote credentials failed: 401 Unauthorized"));
        assert!(message.contains("Authentication failed"));
    }

    #[test]
    fn build_ccr_v2_sdk_url_trims_base_slash() {
        assert_eq!(
            build_ccr_v2_sdk_url("https://api.example/", "cse_session_1").unwrap(),
            "https://api.example/v1/code/sessions/cse_session_1"
        );
    }

    async fn spawn_mock_code_session_server(
        status: u16,
        response_body: String,
    ) -> (String, Arc<StdMutex<Option<RecordedRequest>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let request = Arc::new(StdMutex::new(None));
        let shared_request = request.clone();

        tokio::spawn(async move {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            if let Ok(recorded) = read_http_request(&mut stream).await {
                *shared_request.lock().unwrap() = Some(recorded);
            }
            let _ = write_http_response(&mut stream, status, &response_body).await;
        });

        (format!("http://{}", address), request)
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
        let mut anthropic_version = None;
        let mut content_type = None;
        let mut trusted_device_token = None;
        let mut content_length = 0_usize;

        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().to_string();
            if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value);
            } else if name.eq_ignore_ascii_case("anthropic-version") {
                anthropic_version = Some(value);
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value);
            } else if name.eq_ignore_ascii_case("x-trusted-device-token") {
                trusted_device_token = Some(value);
            } else if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or_default();
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
            anthropic_version,
            content_type,
            trusted_device_token,
            body,
        })
    }

    fn find_header_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_http_response(
        stream: &mut TcpStream,
        status: u16,
        body: &str,
    ) -> std::io::Result<()> {
        let status_text = match status {
            200 => "OK",
            201 => "Created",
            401 => "Unauthorized",
            403 => "Forbidden",
            500 => "Internal Server Error",
            _ => "Status",
        };
        stream
            .write_all(
                format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    status_text,
                    body.len(),
                    body
                )
                .as_bytes(),
            )
            .await?;
        stream.flush().await
    }
}
