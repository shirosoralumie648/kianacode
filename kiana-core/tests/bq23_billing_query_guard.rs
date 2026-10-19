//! BQ-23 ControlPlane-side guards for the read-only billing query (deny first).
//!
//! The first half executes the two domain-only guards this card adds to `kiana-core`:
//!
//! - `BillingQueryAdmission` refuses a query whose path took a claim, consumed or staged an
//!   approval, reserved or released budget, or appended a fact, each with its own stable code;
//! - `BillingQueryReceiptView` refuses a business outcome attached to a non-completed runtime or
//!   to no evidence, and refuses a view that flips the runtime/business separation marker.
//!
//! The second half is a source/wiring guard: it pins the refusal codes and fixture names in the
//! query adapter, the daemon service and this branch's fixtures, and checks the manual workflow
//! lane plus `scripts/ci/test-shards.json`. `include_str!` hits are text checks, not runtime proof.

use kiana_core::{
    BillingQueryAdmission, BillingQueryAdmissionError, BillingQueryEffectWitness,
    BillingQueryReceiptView, BillingReceiptViewError, BILLING_QUERY_ADMISSION_NO_EFFECTS,
    BILLING_RECEIPT_VIEW_NO_FACT_WRITES, BILLING_RECEIPT_VIEW_SCHEMA,
};
use kiana_domain::{
    BillingQueryKind, BillingQueryRequest, ExecutionStatus, ProjectId, BILLING_QUERY_SCHEMA,
};

const ADMISSION_SOURCE: &str = include_str!("../src/billing_query_admission.rs");
const RECEIPT_SOURCE: &str = include_str!("../src/billing_receipt_view.rs");
const QUERY_SOURCE: &str = include_str!("../../kiana-query/src/billing_api.rs");
const QUERY_FIXTURES: &str = include_str!("../../kiana-query/tests/bq23_billing_api.rs");
const QUERY_GUARD: &str = include_str!("../../kiana-query/tests/bq23_billing_api_guard.rs");
const DAEMON_SOURCE: &str = include_str!("../../kiana-daemon/src/billing_query_service.rs");
const DAEMON_FIXTURES: &str =
    include_str!("../../kiana-daemon/tests/bq23_billing_query_surfaces.rs");
const MANUAL_LANE: &str = include_str!("../../.github/workflows/bq23-query.yml");
const SHARDS: &str = include_str!("../../scripts/ci/test-shards.json");

const BOUNDARY: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn read_only_request(kind: BillingQueryKind) -> BillingQueryRequest {
    BillingQueryRequest {
        schema: BILLING_QUERY_SCHEMA.to_owned(),
        kind,
        project_id: ProjectId::new(),
        data_boundary_digest: BOUNDARY.to_owned(),
        source_cursor: Some(1),
        after_cursor: 0,
        limit: 10,
        cursor: None,
        read_only: true,
    }
}

