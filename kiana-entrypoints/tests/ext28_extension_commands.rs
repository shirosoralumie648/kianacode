//! EXT-28 command/UI contract: CLI, Workbench, Web and Desktop forward one versioned command
//! intent through the shared `DaemonHost` adapter and only project what the server returned.
//!
//! Deny paths come first. A command that is malformed is refused by the local DTO before any
//! byte reaches the daemon; an unknown-trust project is refused by policy for every command and
//! the refusal carries no client material; an operator-less or under-declared state change is
//! refused with a stable permission code; and a well-formed state change still waits for approval
//! instead of executing. Only then is the read-only path compared across the four surfaces.
//!
//! `KIANA_HOME` is process-global and read on every trust lookup, so both fixtures serialize on a
//! file-local lock and restore the variable on drop. Local tests are not run in this session
//! (CI-only); the manual lane is `.github/workflows/ext28-extension-commands.yml`.

use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use kiana_daemon::{DaemonHost, StoredProjectTrustAuthority};
use kiana_domain::ExtensionConfigSnapshot;
use kiana_entrypoints::extension_projection::{command_receipt, extension_command_on_host};
use kiana_protocol::{
    EntryPointKind, ExecutionStatus, ExtensionCommand, ExtensionCommandRequest,
    ExtensionVisibilitySnapshot, ResponseEnvelope, EXTENSION_MANAGE_OPERATION,
};
use serde_json::{json, Value};

/// Extension identity the fixture commands address.
const EXTENSION_ID: &str = "ext28-review-skill";
/// Marker that must never be echoed by a refusal.
const REFUSED_TOKEN: &str = "ext28-refused-payload-token";
const TRUST_REVISION: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DEPENDENCY_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

static ENV_LOCK: Mutex<()> = Mutex::new(());

