use async_trait::async_trait;
use kiana_domain::{
    json_digest, ModelAssignment, ModelCallPermit, ModelCallSpec, ModelError, ModelMessage,
    ModelPurpose, ModelRequest, ModelResponseFormat, PreparedModelCall, RequestId, RoleSpec, RunId,
    SchemaVersion, TurnId,
};
use kiana_ports::{ModelBudgetPort, ModelClient, PortError};
use kiana_provider::{ConfigResolver, ProviderConfig, ProviderGateway, WorkspaceConfigTrust};
use serde_json::json;
use std::ffi::OsString;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    fn new() -> Self {
        let values = [
            "KIANA_PROVIDER",
            "KIANA_MODEL_PROFILES_JSON",
            "KIANA_STREAMING",
            "KIANA_OLLAMA_MODEL",
            "KIANA_OLLAMA_BASE_URL",
            "KIANA_MODEL_MAX_CONCURRENCY",
            "KIANA_MODEL_QUEUE_LIMIT",
            "CI06_SECRET_KEY",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        let restore = Self(values);
        for (key, _) in &restore.0 {
            std::env::remove_var(key);
        }
        restore
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

fn trust(revision: &str) -> WorkspaceConfigTrust<'_> {
    WorkspaceConfigTrust {
        project_trusted: true,
        admitted_revision: revision,
        current_revision: revision,
    }
}

fn overlay(model: &str) -> serde_json::Value {
    json!({"schema":"kiana.provider-config.v1","version":1,"model":model})
}

fn provider(base_url: &str) -> ProviderConfig {
    ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: None,
        base_url: Some(base_url.to_owned()),
        api_key: None,
    }
}

fn request_and_spec(role: RoleSpec) -> (ModelRequest, ModelCallSpec) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_millis() as u64;
    (
        ModelRequest {
            messages: vec![ModelMessage::user("CI06 request")],
            tools: Vec::new(),
            sandbox: "read-only".to_owned(),
        },
        ModelCallSpec {
            call_id: RequestId::new(),
            attempt_id: RequestId::new(),
            model_attempt_id: None,
            step_id: None,
            step: 1,
            purpose: ModelPurpose::Task,
            assignment: Some(ModelAssignment {
                schema: "kiana.model-assignment.v1".to_owned(),
                run_id: RunId::new(),
                turn_id: TurnId::new(),
                role_id: role.role_id,
                role_version: Some(role.version),
                catalog_version: Some(SchemaVersion::new(1, 0)),
                prompt_hash: Some(role.prompt_hash),
                input_schema: Some(role.input_schema),
                output_schema: Some(role.output_schema),
                profile: role.model_profile,
                project_root: "/repo".to_owned(),
                project_trusted: true,
                authority_revision: None,
                max_wall_time_ms: 10_000,
                runtime_budget: None,
            }),
            response_format: ModelResponseFormat::Text,
            replay: Vec::new(),
            deadline_unix_ms: now + 10_000,
        },
    )
}

fn prepare(gateway: &ProviderGateway) -> (PreparedModelCall, ModelCallPermit) {
    let (request, spec) = request_and_spec(RoleSpec::pm());
    let prepared = gateway.prepare_call(request, spec).expect("prepared call");
    let permit = ModelCallPermit {
        schema: "kiana.model-call-permit.v1".to_owned(),
        permit_id: RequestId::new(),
        run_id: prepared.spec.assignment.as_ref().unwrap().run_id,
        attempt_id: prepared.spec.attempt_id,
        request_hash: prepared.request_hash.clone(),
        expires_at_unix_ms: prepared.spec.deadline_unix_ms,
        route_digest: Some(prepared.route.digest()),
        configuration_revision: Some(prepared.route.configuration_revision.clone()),
        authority_revision: None,
        credential_revision: prepared.credential_revision.clone(),
        provider_account: prepared.provider_account.clone(),
    };
    (prepared, permit)
}

#[derive(Default)]
struct CountingBudget {
    consumed: AtomicUsize,
    blocked: bool,
    entered: Notify,
    release: Notify,
}

#[async_trait]
impl ModelBudgetPort for CountingBudget {
    async fn reserve(&self, _: RunId, _: RequestId, _: u64) -> Result<(), PortError> {
        Ok(())
    }

    async fn settle(&self, _: RunId, _: RequestId, _: Option<u64>) -> Result<(), PortError> {
        Ok(())
    }

