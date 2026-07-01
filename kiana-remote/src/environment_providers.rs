use crate::remote_session_manager::RemoteSessionApiError;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

const CCR_BYOC_BETA: &str = "ccr-byoc-2025-07-29";
const ENVIRONMENT_API_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentResource {
    pub kind: String,
    pub environment_id: String,
    pub name: String,
    pub created_at: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentListResponse {
    #[serde(default)]
    pub environments: Vec<EnvironmentResource>,
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub first_id: Option<String>,
    #[serde(default)]
    pub last_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvironmentSelectionInfo {
    pub available_environments: Vec<EnvironmentResource>,
    pub selected_environment: Option<EnvironmentResource>,
    pub selected_environment_source: Option<String>,
}

pub async fn fetch_environments(
    api_base_url: &str,
    org_uuid: &str,
    access_token: &str,
) -> Result<Vec<EnvironmentResource>, RemoteSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    let url = format!("{base_url}/v1/environment_providers");
    let response =
        add_environment_api_headers(reqwest::Client::new().get(url), access_token, org_uuid)
            .timeout(ENVIRONMENT_API_TIMEOUT)
            .send()
            .await?;

    if response.status() != reqwest::StatusCode::OK {
        return Err(RemoteSessionApiError::Message(format!(
            "Failed to fetch environments: {}",
            response.status()
        )));
    }

    Ok(response
        .json::<EnvironmentListResponse>()
        .await?
        .environments)
}

pub async fn create_default_cloud_environment(
    api_base_url: &str,
    org_uuid: &str,
    access_token: &str,
    name: &str,
) -> Result<EnvironmentResource, RemoteSessionApiError> {
    let base_url = normalized_api_base_url(api_base_url)?;
    let url = format!("{base_url}/v1/environment_providers/cloud/create");
    let response =
        add_environment_api_headers(reqwest::Client::new().post(url), access_token, org_uuid)
            .header("anthropic-beta", CCR_BYOC_BETA)
            .timeout(ENVIRONMENT_API_TIMEOUT)
            .json(&json!({
                "name": name,
                "kind": "anthropic_cloud",
                "description": "",
                "config": {
                    "environment_type": "anthropic",
                    "cwd": "/home/user",
                    "init_script": null,
                    "environment": {},
                    "languages": [
                        { "name": "python", "version": "3.11" },
                        { "name": "node", "version": "20" }
                    ],
                    "network_config": {
                        "allowed_hosts": [],
                        "allow_default_hosts": true
                    }
                }
            }))
            .send()
            .await?;

    if response.status() != reqwest::StatusCode::OK
        && response.status() != reqwest::StatusCode::CREATED
    {
        return Err(RemoteSessionApiError::Message(format!(
            "Failed to create default cloud environment: {}",
            response.status()
        )));
    }

    Ok(response.json::<EnvironmentResource>().await?)
}

pub fn select_environment(
    environments: &[EnvironmentResource],
    default_environment_id: Option<&str>,
) -> EnvironmentSelectionInfo {
    if environments.is_empty() {
        return EnvironmentSelectionInfo {
            available_environments: Vec::new(),
            selected_environment: None,
            selected_environment_source: None,
        };
    }

    let fallback = environments
        .iter()
        .find(|environment| environment.kind != "bridge")
        .or_else(|| environments.first())
        .cloned();
    let selected_from_default = default_environment_id
        .filter(|id| !id.trim().is_empty())
        .and_then(|id| {
            environments
                .iter()
                .find(|environment| environment.environment_id == id)
                .cloned()
        });

    EnvironmentSelectionInfo {
        available_environments: environments.to_vec(),
        selected_environment: selected_from_default.clone().or(fallback),
        selected_environment_source: selected_from_default.map(|_| "settings".to_string()),
    }
}

fn normalized_api_base_url(api_base_url: &str) -> Result<String, RemoteSessionApiError> {
    let base_url = api_base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        Err(RemoteSessionApiError::Message(
            "remote environment API base URL is empty".to_string(),
        ))
    } else {
        Ok(base_url)
    }
}