/// Deny: every observed effect gets its own stable refusal before the zero-effect path passes.
#[test]
fn admission_refuses_claim_approval_and_reservation_effects() {
    let request = read_only_request(BillingQueryKind::BudgetSummary);
    assert!(BILLING_QUERY_ADMISSION_NO_EFFECTS);

    // Deny ordering: a consumed claim is reported before an approval or a reservation.
    let mut witness = BillingQueryEffectWitness::none();
    witness.claims_staged = 1;
    witness.approvals_consumed = 1;
    witness.reservations_reserved = 1;
    assert_eq!(
        BillingQueryAdmission::admit(&request, &witness).unwrap_err(),
        BillingQueryAdmissionError::ConsumedClaim
    );
    assert_eq!(
        BillingQueryAdmission::admit(&request, &witness)
            .unwrap_err()
            .to_string(),
        "billing_query_consumed_claim"
    );
    assert!(!BillingQueryAdmission::is_read_only(&witness));

    for (witness, code) in [
        (
            BillingQueryEffectWitness {
                claims_staged: 0,
                approvals_consumed: 1,
                ..BillingQueryEffectWitness::none()
            },
            "billing_query_consumed_approval",
        ),
        (
            BillingQueryEffectWitness {
                approvals_staged: 1,
                ..BillingQueryEffectWitness::none()
            },
            "billing_query_staged_approval",
        ),
        (
            BillingQueryEffectWitness {
                reservations_reserved: 1,
                ..BillingQueryEffectWitness::none()
            },
            "billing_query_reserved_budget",
        ),
        (
            BillingQueryEffectWitness {
                reservations_released: 1,
                ..BillingQueryEffectWitness::none()
            },
            "billing_query_released_reservation",
        ),
        (
            BillingQueryEffectWitness {
                facts_appended: 1,
                ..BillingQueryEffectWitness::none()
            },
            "billing_query_wrote_facts",
        ),
    ] {
        assert_eq!(
            BillingQueryAdmission::admit(&request, &witness)
                .unwrap_err()
                .to_string(),
            code
        );
        assert!(!BillingQueryAdmission::is_read_only(&witness));
    }

    // Deny: a non-read-only request is refused even with a zero-effect witness.
    let mut mutable = request.clone();
    mutable.read_only = false;
    assert_eq!(
        BillingQueryAdmission::admit(&mutable, &BillingQueryEffectWitness::none())
            .unwrap_err()
            .to_string(),
        "billing_query_request_invalid:billing_query_request_not_read_only"
    );

    // The zero-effect read-only request is the only admitted shape.
    let none = BillingQueryEffectWitness::none();
    assert!(BillingQueryAdmission::is_read_only(&none));
    assert_eq!(BillingQueryAdmission::admit(&request, &none), Ok(()));
}

/// Deny: a completed runtime is not a business outcome, and an outcome needs evidence.
#[test]
fn receipt_view_refuses_runtime_business_conflation() {
    assert!(BILLING_RECEIPT_VIEW_NO_FACT_WRITES);

    // A completed runtime alone reports no business outcome.
    let runtime_only = BillingQueryReceiptView::runtime_only(ExecutionStatus::Completed);
    assert_eq!(runtime_only.schema, BILLING_RECEIPT_VIEW_SCHEMA);
    assert!(runtime_only.business_outcome.is_none());
    assert!(!runtime_only.business_outcome_confirmed);
    assert!(runtime_only.runtime_completed_is_not_business_outcome);
    runtime_only.validate().expect("runtime-only view");
    assert!(runtime_only.digest().starts_with("sha256:"));

    // Deny: outcome without evidence.
    assert_eq!(
        BillingQueryReceiptView::with_business_outcome(
            ExecutionStatus::Completed,
            "invoice.settled",
            Vec::new(),
        )
        .unwrap_err(),
        BillingReceiptViewError::BusinessOutcomeWithoutEvidence
    );
    // Deny: outcome on a runtime that did not complete.
    for status in [
        ExecutionStatus::Failed,
        ExecutionStatus::Blocked,
        ExecutionStatus::ResultUnknown,
    ] {
        assert_eq!(
            BillingQueryReceiptView::with_business_outcome(
                status,
                "invoice.settled",
                vec!["receipt:1".to_owned()],
            )
            .unwrap_err(),
            BillingReceiptViewError::BusinessOutcomeWithoutRuntime
        );
    }
    // Deny: blank outcome text.
    assert_eq!(
        BillingQueryReceiptView::with_business_outcome(
            ExecutionStatus::Completed,
            "   ",
            vec!["receipt:1".to_owned()],
        )
        .unwrap_err(),
        BillingReceiptViewError::BusinessOutcomeEmpty
    );

    // Deny: a view that flips the separation marker is refused on validate.
    let mut conflated = runtime_only.clone();
    conflated.runtime_completed_is_not_business_outcome = false;
    assert_eq!(
        conflated.validate().unwrap_err().to_string(),
        "billing_receipt_view_invalid:billing_receipt_runtime_business_outcome_conflated"
    );
    // Deny: an unconfirmed flag that disagrees with the fields.
    let mut unconfirmed = runtime_only.clone();
    unconfirmed.business_outcome = Some("invoice.settled".to_owned());
    unconfirmed.business_evidence_refs = vec!["receipt:1".to_owned()];
    assert_eq!(
        unconfirmed.validate().unwrap_err(),
        BillingReceiptViewError::BusinessOutcomeConfirmationMismatch
    );

    // Allow path: an evidenced outcome on a completed runtime is confirmed by construction.
    let evidenced = BillingQueryReceiptView::with_business_outcome(
        ExecutionStatus::Completed,
        "invoice.settled",
        vec!["receipt:1".to_owned()],
    )
    .expect("evidenced outcome");
    assert!(evidenced.business_outcome_confirmed);
    assert!(evidenced.runtime_completed_is_not_business_outcome);
    evidenced.validate().expect("evidenced view");
    assert_ne!(evidenced.digest(), runtime_only.digest());
}