    async fn consume_prepared(
        &self,
        _: &PreparedModelCall,
        _: &ModelCallPermit,
    ) -> Result<(), PortError> {
        self.consumed.fetch_add(1, Ordering::SeqCst);
        if self.blocked {
            self.entered.notify_one();
            self.release.notified().await;
            return Err(PortError::Unavailable("fixture_budget_rejected".to_owned()));
        }
        Ok(())
    }
}

async fn read_request(stream: &mut TcpStream) -> serde_json::Value {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 2_048];
    let end = loop {
        let read = stream.read(&mut chunk).await.expect("request read");
        assert!(read > 0, "request closed before headers");
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        assert!(bytes.len() < 64 * 1024, "bounded headers");
    };
    let length = String::from_utf8_lossy(&bytes[..end])
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .and_then(|length| length.trim().parse::<usize>().ok())
        })
        .expect("content length");
    while bytes.len() < end + length {
        let read = stream.read(&mut chunk).await.expect("request body");
        assert!(read > 0, "request closed before body");
        bytes.extend_from_slice(&chunk[..read]);
    }
    serde_json::from_slice(&bytes[end..end + length]).expect("JSON request")
}

async fn respond(stream: &mut TcpStream, status: &str, payload: &str) {
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/x-ndjson\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len(),
    );
    stream
        .write_all(response.as_bytes())
        .await
        .expect("response write");
    stream.shutdown().await.expect("response close");
}

fn valid_workspace() -> String {
    json!({
        "schema": "kiana.provider-config.v1",
        "version": 1,
        "provider": "ollama",
        "model": "fixture-model",
        "base_url": "http://127.0.0.1:22114",
        "api_key_env": "OLLAMA_OPTIONAL_KEY",
        "profiles": {
            "planning": {
                "provider": "ollama",
                "model": "planning-model",
                "base_url": "http://localhost:22115",
                "inherit_default": false
            }
        }
    })
    .to_string()
}

fn code(result: Result<kiana_provider::WorkspaceConfig, ModelError>) -> String {
    result.unwrap_err().code
}

#[test]
fn config_resolver_rejects_untrusted_and_invalid_workspace_inputs() {
    let raw = valid_workspace();
    assert_eq!(
        code(ConfigResolver::parse_workspace(&raw, false)),
        "config_workspace_untrusted"
    );

    let mut unknown = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    unknown["unexpected"] = json!(true);
    assert_eq!(
        code(ConfigResolver::parse_workspace(&unknown.to_string(), true)),
        "config_workspace_invalid"
    );

    let mut major = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    major["version"] = json!(2);
    assert_eq!(
        code(ConfigResolver::parse_workspace(&major.to_string(), true)),
        "config_workspace_schema_unsupported"
    );

    let mut endpoint = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    endpoint["base_url"] = json!("http://user:pass@example.invalid/v1?token=raw");
    assert_eq!(
        code(ConfigResolver::parse_workspace(&endpoint.to_string(), true)),
        "config_endpoint_credentials_or_query_denied"
    );

    let mut profile = serde_json::from_str::<serde_json::Value>(&raw).unwrap();
    profile["profiles"]["future"] = json!({
        "provider": "ollama",
        "model": "future-model"
    });
    assert_eq!(
        code(ConfigResolver::parse_workspace(&profile.to_string(), true)),
        "config_profile_invalid"
    );

    let oversized = "x".repeat(kiana_provider::MAX_WORKSPACE_CONFIG_BYTES + 1);
    assert_eq!(
        code(ConfigResolver::parse_workspace(&oversized, true)),
        "config_workspace_too_large"
    );
}

#[test]
fn config_resolver_produces_a_canonical_secret_free_snapshot() {
    let _env = EnvRestore::new();
    let trust = json_digest(&json!("trusted-project"));
    let snapshot = ConfigResolver::workspace_snapshot(&valid_workspace(), true, &trust)
        .expect("trusted workspace snapshot");
    snapshot.validate().expect("snapshot validates");
    let encoded = serde_json::to_string(&snapshot).expect("snapshot serializes");
    assert!(!encoded.contains("raw"));
    assert!(encoded.contains("api_key_env"));
    assert_eq!(
        snapshot.config_revision,
        json_digest(&snapshot.effective_non_secret_config)
    );

    let config = ProviderConfig {
        provider: Some("ollama".to_owned()),
        model: Some("resolver-model".to_owned()),
        base_url: Some("http://127.0.0.1:22114".to_owned()),
        api_key: None,
    };
    let first = ProviderGateway::from_env(config.clone()).expect("gateway via resolver");
    let second = ProviderGateway::from_env(config).expect("same gateway via resolver");
    assert_eq!(
        first.configuration_snapshot().unwrap(),
        second.configuration_snapshot().unwrap()
    );
}

