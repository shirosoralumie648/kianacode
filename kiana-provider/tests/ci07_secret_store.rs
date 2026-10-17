use async_trait::async_trait;
use kiana_domain::{
    ModelAssignment, ModelCallPermit, ModelCallSpec, ModelPurpose, ModelRequest,
    ModelResponseFormat, RequestId, RoleSpec, RunId, SchemaVersion, SecretRef, TurnId,
};
use kiana_ports::{ModelBudgetPort, ModelClient, PortError};
use kiana_provider::{ProviderConfig, ProviderGateway};
use std::ffi::OsString;
use tokio::net::TcpListener;

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

fn config(api_key: Option<&str>) -> ProviderConfig {
    ProviderConfig {
        provider: Some("openai".to_owned()),
        model: Some("gpt-4o-mini".to_owned()),
        base_url: Some("https://api.openai.com/v1".to_owned()),
        api_key: api_key.map(str::to_owned),
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

#[test]
fn explicit_credential_is_ref_only_in_configuration_and_catalog() {
    let _env = EnvRestore::new(&[
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    let gateway = ProviderGateway::from_env(config(Some("CI07_SECRET_SENTINEL")))
        .expect("explicit provider config");
    let snapshot = gateway.configuration_snapshot().expect("snapshot");
    let encoded = serde_json::to_string(&snapshot).expect("snapshot json");
    assert!(!encoded.contains("CI07_SECRET_SENTINEL"));
    assert!(snapshot.profiles.iter().all(|profile| profile
        .credential_ref
        .as_deref()
        .is_some_and(|value| { value.starts_with("sha256:") && value.len() == 71 })));
    let catalog = serde_json::to_string(&gateway.catalog()).expect("catalog json");
    assert!(!catalog.contains("CI07_SECRET_SENTINEL"));
}

#[test]
fn env_credential_reference_is_opaque_and_missing_resolution_fails_closed() {
    let _env = EnvRestore::new(&[
        "OPENAI_API_KEY",
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("OPENAI_API_KEY", "CI07_ENV_SECRET_SENTINEL");
    let gateway = ProviderGateway::from_env(config(None)).expect("env provider config");
    let snapshot = gateway.configuration_snapshot().expect("snapshot");
    assert!(!serde_json::to_string(&snapshot)
        .expect("snapshot json")
        .contains("CI07_ENV_SECRET_SENTINEL"));

    std::env::remove_var("OPENAI_API_KEY");
    let error = match ProviderGateway::from_env(config(None)) {
        Ok(_) => panic!("missing env secret must fail closed"),
        Err(error) => error,
    };
    assert_eq!(error.code, "model_credential_unavailable");
}

#[tokio::test]
async fn revoked_credential_fails_closed_before_any_provider_connection() {
    let _env = EnvRestore::new(&[
        "OPENAI_API_KEY",
        "KIANA_MODEL_PROFILES_JSON",
        "KIANA_STREAMING",
        "KIANA_MODEL_MAX_CONCURRENCY",
    ]);
    std::env::set_var("KIANA_STREAMING", "off");
    std::env::set_var("OPENAI_API_KEY", "CI07_REVOKED_SECRET_SENTINEL");

    // A listener that must never see a connection: if the credential fence failed, the admitted
    // call would reach the transport and connect here.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
    let address = listener.local_addr().expect("listener address");

    let gateway = ProviderGateway::from_env(ProviderConfig {
        provider: Some("openai".to_owned()),
        model: Some("gpt-4o-mini".to_owned()),
        base_url: Some(format!("http://{address}/v1")),
        api_key: None,
    })
    .expect("env provider gateway");
    let reference = SecretRef::new("env", "OPENAI_API_KEY", "provider.request", "openai", 1)
        .expect("server-owned credential reference");
    gateway
        .revoke_credential_ref(&reference)
        .expect("revocation recorded");

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
            ModelRequest {
                messages: vec![kiana_domain::ModelMessage::user("CI07 revoked request")],
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

    let mut deltas = Vec::new();
    let error = gateway
        .complete_admitted(prepared, permit, &PermitOnlyBudget, &mut |delta| {
            deltas.push(delta);
            Ok(())
        })
        .await
        .err()
        .expect("a revoked reference must fail closed");
    assert_eq!(error.code, "credential_secret_ref_revoked");
    assert!(!error.request_sent);
    assert!(deltas.is_empty());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(250), listener.accept())
            .await
            .is_err(),
        "a revoked credential must not open a provider connection"
    );
}