/// Guard: the query adapter and the daemon service keep the same refusal vocabulary.
#[test]
fn query_sources_keep_deny_codes_and_read_only_shape() {
    for marker in [
        "ProjectionSourceMissing",
        "billing_query_projection_source_cursor_missing",
        "BillingQueryError::ProjectionSourceMissing",
        "query_billing_projection",
        "QueryDataDisposition::Allowed",
    ] {
        assert!(
            QUERY_SOURCE.contains(marker),
            "BQ-23 query marker missing: {marker}"
        );
    }
    // The adapter never renders an empty bill for a cursor-less projection.
    let missing_guard = QUERY_SOURCE
        .split("pub fn query_billing_projection")
        .nth(1)
        .expect("query function");
    assert!(
        missing_guard.contains("source_cursor == 0"),
        "BQ-23 adapter lost its source-cursor refusal"
    );
    assert!(
        missing_guard.contains("ProjectionSourceMissing"),
        "BQ-23 adapter no longer returns the missing-source refusal"
    );

    for marker in [
        "billing_query_projection_unsupported",
        "billing_query_projection_unavailable",
        "billing_query_projection_retention_truncated",
        "billing_query_projection_cursor_lagging",
        "billing_query_projection_source_cursor_missing",
        "billing_query_boundary_mismatch",
        "billing_query_project_mismatch",
        "billing_query_identity_override_rejected",
        "billing_query_argument_schema_invalid",
        "billing_query_kind_invalid",
        "billing_query_limit_invalid",
        "billing_query_after_cursor_invalid",
        "billing_query_source_cursor_invalid",
        "billing_query_cursor_invalid",
        "BILLING_QUERY_SURFACE_SCHEMA",
        "BILLING_QUERY_SERVICE_READ_ONLY",
    ] {
        assert!(
            DAEMON_SOURCE.contains(marker),
            "BQ-23 daemon service marker missing: {marker}"
        );
    }
    // The service reads a projection; it never appends, reserves, releases or consumes.
    for forbidden in [
        ".append(",
        "append_idempotent",
        "reserve_budget",
        "release_reservation",
        "consume_approval",
        "decide_approval",
        "ModelClient",
        "tokio::spawn",
        "std::process::Command",
    ] {
        assert!(
            !DAEMON_SOURCE.contains(forbidden),
            "BQ-23 daemon service gained an effect path: {forbidden}"
        );
    }
    // The five model-visible tools stay unchanged: no tool schema on this path.
    for forbidden in ["apply_patch", "memory.search", "memory.write", "ToolSchema"] {
        for (name, source) in [
            ("query", QUERY_SOURCE),
            ("daemon", DAEMON_SOURCE),
            ("admission", ADMISSION_SOURCE),
            ("receipt", RECEIPT_SOURCE),
        ] {
            assert!(
                !source.contains(forbidden),
                "BQ-23 {name} path gained a model tool surface: {forbidden}"
            );
        }
    }
    for forbidden in [
        "EventStorePort",
        "CapabilityBroker",
        "ApprovalStore",
        "reserve(",
        "release_reservation",
        "append(",
        "Uuid::new_v4",
        "unsafe",
    ] {
        for (name, source) in [("admission", ADMISSION_SOURCE), ("receipt", RECEIPT_SOURCE)] {
            assert!(
                !source.contains(forbidden),
                "BQ-23 {name} guard widened authority: {forbidden}"
            );
        }
    }
}