#[test]
fn workspace_resolution_applies_precedence_and_opaque_secret_refs() {
    let _env = EnvRestore::new();
    let revision = json_digest(&json!("server-trust"));
    let mut raw = overlay("workspace-model");
    raw["provider"] = json!("ollama");
    raw["base_url"] = json!("http://127.0.0.1:22114");
    raw["api_key_env"] = json!("CI06_SECRET_KEY");
    raw["profiles"] = json!({"planning": {
        "provider":"ollama", "model":"workspace-plan", "base_url":"http://127.0.0.1:22115"
    }});
    std::env::set_var("CI06_SECRET_KEY", "CI06_SECRET_SENTINEL");
    let gateway = ProviderGateway::from_workspace(
        ProviderConfig::default(),
        &raw.to_string(),
        trust(&revision),
    )
    .expect("trusted project defaults");
    let snapshot = gateway.configuration_snapshot().unwrap();
    snapshot.validate().unwrap();
    let default = snapshot
        .profiles
        .iter()
        .find(|profile| profile.profile == "default")
        .unwrap();
    assert_eq!(default.route.model_id, "workspace-model");
    assert!(default.credential_ref.is_some());
    assert_eq!(prepare(&gateway).0.wire_body["model"], "workspace-plan");
    let (request, spec) = request_and_spec(RoleSpec::builder());
    assert_eq!(
        gateway.prepare_call(request, spec).unwrap_err().code,
        "model_profile_unconfigured"
    );
    let workspace = gateway.workspace_configuration_snapshot().unwrap().unwrap();
    assert_eq!(workspace.project_trust_revision, revision);
    let catalog = gateway.catalog().to_string();
    assert!(!catalog.contains("CI06_SECRET_SENTINEL"));
    assert!(!serde_json::to_string(&workspace)
        .unwrap()
        .contains("CI06_SECRET_SENTINEL"));

    std::env::set_var("KIANA_OLLAMA_MODEL", "environment-model");
    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        json!({"planning": {
            "provider":"ollama", "model":"environment-plan", "base_url":"http://127.0.0.1:22116"
        }})
        .to_string(),
    );
    let environment = ProviderGateway::from_workspace(
        ProviderConfig::default(),
        &raw.to_string(),
        trust(&revision),
    )
    .unwrap();
    let explicit = ProviderGateway::from_workspace(
        ProviderConfig {
            model: Some("explicit-model".to_owned()),
            ..ProviderConfig::default()
        },
        &raw.to_string(),
        trust(&revision),
    )
    .unwrap();
    for (gateway, model) in [
        (&environment, "environment-model"),
        (&explicit, "explicit-model"),
    ] {
        let snapshot = gateway.configuration_snapshot().unwrap();
        assert_eq!(
            snapshot
                .profiles
                .iter()
                .find(|profile| profile.profile == "default")
                .unwrap()
                .route
                .model_id,
            model
        );
        assert_eq!(prepare(gateway).0.wire_body["model"], "environment-plan");
    }
}

