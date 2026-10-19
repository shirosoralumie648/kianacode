//! BQ-23 protocol/command surface fixtures over the real daemon spine (deny first).
//!
//! These fixtures drive `DaemonHost -> ControlPlane -> EventStore -> BQ-20 projector -> BQ-23
//! adapter`. They do not build a projection locally and do not call the adapter directly, so a
//! surface that stopped going through the daemon would fail here.
//!
//! Deny order:
//! 1. a store that never committed a cursor renders no "empty bill success";
//! 2. a caller-chosen boundary digest is refused instead of selecting its own scope;
//! 3. a caller identity other than the authenticated principal is refused.
//!
//! Only then the success fixture proves the four kinds return one shared shape, that repeating
//! the same request over the same cursor/version yields the same digests, and that the human
//! `budget.query.v1` command carries the same projection as the protocol body.
//!
//! Proof ceiling: `local_behavior` over the in-process memory EventLog. It is not durable
//! cross-process projection, provider-invoice truth or live billing.

use kiana_daemon::{DaemonHost, ProjectTrustAuthority};
use kiana_domain::{json_digest, BillingQueryKind, BillingQueryRequest};
use kiana_protocol::{ExecutionStatus, RequestEnvelope, RequestMetadata, BILLING_QUERY_SCHEMA};
use kiana_runner::{KianaHarness, ScriptedModel};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

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
    let root = std::env::temp_dir().join(format!("kiana-bq23-{label}-{stamp}"));
    fs::create_dir_all(root.join(".git")).expect("project root");
    root
}

fn host() -> DaemonHost {
    let model = ScriptedModel::from_json(&json!([{"text": "unused fixture response"}]))
        .expect("fake model cassette");
    let harness = KianaHarness::new(Arc::new(model));
    DaemonHost::with_harness_and_project_authority(harness, Arc::new(TrustedProject))
        .expect("daemon host")
}

fn metadata(root: &Path, session: &str) -> RequestMetadata {
    let mut metadata = RequestMetadata::local(session, root.to_string_lossy());
    metadata.project_trusted = true;
    metadata
}

/// Boundary digest the server derives for a canonical project root. The fixture repeats the
/// derivation only because the test acts as the caller; the daemon never reads this value.
fn scope_digest(canonical_root: &str) -> String {
    json_digest(&json!({
        "schema": "kiana.billing-query-scope.v1",
        "project_root": canonical_root,
    }))
}

fn query(
    identity: &kiana_domain::ProjectIdentity,
    kind: BillingQueryKind,
    boundary_digest: String,
) -> BillingQueryRequest {
    BillingQueryRequest {
        schema: BILLING_QUERY_SCHEMA.to_owned(),
        kind,
        project_id: identity.project_id,
        data_boundary_digest: boundary_digest,
        source_cursor: Some(1),
        after_cursor: 0,
        limit: 50,
        cursor: None,
        read_only: true,
    }
}

/// Deny: a store with no committed cursor must not answer "zero rows, Completed".
#[tokio::test]
async fn empty_log_is_not_an_empty_bill_success() {
    let root = temp_project("empty-log");
    let host = host();
    let identity = host
        .project_identity(root.to_string_lossy().as_ref())
        .expect("project identity");
    let envelope = RequestEnvelope::billing_query(
        metadata(&root, "bq23-empty-log"),
        query(
            &identity,
            BillingQueryKind::BudgetSummary,
            scope_digest(identity.canonical_root.as_str()),
        ),
    );
    let response = host.handle(envelope).await;
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("billing_query_projection_source_cursor_missing")
    );
    assert_eq!(response.output, Value::Null);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Deny: the digest on the wire is compared with the server value, never adopted.
#[tokio::test]
async fn caller_chosen_boundary_digest_is_refused() {
    let root = temp_project("boundary");
    let host = host();
    let identity = host
        .project_identity(root.to_string_lossy().as_ref())
        .expect("project identity");
    let forged = json_digest(
        &json!({"schema": "kiana.billing-query-scope.v1", "project_root": "/elsewhere"}),
    );
    let envelope = RequestEnvelope::billing_query(
        metadata(&root, "bq23-boundary"),
        query(&identity, BillingQueryKind::CostExport, forged),
    );
    let response = host.handle(envelope).await;
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("billing_query_boundary_mismatch")
    );
    assert_eq!(response.output, Value::Null);
    fs::remove_dir_all(root).expect("cleanup");
}

/// Deny: another actor cannot read this principal's billing projection.
#[tokio::test]
async fn wire_actor_other_than_the_authenticated_principal_is_refused() {
    let root = temp_project("actor");
    let host = host();
    let identity = host
        .project_identity(root.to_string_lossy().as_ref())
        .expect("project identity");
    let mut metadata = metadata(&root, "bq23-actor");
    metadata.actor_id = Some("wire-attacker".to_owned());
    let envelope = RequestEnvelope::billing_query(
        metadata,
        query(
            &identity,
            BillingQueryKind::BudgetSummary,
            scope_digest(identity.canonical_root.as_str()),
        ),
    );
    let response = host.handle(envelope).await;
    assert_eq!(response.status, ExecutionStatus::Blocked, "{response:?}");
    assert_eq!(
        response.error.as_deref(),
        Some("billing_query_unauthenticated")
    );
    fs::remove_dir_all(root).expect("cleanup");
}

