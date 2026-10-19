//! BQ-23 surface parity: CLI/Workbench/Web command transport and the protocol body return one
//! projection, freshness, unknown count and provenance.
//!
//! Deny first: a CLI budget line that names an actor/project is refused locally, and the daemon
//! refuses an identity override that reaches it. Only then the allow path compares:
//!
//! - the CLI `/budget` route (`kiana_commands::budget::BudgetCommand`) encodes `budget.query.v1`
//!   with the argument schema the daemon serves;
//! - the Workbench and Web command transports (`harness_run::command_envelope_on_host`, the same
//!   function both surfaces call) produce one `command_result` value;
//! - decoding that value equals the protocol `BillingQuery` body answered by the same host.
//!
//! Proof ceiling: `local_behavior` over the in-process memory EventLog. It does not prove a real
//! HTTP round trip, durable projection or live billing.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use kiana_commands::budget::BudgetCommand;
use kiana_commands::types::{Command, CommandContext};
use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{json_digest, BillingQueryKind, BillingQueryRequest};
use kiana_entrypoints::harness_run;
use kiana_protocol::{
    ExecutionStatus, RequestEnvelope, RequestMetadata, ResponseEnvelope, BILLING_QUERY_SCHEMA,
};
use serde_json::{json, Value};

#[derive(Clone, Copy)]
struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn temp_project(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("kiana-bq23-ep-{label}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("project root");
    root
}

fn host() -> Arc<DaemonHost> {
    // The entrypoint crate owns no model crate dependency; the space fixture uses the same
    // configured harness constructor the real surfaces use.
    Arc::new(
        DaemonHost::with_env_harness_and_project_authority(Arc::new(TrustedProject))
            .expect("daemon host"),
    )
}

fn options(root: &Path) -> HashMap<String, Value> {
    let mut options = HashMap::new();
    options.insert(
        "cwd".to_owned(),
        Value::String(root.to_string_lossy().into_owned()),
    );
    options
}

fn metadata(root: &Path) -> RequestMetadata {
    let mut metadata = RequestMetadata::local("bq23-entrypoints", root.to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

/// Seed a committed event history. The projection needs a non-zero source cursor, not a
/// successful model call: the configured harness has no credential here, so the run may fail at
/// the model boundary while its authorization/start facts are still committed.
async fn seed_fact_history(host: &DaemonHost, root: &Path) {
    let response = host
        .handle(RequestEnvelope::run(
            metadata(root),
            "create the fixture fact history",
            Some("read-only".to_owned()),
        ))
        .await;
    let events = host
        .persisted_events()
        .await
        .expect("event store")
        .expect("memory event store");
    assert!(
        !events.is_empty(),
        "fixture history must commit facts; run returned {response:?}"
    );
}

fn command_context(root: &Path, args: &str) -> CommandContext {
    let mut app_state = HashMap::new();
    app_state.insert(
        "cwd".to_owned(),
        Value::String(root.to_string_lossy().into_owned()),
    );
    CommandContext {
        args: args.to_owned(),
        app_state,
    }
}

fn scope_digest(canonical_root: &str) -> String {
    json_digest(&json!({
        "schema": "kiana.billing-query-scope.v1",
        "project_root": canonical_root,
    }))
}

/// Deny: the CLI budget line refuses an actor/project flag before it can reach the daemon.
#[tokio::test]
async fn cli_budget_route_refuses_identity_override_flags() {
    let root = temp_project("cli-deny");
    let command = BudgetCommand;
    for args in ["card --actor someone-else", "queue --project /elsewhere"] {
        let error = command
            .route(&command_context(&root, args))
            .expect_err("identity override must be refused");
        assert_eq!(error.to_string(), "budget_identity_override_rejected");
    }
    let help = command
        .route(&command_context(&root, "help"))
        .expect("help");
    assert_eq!(help, kiana_commands::types::CommandRoute::Local);
    drop(command);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Deny: an identity override that reaches the daemon is refused, not filtered.
#[tokio::test]
async fn daemon_refuses_a_command_identity_override() {
    let root = temp_project("daemon-deny");
    let host = host();
    let response = host
        .handle(RequestEnvelope::command(
            metadata(&root),
            "budget.query.v1",
            json!({"schema": "kiana.budget-card-query.v1", "data_boundary_digest": "sha256:00"}),
        ))
        .await;
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("billing_query_identity_override_rejected")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

/// Success: CLI route, Workbench transport, Web transport and the protocol body agree.
#[tokio::test]
async fn three_entrypoints_and_the_protocol_body_return_one_projection() {
    let root = temp_project("parity");
    let host = host();
    seed_fact_history(&host, &root).await;

    // CLI: the route the `/budget card` line encodes.
    let route = BudgetCommand
        .route(&command_context(&root, "card --json --limit 50"))
        .expect("budget route");
    let kiana_commands::types::CommandRoute::ControlPlane { name, arguments } = route else {
        panic!("budget card must route through the control plane");
    };
    assert_eq!(name, "budget.query.v1");
    assert_eq!(arguments["schema"], json!("kiana.budget-card-query.v1"));
    assert_eq!(arguments["kind"], json!("budget_summary"));

    // Workbench and Web both call `command_envelope_on_host` with the route arguments.
    let workbench = harness_run::command_envelope_on_host(
        host.clone(),
        "bq23-entrypoints",
        name.clone(),
        arguments.clone(),
        &options(&root),
    )
    .await
    .expect("workbench command");
    let web = harness_run::command_envelope_on_host(
        host.clone(),
        "bq23-entrypoints",
        name,
        arguments,
        &options(&root),
    )
    .await
    .expect("web command");
    for response in [&workbench, &web] {
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
    }
    // The transport answers with `command_result`; the entrypoint decodes it as text.
    let workbench_result = &workbench.output["command_result"];
    let web_result = &web.output["command_result"];
    assert_eq!(workbench_result, web_result, "surfaces disagree");
    let carried: Value =
        serde_json::from_str(workbench_result["value"].as_str().expect("text value"))
            .expect("surface json");
    assert_eq!(carried["schema"], json!("kiana.billing-query-surface.v1"));

    // Protocol body over the same host: same projection, freshness, unknown and provenance.
    let identity = host
        .project_identity(root.to_string_lossy().as_ref())
        .expect("project identity");
    let protocol = host
        .handle(RequestEnvelope::billing_query(
            metadata(&root),
            BillingQueryRequest {
                schema: BILLING_QUERY_SCHEMA.to_owned(),
                kind: BillingQueryKind::BudgetSummary,
                project_id: identity.project_id,
                data_boundary_digest: scope_digest(identity.canonical_root.as_str()),
                source_cursor: None,
                after_cursor: 0,
                limit: 50,
                cursor: None,
                read_only: true,
            },
        ))
        .await;
    assert_eq!(protocol.status, ExecutionStatus::Completed, "{protocol:?}");

    for field in [
        "projection_digest",
        "response_digest",
        "source_cursor",
        "freshness",
        "unknown_count",
        "quarantine_count",
        "project_id",
        "data_boundary_digest",
    ] {
        assert_eq!(
            carried["billing_query"][field], protocol.output["billing_query"][field],
            "entrypoint and protocol disagree on {field}"
        );
    }
    assert_eq!(
        carried["cursor_snapshot_digest"],
        protocol.output["cursor_snapshot_digest"]
    );

    // Provenance: the response carries a sha256 projection digest and an unconfirmed receipt.
    assert!(carried["billing_query"]["projection_digest"]
        .as_str()
        .is_some_and(|digest| digest.starts_with("sha256:")));
    assert_eq!(
        carried["receipt_view"]["runtime_completed_is_not_business_outcome"],
        json!(true)
    );
    assert!(carried["receipt_view"]["business_outcome"].is_null());

    fs::remove_dir_all(root).expect("cleanup");
}

/// Guard: the three surfaces keep using the one command helper, not a private client.
#[test]
fn surfaces_share_the_command_helper() {
    let web = include_str!("../src/web.rs");
    let workbench = include_str!("../src/workbench_chat.rs");
    let cli = include_str!("../src/command_dispatch.rs");
    for (name, source) in [("web", web), ("workbench", workbench), ("cli", cli)] {
        assert!(
            source.contains("command_envelope_on_host")
                || source.contains("execute_control_plane_command"),
            "BQ-23 {name} surface stopped using the shared command path"
        );
        assert!(
            !source.contains("billing_query_projection_not_wired"),
            "BQ-23 {name} surface still claims the projection is unwired"
        );
    }
    let response: ResponseEnvelope = serde_json::from_value(json!({
        "schema": "kiana.protocol.v1",
        "request_id": kiana_domain::RequestId::new(),
        "status": "completed",
        "output": {},
        "error": null,
    }))
    .expect("envelope shape");
    assert_eq!(response.status, ExecutionStatus::Completed);
}