#[test]
fn reload_rejects_trust_drift_stale_revision_and_failed_candidates_without_mutation() {
    let _env = EnvRestore::new();
    let config = provider("http://127.0.0.1:22114");
    let gateway = ProviderGateway::from_env(config.clone()).unwrap();
    let before = gateway.configuration_snapshot().unwrap();
    let revision = json_digest(&json!("server-trust"));
    let newer_trust = json_digest(&json!("newer-server-trust"));
    for (authority, code) in [
        (
            WorkspaceConfigTrust {
                project_trusted: false,
                ..trust(&revision)
            },
            "config_workspace_untrusted",
        ),
        (
            WorkspaceConfigTrust {
                current_revision: &newer_trust,
                ..trust(&revision)
            },
            "config_project_trust_revision_changed",
        ),
        (
            trust("not-a-digest"),
            "config_project_trust_revision_invalid",
        ),
    ] {
        let error = gateway
            .reload_workspace(
                config.clone(),
                "not JSON",
                authority,
                &before.snapshot_digest,
            )
            .unwrap_err();
        assert_eq!(error.code, code, "trust is checked before parsing");
        assert_eq!(gateway.configuration_snapshot().unwrap(), before);
    }
    let mut invalid = overlay("candidate-default");
    invalid["profiles"] =
        json!({"planning":{"provider":"unsupported-provider","model":"candidate-plan"}});
    let error = gateway
        .reload_workspace(
            config.clone(),
            &invalid.to_string(),
            trust(&revision),
            &before.snapshot_digest,
        )
        .unwrap_err();
    assert_eq!(error.code, "model_provider_unsupported");
    assert_eq!(gateway.configuration_snapshot().unwrap(), before);
    assert!(gateway
        .workspace_configuration_snapshot()
        .unwrap()
        .is_none());

    std::env::set_var("KIANA_MODEL_MAX_CONCURRENCY", "3");
    let error = gateway
        .reload_workspace(
            config.clone(),
            &overlay("new-model").to_string(),
            trust(&revision),
            &before.snapshot_digest,
        )
        .unwrap_err();
    assert_eq!(error.code, "config_capacity_policy_changed");
    assert_eq!(gateway.configuration_snapshot().unwrap(), before);
    std::env::remove_var("KIANA_MODEL_MAX_CONCURRENCY");

    let raw = overlay("accepted-model").to_string();
    let accepted = gateway
        .reload_workspace(
            config.clone(),
            &raw,
            trust(&revision),
            &before.snapshot_digest,
        )
        .unwrap();
    assert_ne!(accepted.snapshot_digest, before.snapshot_digest);
    assert_eq!(
        gateway
            .reload_workspace(
                config.clone(),
                &raw,
                trust(&revision),
                &before.snapshot_digest
            )
            .unwrap_err()
            .code,
        "config_snapshot_revision_changed"
    );
    assert_eq!(gateway.configuration_snapshot().unwrap(), accepted);
    let same_content = gateway
        .reload_workspace(config, &raw, trust(&revision), &accepted.snapshot_digest)
        .unwrap();
    assert_ne!(
        same_content.snapshot_digest, accepted.snapshot_digest,
        "no ABA revival of an earlier generation"
    );
}

#[tokio::test]
async fn changing_another_profile_fences_old_permits_before_budget_or_network() {
    let _env = EnvRestore::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = provider(&format!("http://{}", listener.local_addr().unwrap()));
    let revision = json_digest(&json!("server-trust"));
    let mut raw = overlay("unchanged-plan");
    raw["profiles"] = json!({"planning":{"provider":"","model":"","inherit_default":true}});
    let gateway =
        ProviderGateway::from_workspace(config.clone(), &raw.to_string(), trust(&revision))
            .unwrap();
    let before = gateway.configuration_snapshot().unwrap();
    let (prepared, permit) = prepare(&gateway);
    let old_route = prepared.route.clone();
    raw["profiles"][RoleSpec::builder().model_profile] = json!({
        "provider":"ollama","model":"new-builder","base_url":"http://127.0.0.1:22116"
    });
    gateway
        .reload_workspace(
            config,
            &raw.to_string(),
            trust(&revision),
            &before.snapshot_digest,
        )
        .unwrap();
    let new_route = prepare(&gateway).0.route;
    assert_eq!(new_route.provider_id, old_route.provider_id);
    assert_eq!(new_route.model_id, old_route.model_id);
    assert_eq!(new_route.connection_id, old_route.connection_id);
    assert_ne!(
        new_route.configuration_revision,
        old_route.configuration_revision
    );
    let budget = CountingBudget::default();
    let error = gateway
        .complete_admitted(prepared, permit, &budget, &mut |_| Ok(()))
        .await
        .unwrap_err();
    assert_eq!(error.code, "model_route_changed_after_admission");
    assert_eq!(budget.consumed.load(Ordering::SeqCst), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "stale permit must not reach provider"
    );
}