/// Guard: the deny fixtures exist before the success fixture and are wired to the manual lane.
#[test]
fn fixtures_are_deny_first_and_wired_to_the_manual_lane() {
    // Query adapter fixtures: catalog refusal codes, then the success shape.
    for code in [
        "billing_query_boundary_denied",
        "billing_query_cursor_projection_mismatch",
        "billing_query_projection_source_cursor_missing",
    ] {
        assert!(
            QUERY_FIXTURES.contains(code) || QUERY_GUARD.contains(code),
            "BQ-23 query fixture refusal code missing: {code}"
        );
    }
    assert!(
        QUERY_FIXTURES.contains("query_returns_same_projection_fence_and_freshness_for_read_views"),
        "BQ-23 query success fixture missing"
    );

    // Daemon fixtures: deny fixtures are declared before the success fixture in file order.
    let deny_positions = [
        "empty_log_is_not_an_empty_bill_success",
        "caller_chosen_boundary_digest_is_refused",
        "wire_actor_other_than_the_authenticated_principal_is_refused",
    ]
    .iter()
    .map(|name| {
        DAEMON_FIXTURES
            .find(name)
            .unwrap_or_else(|| panic!("BQ-23 daemon deny fixture missing: {name}"))
    });
    let success = DAEMON_FIXTURES
        .find("four_kinds_share_one_surface_and_recompute_stably")
        .expect("BQ-23 daemon success fixture missing");
    for deny in deny_positions {
        assert!(
            deny < success,
            "BQ-23 deny fixtures must precede the success fixture"
        );
    }
    for code in [
        "billing_query_projection_source_cursor_missing",
        "billing_query_boundary_mismatch",
        "billing_query_unauthenticated",
        "billing_query_identity_override_rejected",
    ] {
        assert!(
            DAEMON_FIXTURES.contains(code),
            "BQ-23 daemon fixture refusal code missing: {code}"
        );
    }
    for forbidden in ["#[ignore", "--ignored"] {
        for (name, source) in [("query", QUERY_FIXTURES), ("daemon", DAEMON_FIXTURES)] {
            assert!(
                !source.contains(forbidden),
                "BQ-23 {name} fixtures must not be skipped: {forbidden}"
            );
        }
    }

    // Manual lane: dispatch-only, read-only permissions, and every BQ-23 target.
    assert!(MANUAL_LANE.contains("workflow_dispatch:"));
    assert!(MANUAL_LANE.contains("permissions:"));
    assert!(MANUAL_LANE.contains("contents: read"));
    for forbidden in ["push:", "pull_request:", "continue-on-error", "--ignored"] {
        assert!(
            !MANUAL_LANE.contains(forbidden),
            "BQ-23 manual lane must stay dispatch-only: {forbidden}"
        );
    }
    for command in [
        "cargo test -p kiana-core --test bq23_billing_query_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test bq23_billing_query_guard --locked -- --test-threads=1",
        "cargo test -p kiana-daemon --test bq23_billing_query_surfaces --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test bq23_budget_query_surfaces --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq23_billing_api --locked -- --test-threads=1",
        "cargo test -p kiana-query --test bq23_billing_api_guard --locked -- --test-threads=1",
    ] {
        assert!(
            MANUAL_LANE.contains(command),
            "BQ-23 manual lane missing: {command}"
        );
    }

    // Shard registry: the new core guard is registered; daemon/query are whole-crate shards.
    let shards: serde_json::Value = serde_json::from_str(SHARDS).expect("shard registry json");
    let core_targets = shards["kiana-core-s1/6"]["targets"]
        .as_array()
        .expect("core shard targets");
    assert!(core_targets
        .iter()
        .any(|target| target.as_str() == Some("bq23_billing_query_guard")));
    assert!(shards["kiana-daemon"]["targets"].is_null());
    assert!(shards["kiana-query"]["targets"].is_null());
}