async fn completed_run(host: &DaemonHost, root: &Path, session: &str) {
    let response = host
        .handle(RequestEnvelope::run(
            metadata(root, session),
            "create the fixture fact history",
            Some("read-only".to_owned()),
        ))
        .await;
    assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
}

/// Success: all four kinds share one surface, recompute stably, and the human command carries
/// exactly the same projection as the protocol body.
#[tokio::test]
async fn four_kinds_share_one_surface_and_recompute_stably() {
    let root = temp_project("surfaces");
    let host = host();
    completed_run(&host, &root, "bq23-surfaces").await;
    let identity = host
        .project_identity(root.to_string_lossy().as_ref())
        .expect("project identity");
    let digest = scope_digest(identity.canonical_root.as_str());

    let mut seen_digests = Vec::new();
    for kind in [
        BillingQueryKind::BudgetSummary,
        BillingQueryKind::UsageBreakdown,
        BillingQueryKind::CostExport,
        BillingQueryKind::ReconciliationInbox,
    ] {
        let envelope = RequestEnvelope::billing_query(
            metadata(&root, "bq23-surfaces"),
            query(&identity, kind, digest.clone()),
        );
        let response = host.handle(envelope).await;
        assert_eq!(response.status, ExecutionStatus::Completed, "{response:?}");
        assert_eq!(response.output["schema"], "kiana.billing-query-surface.v1");
        assert_eq!(response.output["billing_query"]["kind"], json!(kind));
        // The receipt view never converts a completed runtime into a business outcome.
        assert_eq!(
            response.output["receipt_view"]["runtime_completed_is_not_business_outcome"],
            json!(true)
        );
        assert_eq!(
            response.output["receipt_view"]["business_outcome_confirmed"],
            json!(false)
        );
        assert!(response.output["receipt_view"]["business_outcome"].is_null());
        assert!(response.output["billing_query"]["source_cursor"]
            .as_u64()
            .is_some_and(|cursor| cursor > 0));
        assert!(response.output["billing_query"]["projection_digest"]
            .as_str()
            .is_some_and(|digest| digest.starts_with("sha256:")));

        // Same cursor and projection version recompute to the same response digest.
        let repeat = host
            .handle(RequestEnvelope::billing_query(
                metadata(&root, "bq23-surfaces"),
                query(&identity, kind, digest.clone()),
            ))
            .await;
        assert_eq!(
            repeat.output["billing_query"]["response_digest"],
            response.output["billing_query"]["response_digest"],
            "recomputation drifted for {kind:?}"
        );
        seen_digests.push(
            response.output["billing_query"]["projection_digest"]
                .as_str()
                .expect("projection digest")
                .to_owned(),
        );
    }
    // Every kind reads the same projection version.
    assert!(seen_digests.windows(2).all(|pair| pair[0] == pair[1]));

    // Human command transport: same value, no second computation path.
    let command = host
        .handle(RequestEnvelope::command(
            metadata(&root, "bq23-surfaces"),
            "budget.query.v1",
            json!({
                "schema": "kiana.budget-card-query.v1",
                "operation": "budget_card",
                "kind": "budget_summary",
                "output": "json",
                "limit": 50,
            }),
        ))
        .await;
    assert_eq!(command.status, ExecutionStatus::Completed, "{command:?}");
    let result = &command.output["command_result"];
    assert_eq!(result["output_type"], json!("text"));
    assert_eq!(
        result["metadata"]["schema"],
        json!("kiana.billing-query-surface.v1")
    );
    let carried: Value =
        serde_json::from_str(result["value"].as_str().expect("text value")).expect("surface json");
    assert_eq!(carried["schema"], json!("kiana.billing-query-surface.v1"));
    assert_eq!(
        carried["billing_query"]["projection_digest"],
        json!(seen_digests[0])
    );

    // A command that tries to choose its own identity is refused, not filtered.
    let override_attempt = host
        .handle(RequestEnvelope::command(
            metadata(&root, "bq23-surfaces"),
            "budget.query.v1",
            json!({"schema": "kiana.budget-card-query.v1", "project_root": "/elsewhere"}),
        ))
        .await;
    assert_eq!(
        override_attempt.status,
        ExecutionStatus::Blocked,
        "{override_attempt:?}"
    );
    assert_eq!(
        override_attempt.error.as_deref(),
        Some("billing_query_identity_override_rejected")
    );
    fs::remove_dir_all(root).expect("cleanup");
}