#[tokio::test]
async fn reload_is_busy_during_admission_and_succeeds_after_the_pinned_attempt_finishes() {
    let _env = EnvRestore::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = provider(&format!("http://{}", listener.local_addr().unwrap()));
    let gateway = Arc::new(ProviderGateway::from_env(config.clone()).unwrap());
    let revision = json_digest(&json!("server-trust"));
    let before = gateway.configuration_snapshot().unwrap();
    let (stale_prepared, stale_permit) = prepare(&gateway);
    let budget = Arc::new(CountingBudget {
        blocked: true,
        ..CountingBudget::default()
    });
    let (prepared, permit) = prepare(&gateway);
    let attempt = tokio::spawn({
        let gateway = gateway.clone();
        let budget = budget.clone();
        async move {
            gateway
                .complete_admitted(prepared, permit, budget.as_ref(), &mut |_| Ok(()))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), budget.entered.notified())
        .await
        .expect("admission entered");
    let raw = overlay("after-admission").to_string();
    assert_eq!(
        gateway
            .reload_workspace(
                config.clone(),
                &raw,
                trust(&revision),
                &before.snapshot_digest
            )
            .unwrap_err()
            .code,
        "config_reload_busy"
    );
    assert_eq!(gateway.configuration_snapshot().unwrap(), before);
    budget.release.notify_one();
    assert_eq!(
        attempt.await.unwrap().unwrap_err().code,
        "port_unavailable:fixture_budget_rejected"
    );
    let after = gateway
        .reload_workspace(
            config.clone(),
            &raw,
            trust(&revision),
            &before.snapshot_digest,
        )
        .unwrap();
    assert_ne!(before.snapshot_digest, after.snapshot_digest);
    let cancelled_budget = Arc::new(CountingBudget {
        blocked: true,
        ..CountingBudget::default()
    });
    let (prepared, permit) = prepare(&gateway);
    let cancelled = tokio::spawn({
        let gateway = gateway.clone();
        let budget = cancelled_budget.clone();
        async move {
            gateway
                .complete_admitted(prepared, permit, budget.as_ref(), &mut |_| Ok(()))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), cancelled_budget.entered.notified())
        .await
        .expect("cancelled admission entered");
    cancelled.abort();
    assert!(cancelled.await.unwrap_err().is_cancelled());
    gateway
        .reload_workspace(config, &raw, trust(&revision), &after.snapshot_digest)
        .expect("cancel releases its pinned generation");
    let rejected_budget = CountingBudget::default();
    assert_eq!(
        gateway
            .complete_admitted(stale_prepared, stale_permit, &rejected_budget, &mut |_| Ok(
                ()
            ))
            .await
            .unwrap_err()
            .code,
        "model_route_changed_after_admission"
    );
    assert_eq!(rejected_budget.consumed.load(Ordering::SeqCst), 0);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn reloaded_model_is_the_only_model_sent_and_reload_is_busy_during_transport() {
    let _env = EnvRestore::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = provider(&format!("http://{}", listener.local_addr().unwrap()));
    let gateway = Arc::new(ProviderGateway::from_env(config.clone()).unwrap());
    let revision = json_digest(&json!("server-trust"));
    let initial = gateway.configuration_snapshot().unwrap();
    let raw = overlay("actually-reloaded-model").to_string();
    let active = gateway
        .reload_workspace(
            config.clone(),
            &raw,
            trust(&revision),
            &initial.snapshot_digest,
        )
        .unwrap();
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let server = tokio::spawn({
        let entered = entered.clone();
        let release = release.clone();
        async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let request = read_request(&mut stream).await;
            entered.notify_one();
            release.notified().await;
            respond(&mut stream, "200 OK", "{\"model\":\"actually-reloaded-model\",\"message\":{\"role\":\"assistant\",\"content\":\"fixture-ok\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n").await;
            request
        }
    });
    let budget = Arc::new(CountingBudget::default());
    let (prepared, permit) = prepare(&gateway);
    let attempt = tokio::spawn({
        let gateway = gateway.clone();
        let budget = budget.clone();
        async move {
            gateway
                .complete_admitted(prepared, permit, budget.as_ref(), &mut |_| Ok(()))
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), entered.notified())
        .await
        .expect("network request entered");
    assert_eq!(
        gateway
            .reload_workspace(
                config.clone(),
                &overlay("next-model").to_string(),
                trust(&revision),
                &active.snapshot_digest
            )
            .unwrap_err()
            .code,
        "config_reload_busy"
    );
    assert_eq!(gateway.configuration_snapshot().unwrap(), active);
    release.notify_one();
    assert_eq!(attempt.await.unwrap().unwrap().output.text, "fixture-ok");
    assert_eq!(server.await.unwrap()["model"], "actually-reloaded-model");
    assert_eq!(budget.consumed.load(Ordering::SeqCst), 1);
    gateway
        .reload_workspace(
            config,
            &overlay("next-model").to_string(),
            trust(&revision),
            &active.snapshot_digest,
        )
        .expect("idle reload after effect");
}

#[tokio::test]
async fn removing_and_restoring_a_scope_preserves_its_open_circuit() {
    let _env = EnvRestore::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = provider(&format!("http://{}", listener.local_addr().unwrap()));
    let gateway = ProviderGateway::from_env(config.clone()).unwrap();
    let server = tokio::spawn(async move {
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_request(&mut stream).await;
            respond(&mut stream, "503 Service Unavailable", "{}").await;
        }
        listener
    });
    let budget = CountingBudget::default();
    for _ in 0..3 {
        let (prepared, permit) = prepare(&gateway);
        let error = gateway
            .complete_admitted(prepared, permit, &budget, &mut |_| Ok(()))
            .await
            .unwrap_err();
        assert_eq!(error.code, "provider_http_503");
    }
    let listener = server.await.unwrap();
    let revision = json_digest(&json!("server-trust"));
    let before = gateway.configuration_snapshot().unwrap();
    let away = gateway
        .reload_workspace(
            provider("http://127.0.0.1:22199"),
            &overlay("other-scope").to_string(),
            trust(&revision),
            &before.snapshot_digest,
        )
        .unwrap();
    gateway
        .reload_workspace(
            config,
            &overlay("new-model-same-scope").to_string(),
            trust(&revision),
            &away.snapshot_digest,
        )
        .unwrap();
    let (prepared, permit) = prepare(&gateway);
    assert_eq!(
        gateway
            .complete_admitted(prepared, permit, &budget, &mut |_| Ok(()))
            .await
            .unwrap_err()
            .code,
        "provider_circuit_open"
    );
    assert_eq!(
        budget.consumed.load(Ordering::SeqCst),
        4,
        "circuit remains a transport capacity fence after model admission"
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(50), listener.accept())
            .await
            .is_err(),
        "reload must not reset open health and reach provider"
    );
}

#[tokio::test]
async fn reload_preserves_the_shared_token_capacity_window() {
    let _env = EnvRestore::new();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let config = provider(&base);
    std::env::set_var(
        "KIANA_MODEL_PROFILES_JSON",
        json!({"planning": {
            "provider":"ollama", "model":"large-reservation", "base_url":base,
            "capabilities":{"tools":false,"context_window":1_000_000,"max_output":600_000}
        }})
        .to_string(),
    );
    let gateway = ProviderGateway::from_env(config.clone()).unwrap();
    let requests = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn({
        let requests = requests.clone();
        async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                requests.fetch_add(1, Ordering::SeqCst);
                respond(&mut stream, "200 OK", "{\"model\":\"large-reservation\",\"message\":{\"role\":\"assistant\",\"content\":\"fixture-ok\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n").await;
            }
        }
    });
    let budget = CountingBudget::default();
    let (prepared, permit) = prepare(&gateway);
    assert!(prepared.budget.total > 500_000);
    let minute = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis()
            / 60_000
    };
    let mut window = minute();
    gateway
        .complete_admitted(prepared, permit, &budget, &mut |_| Ok(()))
        .await
        .unwrap();
    let revision = json_digest(&json!("server-trust"));
    let mut successes = 1;
    for _ in 0..2 {
        let before = gateway.configuration_snapshot().unwrap();
        gateway
            .reload_workspace(
                config.clone(),
                &overlay("default-changed").to_string(),
                trust(&revision),
                &before.snapshot_digest,
            )
            .unwrap();
        let (prepared, permit) = prepare(&gateway);
        match gateway
            .complete_admitted(prepared, permit, &budget, &mut |_| Ok(()))
            .await
        {
            Err(error) => {
                assert_eq!(error.code, "provider_capacity_quota_exhausted");
                assert_eq!(
                    requests.load(Ordering::SeqCst),
                    successes,
                    "no request after retained quota exhaustion"
                );
                assert_eq!(budget.consumed.load(Ordering::SeqCst), successes + 1);
                server.abort();
                assert!(server.await.unwrap_err().is_cancelled());
                return;
            }
            Ok(_) => {
                let current = minute();
                assert_ne!(
                    current, window,
                    "only a real UTC minute rollover can renew capacity"
                );
                window = current;
                successes += 1;
            }
        }
    }
    server.abort();
    panic!("capacity must reject another reservation within one fixed minute");
}
