use async_trait::async_trait;
use kiana_domain::{
    json_digest, ModelAssignment, ModelCallPermit, ModelCallSpec, ModelPurpose, ModelRequest,
    ModelResponseFormat, PreparedModelCall, RequestId, RoleSpec, RunId, SchemaVersion, TurnId,
};
use kiana_ports::{ModelBudgetPort, ModelClient, PortError};
use kiana_provider::{ProviderConfig, ProviderGateway};
use std::ffi::OsString;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new(keys: &[&'static str]) -> Self {
        let values = keys
            .iter()
            .map(|key| (*key, std::env::var_os(key)))
            .collect::<Vec<_>>();
        for key in keys {
            std::env::remove_var(key);
        }
        Self(values)
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in &self.0 {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[derive(Default)]
struct PermitOnlyBudget;

#[async_trait]
impl ModelBudgetPort for PermitOnlyBudget {
    async fn reserve(
        &self,
        _run_id: RunId,
        _request_id: RequestId,
        _tokens: u64,
    ) -> Result<(), PortError> {
        Ok(())
    }

    async fn settle(
        &self,
        _run_id: RunId,
        _request_id: RequestId,
        _tokens: Option<u64>,
    ) -> Result<(), PortError> {
        Ok(())
    }

    async fn consume_prepared(
        &self,
        _prepared: &kiana_domain::PreparedModelCall,
        _permit: &ModelCallPermit,
    ) -> Result<(), PortError> {
        Ok(())
    }
}

async fn read_http_request(stream: &mut TcpStream) -> (String, String) {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 2_048];
    let header_end = loop {
        let read = stream.read(&mut chunk).await.expect("request read");
        assert!(read > 0, "request closed before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(bytes.len() < 64 * 1024, "request headers too large");
    };
    let headers = String::from_utf8_lossy(&bytes[..header_end]).to_string();
    let content_length = headers
        .lines()
        .find_map(|line| {
            line.strip_prefix("Content-Length:")
                .or_else(|| line.strip_prefix("content-length:"))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut chunk).await.expect("request body read");
        assert!(read > 0, "request closed before body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    (
        headers,
        String::from_utf8_lossy(&bytes[header_end..header_end + content_length]).to_string(),
    )
}

/// Build one server-owned network call and its admission permit against the current snapshot.
///
/// The gateway rejects `ModelCallSpec::assignment == None` for a network route, so the fixture
/// supplies a validated PM assignment whose profile matches the resolved connection.
fn prepare_network_call(gateway: &ProviderGateway) -> (PreparedModelCall, ModelCallPermit) {
    let request = ModelRequest {
        messages: vec![kiana_domain::ModelMessage::user("CI08 request")],
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    let role = RoleSpec::pm();
    let run_id = RunId::new();
    let assignment = ModelAssignment {
        schema: "kiana.model-assignment.v1".to_owned(),
        run_id,
        turn_id: TurnId::new(),
        role_id: role.role_id.clone(),
        role_version: Some(role.version),
        catalog_version: Some(SchemaVersion::new(1, 0)),
        prompt_hash: Some(role.prompt_hash.clone()),
        input_schema: Some(role.input_schema.clone()),
        output_schema: Some(role.output_schema.clone()),
        profile: role.model_profile.clone(),
        project_root: "/repo".to_owned(),
        project_trusted: true,
        authority_revision: None,
        max_wall_time_ms: 10_000,
        runtime_budget: None,
    };
    assignment.validate().expect("server model assignment");
    let prepared = gateway
        .prepare_call(
            request,
            ModelCallSpec {
                call_id: RequestId::new(),
                attempt_id: RequestId::new(),
                model_attempt_id: None,
                step_id: None,
                step: 1,
                purpose: ModelPurpose::Task,
                assignment: Some(assignment),
                response_format: ModelResponseFormat::Text,
                replay: Vec::new(),
                deadline_unix_ms: now + 10_000,
            },
        )
        .expect("prepared request");
    let permit = ModelCallPermit {
        schema: "kiana.model-call-permit.v1".to_owned(),
        permit_id: RequestId::new(),
        run_id,
        attempt_id: prepared.spec.attempt_id,
        request_hash: prepared.request_hash.clone(),
        expires_at_unix_ms: now + 10_000,
        route_digest: Some(prepared.route.digest()),
        configuration_revision: Some(prepared.route.configuration_revision.clone()),
        authority_revision: None,
        credential_revision: prepared.credential_revision.clone(),
        provider_account: prepared.provider_account.clone(),
    };
    (prepared, permit)
}

#[tokio::test]
async fn fake_provider_receives_one_opaque_account_binding_and_no_secret_in_body() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("listener address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("provider connection");
        let request = read_http_request(&mut stream).await;
        let payload = r#"{"id":"ci08-response","choices":[{"message":{"role":"assistant","content":"fixture-ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            payload.len(),
            payload
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("provider response");
        stream.shutdown().await.expect("provider shutdown");
        request
    });

    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("openai".to_owned()),
        model: Some("gpt-4o-mini".to_owned()),
        base_url: Some(format!("http://{address}/v1")),
        api_key: Some("CI08_SECRET_SENTINEL".to_owned()),
    })
    .expect("provider gateway");
    let (prepared, permit) = prepare_network_call(&gateway);
    let budget = PermitOnlyBudget;
    let mut deltas = Vec::new();
    let reply = gateway
        .complete_admitted(prepared, permit, &budget, &mut |delta| {
            deltas.push(delta);
            Ok(())
        })
        .await
        .expect("fake provider reply");
    assert_eq!(reply.output.text, "fixture-ok");
    let (headers, body) = server.await.expect("provider task");
    let headers = headers.to_ascii_lowercase();
    assert_eq!(headers.matches("x-kiana-provider-account").count(), 1);
    assert!(headers.contains(&format!(
        "x-kiana-provider-account: {}",
        json_digest(&serde_json::json!({"provider":"openai","connection":"default"}))
    )));
    assert_eq!(
        headers
            .matches("authorization: bearer ci08_secret_sentinel")
            .count(),
        1
    );
    assert!(!body.contains("CI08_SECRET_SENTINEL"));
    assert!(deltas.iter().any(|delta| matches!(
        delta,
        kiana_domain::ModelDelta::Text { text } if text == "fixture-ok"
    )));
}

/// A provider-side redirect must fail closed: the request is sent exactly once, the redirect
/// `Location` is never followed, and an ambient `HTTP_PROXY` is ignored, so no second host ever
/// sees the request (or the credential).  The `Location` target points back at the bogus proxy
/// listener, which must observe zero connections whether the gateway honored the redirect or the
/// proxy environment variable.
#[tokio::test]
async fn fake_provider_redirect_is_rejected_and_ambient_proxy_is_ignored() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "NO_PROXY",
        "no_proxy",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    // A bogus ambient proxy.  Honoring `HTTP_PROXY`/`HTTPS_PROXY` would connect here and leak the
    // Authorization header to a third party; the client is built with `no_proxy()` so it must not.
    let proxy_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("proxy listener");
    proxy_listener
        .set_nonblocking(true)
        .expect("proxy listener nonblocking");
    let proxy_address = proxy_listener.local_addr().expect("proxy address");
    std::env::set_var("HTTP_PROXY", format!("http://{proxy_address}"));
    std::env::set_var("HTTPS_PROXY", format!("http://{proxy_address}"));
    std::env::set_var("ALL_PROXY", format!("http://{proxy_address}"));

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("listener address");
    let redirect_target = format!("http://{proxy_address}/redirected");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("provider connection");
        let request = read_http_request(&mut stream).await;
        let response = format!(
            "HTTP/1.1 302 Found\r\nLocation: {redirect_target}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("redirect response");
        stream.shutdown().await.expect("provider shutdown");
        request
    });

    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("openai".to_owned()),
        model: Some("gpt-4o-mini".to_owned()),
        base_url: Some(format!("http://{address}/v1")),
        api_key: Some("CI08_SECRET_SENTINEL".to_owned()),
    })
    .expect("provider gateway");
    let (prepared, permit) = prepare_network_call(&gateway);
    let budget = PermitOnlyBudget;
    let mut deltas = Vec::new();
    let error = gateway
        .complete_admitted(prepared, permit, &budget, &mut |delta| {
            deltas.push(delta);
            Ok(())
        })
        .await
        .expect_err("a redirect must not be followed");
    assert_eq!(error.code, "provider_http_302");
    assert!(error.request_sent, "the single request was already sent");
    assert!(deltas.is_empty(), "no reply deltas on a redirect");
    // The public error is secret-free: no Authorization value reaches the error projection.
    let encoded = serde_json::to_string(&error).expect("error json");
    assert!(!encoded.contains("CI08_SECRET_SENTINEL"));
    // Neither the redirect target nor the ambient proxy was ever contacted.
    match proxy_listener.accept() {
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock),
        Ok(_) => panic!("ambient proxy or redirect target was contacted"),
    }

    let (headers, body) = server.await.expect("provider task");
    let headers = headers.to_ascii_lowercase();
    assert_eq!(
        headers
            .matches("authorization: bearer ci08_secret_sentinel")
            .count(),
        1
    );
    assert!(!body.contains("CI08_SECRET_SENTINEL"));
}

/// The catalog, resolved snapshot, and workspace snapshot are the values a diagnostic or receipt
/// may surface; none of them may contain the raw credential even though the secret was supplied.
#[tokio::test]
async fn gateway_snapshot_and_catalog_never_expose_raw_credential() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("openai".to_owned()),
        model: Some("gpt-4o-mini".to_owned()),
        base_url: Some("http://127.0.0.1:9/v1".to_owned()),
        api_key: Some("CI08_SECRET_SENTINEL".to_owned()),
    })
    .expect("provider gateway");
    let catalog = serde_json::to_string(&gateway.catalog()).expect("catalog json");
    assert!(!catalog.contains("CI08_SECRET_SENTINEL"));
    let snapshot = gateway
        .configuration_snapshot()
        .expect("configuration snapshot");
    let encoded = serde_json::to_string(&snapshot).expect("snapshot json");
    assert!(!encoded.contains("CI08_SECRET_SENTINEL"));
    assert!(gateway
        .workspace_configuration_snapshot()
        .expect("workspace snapshot")
        .is_none());
}