struct Fixture {
    _lock: MutexGuard<'static, ()>,
    original_home: Option<OsString>,
    root: PathBuf,
    project: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-ext28-{label}-{}-{stamp}",
            std::process::id()
        ));
        let home = root.join("kiana-home");
        fs::create_dir_all(&home).expect("fixture kiana home");
        let project = root.join("project");
        fs::create_dir_all(project.join(".git")).expect("fixture project");
        let original_home = std::env::var_os("KIANA_HOME");
        std::env::set_var("KIANA_HOME", &home);
        Self {
            _lock: lock,
            original_home,
            root,
            project,
        }
    }

    fn trust(&self, trust: kiana_types::ProjectTrust) {
        kiana_types::write_project_trust(&self.project, trust).expect("fixture trust record");
    }

    fn options(&self) -> HashMap<String, Value> {
        let mut options = HashMap::new();
        options.insert(
            "cwd".to_owned(),
            Value::String(self.project.to_string_lossy().into_owned()),
        );
        options
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        match self.original_home.as_ref() {
            Some(value) => std::env::set_var("KIANA_HOME", value),
            None => std::env::remove_var("KIANA_HOME"),
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture_host() -> Arc<DaemonHost> {
    Arc::new(
        DaemonHost::with_env_harness_and_project_authority(Arc::new(StoredProjectTrustAuthority))
            .expect("fixture daemon host"),
    )
}

fn operator_fields(
    mut request: ExtensionCommandRequest,
    version: u64,
    key: &str,
) -> ExtensionCommandRequest {
    request.expected_registry_version = Some(version);
    request.actor_id = Some("local-user".to_owned());
    request.reason = Some("EXT-28 entrypoint fixture".to_owned());
    request.idempotency_key = Some(key.to_owned());
    request
}

fn list_request() -> ExtensionCommandRequest {
    ExtensionCommandRequest::new(ExtensionCommand::List)
}

fn inspect_request(extension_id: &str) -> ExtensionCommandRequest {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Inspect);
    request.extension_id = Some(extension_id.to_owned());
    request
}

fn disable_request(version: u64, key: &str) -> ExtensionCommandRequest {
    let mut request = operator_fields(
        ExtensionCommandRequest::new(ExtensionCommand::Disable),
        version,
        key,
    );
    request.extension_id = Some(EXTENSION_ID.to_owned());
    request
}

fn revoke_request(version: u64, key: &str) -> ExtensionCommandRequest {
    let mut request = operator_fields(
        ExtensionCommandRequest::new(ExtensionCommand::Revoke),
        version,
        key,
    );
    request.extension_id = Some(EXTENSION_ID.to_owned());
    request
}

fn install_request(version: u64, key: &str) -> ExtensionCommandRequest {
    let mut request = operator_fields(
        ExtensionCommandRequest::new(ExtensionCommand::Install),
        version,
        key,
    );
    request.extension_id = Some(EXTENSION_ID.to_owned());
    request.package_path = Some("packages/ext28-review-skill.json".to_owned());
    request.package_sha256 = Some("a".repeat(64));
    request.trust_revision = Some(TRUST_REVISION.to_owned());
    request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    request
}

fn enable_request(version: u64, key: &str) -> ExtensionCommandRequest {
    let snapshot = ExtensionConfigSnapshot::new(EXTENSION_ID, version, Vec::new())
        .expect("fixture config snapshot");
    let mut request = operator_fields(
        ExtensionCommandRequest::new(ExtensionCommand::Enable),
        version,
        key,
    );
    request.extension_id = Some(EXTENSION_ID.to_owned());
    request.trust_revision = Some(TRUST_REVISION.to_owned());
    request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    request.approval_id = Some("approval.ext28".to_owned());
    request.config_snapshot_digest = Some(snapshot.snapshot_digest.clone());
    request.config_snapshot = Some(snapshot);
    request
}

fn rollback_request(version: u64, key: &str) -> ExtensionCommandRequest {
    let mut request = operator_fields(
        ExtensionCommandRequest::new(ExtensionCommand::Rollback),
        version,
        key,
    );
    request.extension_id = Some(EXTENSION_ID.to_owned());
    request.package_path = Some("packages/ext28-review-skill.json".to_owned());
    request.package_sha256 = Some("a".repeat(64));
    request
}

async fn command_on(
    host: &Arc<DaemonHost>,
    fixture: &Fixture,
    surface: EntryPointKind,
    session: &str,
    request: ExtensionCommandRequest,
) -> anyhow::Result<ResponseEnvelope> {
    extension_command_on_host(
        host.clone(),
        session.to_owned(),
        surface,
        request,
        &fixture.options(),
    )
    .await
}

fn encoded(response: &ResponseEnvelope) -> String {
    serde_json::to_string(response).expect("encoded response")
}

fn assert_no_client_material(text: &str, context: &str) {
    for marker in [EXTENSION_ID, REFUSED_TOKEN, "packages/ext28"] {
        assert!(
            !text.contains(marker),
            "{context} echoed client material: {marker}"
        );
    }
}

async fn lifecycle_fact_count(host: &Arc<DaemonHost>) -> usize {
    host.persisted_events()
        .await
        .expect("fixture event read")
        .unwrap_or_default()
        .iter()
        .filter(|event| event.kind == "extension.lifecycle")
        .count()
}

/// Deny-first: an unknown-trust project refuses every one of the seven commands before any
/// authority is evaluated, and neither the raw envelope nor the shared adapter echoes client
/// material back to a surface.
#[tokio::test]
async fn untrusted_project_refuses_every_command_before_authority_and_leaks_nothing() {
    let fixture = Fixture::new("untrusted");
    let host = fixture_host();
    let options = fixture.options();

    // The exact wire route a surface uses, read raw: only the envelope proves which authority
    // refused the command and that no output was minted.
    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-untrusted", &options)
            .expect("fixture client");
    let raw = client
        .command(
            metadata,
            EXTENSION_MANAGE_OPERATION,
            json!({"action": "list", "max_results": 64}),
        )
        .await
        .expect("transport");
    assert_eq!(raw.status, ExecutionStatus::Denied);
    assert_eq!(raw.error.as_deref(), Some("project_untrusted"));
    assert!(raw.output.is_null());
    assert_no_client_material(&encoded(&raw), "the raw refusal");

    // Every typed command goes through the shared adapter. None of them may negotiate an
    // authority of its own, and none may append a lifecycle fact.
    let requests = vec![
        (EntryPointKind::Cli, list_request()),
        (EntryPointKind::Workbench, inspect_request(EXTENSION_ID)),
        (
            EntryPointKind::Web,
            install_request(1, "ext28-install-untrusted"),
        ),
        (
            EntryPointKind::Desktop,
            enable_request(1, "ext28-enable-untrusted"),
        ),
        (
            EntryPointKind::Cli,
            disable_request(1, "ext28-disable-untrusted"),
        ),
        (
            EntryPointKind::Web,
            revoke_request(1, "ext28-revoke-untrusted"),
        ),
        (
            EntryPointKind::Desktop,
            rollback_request(1, "ext28-rollback-untrusted"),
        ),
    ];
    for (surface, request) in requests {
        let response = command_on(&host, &fixture, surface, "ext28-untrusted-surface", request)
            .await
            .expect("transport");
        assert_eq!(
            response.status,
            ExecutionStatus::Denied,
            "{surface:?} must be refused by policy"
        );
        assert_eq!(response.error.as_deref(), Some("project_untrusted"));
        assert!(response.output.is_null());
        assert_no_client_material(&encoded(&response), "the adapter refusal");
        assert!(
            command_receipt(&response).is_err(),
            "a refused command mints no receipt"
        );
    }
    assert_eq!(lifecycle_fact_count(&host).await, 0);
}

/// Deny-first: malformed commands never reach the daemon, while operator identity and the full
/// mutation binding stay server-owned; a well-formed read stays free and identical on all four
/// surfaces, and a well-formed state change waits for approval instead of executing.
#[tokio::test]
async fn trusted_project_keeps_reads_free_and_state_changes_approval_bound() {
    let fixture = Fixture::new("trusted");
    fixture.trust(kiana_types::ProjectTrust::Trusted);
    let host = fixture_host();
    let options = fixture.options();

    // (a) local DTO validation: the surface cannot send an inspect without an id, an enable
    // without its lifecycle binding, or a read-only command carrying mutation fields. Each
    // refusal happens before transport, so no request id is consumed by the daemon.
    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-validate-a", &options)
            .expect("fixture client");
    let inspect_without_id = ExtensionCommandRequest::new(ExtensionCommand::Inspect);
    let error = client
        .extension_command(metadata, inspect_without_id)
        .await
        .expect_err("inspect requires an extension id");
    assert!(error
        .to_string()
        .contains("extension_command_extension_id_required"));

    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-validate-b", &options)
            .expect("fixture client");
    let mut enable_without_binding = ExtensionCommandRequest::new(ExtensionCommand::Enable);
    enable_without_binding.extension_id = Some(EXTENSION_ID.to_owned());
    enable_without_binding.expected_registry_version = Some(1);
    enable_without_binding.actor_id = Some("local-user".to_owned());
    enable_without_binding.reason = Some("EXT-28 entrypoint fixture".to_owned());
    enable_without_binding.idempotency_key = Some("ext28-enable-unbound".to_owned());
    let error = client
        .extension_command(metadata, enable_without_binding)
        .await
        .expect_err("enable requires the lifecycle binding");
    assert!(error
        .to_string()
        .contains("extension_command_lifecycle_binding_required"));

    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-validate-c", &options)
            .expect("fixture client");
    let mut read_with_actor = list_request();
    read_with_actor.actor_id = Some("local-user".to_owned());
    let error = client
        .extension_command(metadata, read_with_actor)
        .await
        .expect_err("read-only commands carry no mutation fields");
    assert!(error
        .to_string()
        .contains("extension_command_read_only_mutation_fields"));
    assert_eq!(
        lifecycle_fact_count(&host).await,
        0,
        "a locally refused command appends no fact"
    );

    // (b) operator identity is part of the server context: without it a state change is blocked
    // before the registry can be touched.
    let (client, mut operatorless) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-operator", &options)
            .expect("fixture client");
    operatorless.actor_id = None;
    let response = client
        .command(
            operatorless,
            EXTENSION_MANAGE_OPERATION,
            json!({
                "action": "disable",
                "extension_id": EXTENSION_ID,
                "expected_registry_version": 1,
                "idempotency_key": "ext28-operatorless",
                "reason": "EXT-28 entrypoint fixture",
            }),
        )
        .await
        .expect("transport");
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(
        response.error.as_deref(),
        Some("extension_operator_required")
    );
    assert!(response.output.is_null());

    // (c) an under-declared state change is refused with the stable binding code, and the refusal
    // echoes none of the payload back.
    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-binding", &options)
            .expect("fixture client");
    let response = client
        .command(
            metadata,
            EXTENSION_MANAGE_OPERATION,
            json!({"action": "disable", "extension_id": EXTENSION_ID, "reason": REFUSED_TOKEN}),
        )
        .await
        .expect("transport");
    assert_eq!(response.status, ExecutionStatus::Blocked);
    assert_eq!(
        response.error.as_deref(),
        Some("extension_mutation_fields_required")
    );
    assert!(response.output.is_null());
    let text = encoded(&response);
    assert!(
        !text.contains(REFUSED_TOKEN),
        "the binding refusal must not echo the payload"
    );

    // (d) read-only list: no approval, no receipt, no lifecycle fact, and one identical
    // projection for CLI, Workbench, Web and Desktop.
    let mut snapshots = Vec::new();
    for (surface, session) in [
        (EntryPointKind::Cli, "ext28-cli"),
        (EntryPointKind::Workbench, "ext28-workbench"),
        (EntryPointKind::Web, "ext28-web"),
        (EntryPointKind::Desktop, "ext28-desktop"),
    ] {
        let response = command_on(&host, &fixture, surface, session, list_request())
            .await
            .expect("read-only command");
        assert_eq!(
            response.status,
            ExecutionStatus::Completed,
            "{surface:?} must read the projection without approval"
        );
        assert_eq!(
            command_receipt(&response)
                .expect_err("a read-only command mints no receipt")
                .to_string(),
            "extension_command_receipt_missing"
        );
        let snapshot: ExtensionVisibilitySnapshot =
            serde_json::from_value(response.output["snapshot"].clone()).expect("snapshot");
        snapshots.push((surface, snapshot));
    }
    let reference = &snapshots[0].1;
    for (surface, snapshot) in &snapshots {
        assert_eq!(
            snapshot.snapshot_id, reference.snapshot_id,
            "{surface:?} must read the same snapshot id"
        );
        assert_eq!(
            snapshot.generation, reference.generation,
            "{surface:?} must read the same generation"
        );
        assert_eq!(
            snapshot.snapshot_digest, reference.snapshot_digest,
            "{surface:?} must read the same projection digest"
        );
    }

    // (e) a well-formed state change is an approval-bound intent: it does not execute, it does
    // not mint a receipt, and it appends nothing until the approval is consumed.
    let response = command_on(
        &host,
        &fixture,
        EntryPointKind::Workbench,
        "ext28-approval",
        disable_request(1, "ext28-approval"),
    )
    .await
    .expect("mutation intent");
    assert_eq!(response.status, ExecutionStatus::AwaitingApproval);
    assert_eq!(
        response.error.as_deref(),
        Some("external_side_effect_requires_approval")
    );
    assert!(response.output["approval"].is_object());
    assert_eq!(
        command_receipt(&response)
            .expect_err("no receipt exists before the registry fact")
            .to_string(),
        "extension_command_receipt_missing"
    );
    assert_eq!(
        lifecycle_fact_count(&host).await,
        0,
        "an unapproved state change appends no lifecycle fact"
    );

    // (f) a malformed mutation never reaches the daemon either.
    let (client, metadata) =
        kiana_entrypoints::harness_run::client_on_host(host.clone(), "ext28-late", &options)
            .expect("fixture client");
    let mut install_without_package = operator_fields(
        ExtensionCommandRequest::new(ExtensionCommand::Install),
        1,
        "ext28-install-unbound",
    );
    install_without_package.extension_id = Some(EXTENSION_ID.to_owned());
    let error = client
        .extension_command(metadata, install_without_package)
        .await
        .expect_err("install requires a package binding");
    assert!(error
        .to_string()
        .contains("extension_command_package_required"));
    assert_eq!(lifecycle_fact_count(&host).await, 0);
}