fn add_environment_api_headers(
    builder: reqwest::RequestBuilder,
    access_token: &str,
    org_uuid: &str,
) -> reqwest::RequestBuilder {
    builder
        .bearer_auth(access_token)
        .header("content-type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .header("x-organization-uuid", org_uuid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    #[derive(Debug, Clone)]
    struct RecordedRequest {
        method: String,
        path: String,
        authorization: Option<String>,
        anthropic_version: Option<String>,
        anthropic_beta: Option<String>,
        organization_uuid: Option<String>,
        content_type: Option<String>,
        body: String,
    }

    #[tokio::test]
    async fn fetch_environments_calls_reference_endpoint_and_headers() {
        let body = json!({
            "environments": [{
                "kind": "anthropic_cloud",
                "environment_id": "env-1",
                "name": "Claude Cloud",
                "created_at": "2026-06-15T00:00:00Z",
                "state": "active"
            }],
            "has_more": false,
            "first_id": "env-1",
            "last_id": "env-1"
        })
        .to_string();
        let (base_url, request) = spawn_mock_environment_api_server(200, body).await;

        let environments = fetch_environments(&base_url, "org-1", "access-token")
            .await
            .unwrap();

        assert_eq!(environments.len(), 1);
        assert_eq!(environments[0].environment_id, "env-1");
        assert_eq!(environments[0].kind, "anthropic_cloud");
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "GET");
        assert_eq!(request.path, "/v1/environment_providers");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta, None);
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
    }

    #[tokio::test]
    async fn create_default_cloud_environment_posts_reference_shape() {
        let body = json!({
            "kind": "anthropic_cloud",
            "environment_id": "env-1",
            "name": "Default Cloud",
            "created_at": "2026-06-15T00:00:00Z",
            "state": "active"
        })
        .to_string();
        let (base_url, request) = spawn_mock_environment_api_server(201, body).await;

        let environment =
            create_default_cloud_environment(&base_url, "org-1", "access-token", "Default Cloud")
                .await
                .unwrap();

        assert_eq!(environment.environment_id, "env-1");
        let request = request.lock().unwrap().clone().expect("request captured");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/v1/environment_providers/cloud/create");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer access-token")
        );
        assert_eq!(request.anthropic_version.as_deref(), Some("2023-06-01"));
        assert_eq!(request.anthropic_beta.as_deref(), Some(CCR_BYOC_BETA));
        assert_eq!(request.organization_uuid.as_deref(), Some("org-1"));
        assert!(request
            .content_type
            .as_deref()
            .unwrap_or_default()
            .starts_with("application/json"));
        let body: Value = serde_json::from_str(&request.body).unwrap();
        assert_eq!(body["name"], "Default Cloud");
        assert_eq!(body["kind"], "anthropic_cloud");
        assert_eq!(body["description"], "");
        assert_eq!(body["config"]["environment_type"], "anthropic");
        assert_eq!(body["config"]["cwd"], "/home/user");
        assert_eq!(body["config"]["languages"][0]["name"], "python");
        assert_eq!(body["config"]["languages"][1]["version"], "20");
        assert_eq!(
            body["config"]["network_config"]["allow_default_hosts"],
            true
        );
    }

    #[test]
    fn select_environment_prefers_configured_then_non_bridge() {
        let environments = vec![
            EnvironmentResource {
                kind: "bridge".to_string(),
                environment_id: "bridge-1".to_string(),
                name: "Bridge".to_string(),
                created_at: "2026-06-15T00:00:00Z".to_string(),
                state: "active".to_string(),
            },
            EnvironmentResource {
                kind: "anthropic_cloud".to_string(),
                environment_id: "cloud-1".to_string(),
                name: "Cloud".to_string(),
                created_at: "2026-06-15T00:00:00Z".to_string(),
                state: "active".to_string(),
            },
        ];

        let selected = select_environment(&environments, None);
        assert_eq!(
            selected
                .selected_environment
                .as_ref()
                .map(|environment| environment.environment_id.as_str()),
            Some("cloud-1")
        );
        assert_eq!(selected.selected_environment_source, None);

        let selected = select_environment(&environments, Some("bridge-1"));
        assert_eq!(
            selected
                .selected_environment
                .as_ref()
                .map(|environment| environment.environment_id.as_str()),
            Some("bridge-1")
        );
        assert_eq!(
            selected.selected_environment_source.as_deref(),
            Some("settings")
        );
    }

    async fn spawn_mock_environment_api_server(
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
        let mut anthropic_beta = None;
        let mut organization_uuid = None;
        let mut content_type = None;
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
            } else if name.eq_ignore_ascii_case("anthropic-beta") {
                anthropic_beta = Some(value);
            } else if name.eq_ignore_ascii_case("x-organization-uuid") {
                organization_uuid = Some(value);
            } else if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value);
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
            anthropic_beta,
            organization_uuid,
            content_type,
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
