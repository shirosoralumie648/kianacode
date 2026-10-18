//! UI-15 boundary guards: the Workbench inbox, artifact viewer and receipt view are display and
//! validation boundaries.
//!
//! They may keep server payloads, scope summaries, artifact pages and receipt provenance for
//! display, but they must never own a daemon, client transport, broker, event store, process,
//! filesystem, network or model loop, and they must never approve, restore, execute or reconcile
//! anything. The completion-slice guard keeps the declarative fixture, the behaviour fixtures, the
//! manual lane, the baseline document and the whole-crate shard wired together.

use kiana_entrypoints::workbench_review::{
    ArtifactViewState, InboxCardState, ReceiptDisplayState, ReceiptUpdate, ReviewRefusal,
    ReviewRefusalStage, WorkbenchDecision, MAX_ARTIFACT_BYTES, MAX_ARTIFACT_PAGES,
    MAX_ARTIFACT_PAGE_BYTES, MAX_INBOX_CARDS, MAX_INBOX_DECISIONS, MAX_INBOX_FIELDS,
    MAX_INBOX_PAYLOAD_BYTES, MAX_RECEIPT_FILES, MAX_RECEIPT_LIMITATIONS, MAX_RECEIPT_PROVENANCE,
    MAX_SCOPE_PATHS,
};

#[test]
fn workbench_review_is_a_display_and_validation_boundary() {
    let source = include_str!("../src/workbench_review.rs");
    for marker in [
        "WorkbenchInboxCard",
        "reason",
        "scope",
        "expires_at_unix_ms",
        "fields",
        "allowed_decisions",
        "ArtifactRef",
        "artifact_page_digest_mismatch",
        "inbox_payload_digest_mismatch",
        "json_digest(&self.payload)",
        "artifact_revision_mismatch",
        "ReceiptFile",
        "ReceiptCost",
        "limitations",
        "provenance",
        "ResultUnknown",
        "approval_expired",
        "approval_revoked",
        "client_payload_mutation",
        "ControlPlane",
        "WorkbenchDecision",
        "InboxCardState",
        "ArtifactViewState",
        "ReceiptGolden",
        "ReceiptUpdate",
        "REVIEW_REFUSALS",
        "fn display_state",
        "fn upsert",
        "fn revoke",
        "fn actionable",
        "fn missing_pages",
        "fn retained_pages",
        "fn apply_server_receipt",
        "fn golden",
        "fn is_terminal",
        "receipt_terminal_regression",
        "inbox_card_revocation_regression",
        "result_unknown_marker_missing",
        "is_terminal_status",
    ] {
        assert!(source.contains(marker), "UI-15 marker missing: {marker}");
    }
    for forbidden in [
        "DaemonHost::",
        "CapabilityBroker",
        "KianaHarness",
        "std::fs",
        "tokio::",
        "reqwest::",
        "Command::new",
        "apply_patch",
        "fn execute",
        "fn approve",
        "fn restore",
        "fn reconcile",
        "std::process",
        "std::net",
        "async fn",
        "unsafe",
    ] {
        assert!(
            !source.contains(forbidden),
            "display boundary leaked {forbidden}"
        );
    }
    // The viewer never computes a replacement diff and never fetches a path or URL.
    for forbidden in [
        "similar::",
        "diff::",
        "reqwest",
        "ureq",
        "hyper",
        "url::Url",
    ] {
        assert!(
            !source.contains(forbidden),
            "the diff view must stay server-provided: {forbidden}"
        );
    }
    // A receipt can never be replaced without the replay/terminal fence.
    assert!(source.contains("*self = next;"));
    assert!(source.contains("if next.provenance.source_cursor < self.provenance.source_cursor"));
}

/// Completion slice: the declarative fixture, the behaviour fixtures, the manual lane, the baseline
/// document and the automatic shard must stay wired together.
#[test]
fn ui15_completion_slice_stays_wired_to_the_fixture_lane_and_shard() {
    let source = include_str!("../src/workbench_review.rs");
    let fixtures = include_str!("ui15_workbench_review.rs");
    let fixture_raw = include_str!("fixtures/ui15-workbench-review.json");
    let baseline = include_str!("../../docs/roadmap/ui15-workbench-inbox-receipt-baseline.md");
    let card = include_str!("../../docs/roadmap/ui-entrypoints.md");
    let lane = include_str!("../../.github/workflows/ui15-workbench-review.yml");
    let shards_raw = include_str!("../../scripts/ci/test-shards.json");
    let protocol = include_str!("../../kiana-protocol/src/ui_contracts.rs");

    let fixture: serde_json::Value =
        serde_json::from_str(fixture_raw).expect("the UI-15 fixture must stay valid JSON");
    assert_eq!(fixture["schema"], "kiana.ui15-workbench-review.v1");
    assert_eq!(fixture["step"], "UI-15");

    // 1. the declared budgets match the source constants.
    for (budget, expected) in [
        ("max_inbox_payload_bytes", MAX_INBOX_PAYLOAD_BYTES as u64),
        ("max_inbox_cards", MAX_INBOX_CARDS as u64),
        ("max_inbox_fields", MAX_INBOX_FIELDS as u64),
        ("max_inbox_decisions", MAX_INBOX_DECISIONS as u64),
        ("max_scope_paths", MAX_SCOPE_PATHS as u64),
        ("max_artifact_pages", MAX_ARTIFACT_PAGES as u64),
        ("max_artifact_page_bytes", MAX_ARTIFACT_PAGE_BYTES as u64),
        ("max_artifact_bytes", MAX_ARTIFACT_BYTES as u64),
        ("max_receipt_files", MAX_RECEIPT_FILES as u64),
        ("max_receipt_limitations", MAX_RECEIPT_LIMITATIONS as u64),
        ("max_receipt_provenance", MAX_RECEIPT_PROVENANCE as u64),
    ] {
        assert_eq!(
            fixture["budgets"][budget].as_u64(),
            Some(expected),
            "UI-15 fixture budget {budget} drifted from the source constant"
        );
    }

    // 2. every declared vocabulary is the one the source exposes.
    let declared = |key: &str| -> Vec<String> {
        fixture[key]
            .as_array()
            .unwrap_or_else(|| panic!("the UI-15 fixture must declare {key}"))
            .iter()
            .map(|value| value.as_str().expect("vocabulary entry").to_owned())
            .collect()
    };
    assert_eq!(
        declared("decisions"),
        WorkbenchDecision::ALL
            .into_iter()
            .map(|decision| decision.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        declared("card_states"),
        InboxCardState::ALL
            .into_iter()
            .map(|state| state.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        declared("artifact_states"),
        ArtifactViewState::ALL
            .into_iter()
            .map(|state| state.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        declared("receipt_display_states"),
        ReceiptDisplayState::ALL
            .into_iter()
            .map(|state| state.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        declared("receipt_updates"),
        ReceiptUpdate::ALL
            .into_iter()
            .map(|update| update.as_str().to_owned())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        declared("review_stages"),
        ReviewRefusalStage::ALL
            .into_iter()
            .map(|stage| stage.as_str().to_owned())
            .collect::<Vec<_>>()
    );

    // 3. every declared fixture is a real behaviour test, and every scenario points at one.
    let names = declared("fixtures");
    assert!(
        names.len() >= 20,
        "the completion slice keeps every named fixture"
    );
    for name in &names {
        assert_eq!(
            fixtures.matches(&format!("fn {name}(")).count(),
            1,
            "UI-15 fixture is not defined exactly once in kiana-entrypoints/tests/ui15_workbench_review.rs: {name}"
        );
    }
    for name in [
        "expired_and_revoked_cards_are_never_actionable_and_never_produce_a_decision",
        "revision_mismatch_and_client_payload_mutation_fail_before_an_action_exists",
        "hand_built_decisions_must_still_match_their_payload_digest",
        "inbox_deduplicates_cards_and_updates_are_never_a_silent_downgrade",
        "artifact_pagination_is_partial_until_every_server_page_is_verified",
        "artifact_pages_outside_the_declared_bounds_are_refused",
        "receipt_never_paints_result_unknown_as_green_completed",
        "receipt_replay_is_idempotent_and_never_regresses_a_terminal_state",
        "receipt_golden_is_deterministic_and_keeps_unknown_and_limitations_visible",
        "review_refusal_catalogue_covers_every_presenter_visible_refusal",
    ] {
        assert!(
            names.contains(&name.to_owned()),
            "the fixture must keep the completion-slice test: {name}"
        );
    }
    assert!(!fixtures.contains("#[ignore"));
    assert!(!fixture_raw.contains("--ignored"));
    let scenarios = fixture["scenarios"].as_array().expect("scenarios");
    assert!(scenarios.len() >= 10);
    // Refusal codes are checked against the module body only: the catalogue itself must not be
    // able to satisfy this assertion.
    let body = source
        .split("pub const REVIEW_REFUSALS")
        .next()
        .expect("the module body precedes the catalogue");
    for scenario in scenarios {
        let name = scenario["fixture"].as_str().expect("scenario fixture");
        assert!(
            names.contains(&name.to_owned()),
            "the UI-15 fixture file does not exercise the declared scenario fixture: {name}"
        );
        assert!(!scenario["id"].as_str().expect("scenario id").is_empty());
        for refusal in scenario["refusals"].as_array().expect("scenario refusals") {
            let refusal = refusal.as_str().expect("scenario refusal");
            assert!(
                body.contains(&format!("\"{refusal}\"")),
                "UI-15 scenario references a refusal the module never raises: {refusal}"
            );
        }
    }

    // 4. the refusal catalogue is complete on both sides: every catalogued code is raised by the
    //    module, every fixture denial is catalogued with a recovery note, and every catalogue entry
    //    is asserted by a real behaviour fixture.
    let denied = declared("denied");
    assert_eq!(denied, ReviewRefusal::codes());
    assert!(denied.len() >= 30, "the catalogue stayed complete");
    for code in &denied {
        assert!(
            body.contains(&format!("\"{code}\"")),
            "the UI-15 module never raises the catalogued refusal: {code}"
        );
        let refusal = ReviewRefusal::find(code).expect("catalogued refusal");
        assert!(!refusal.recovery.trim().is_empty());
        assert!(refusal.recovery.len() <= 120);
        assert!(
            fixtures.contains(&format!("\"{code}\"")),
            "no behaviour fixture asserts the catalogued refusal: {code}"
        );
    }
    for stage in ReviewRefusalStage::ALL {
        assert!(
            denied
                .iter()
                .any(|code| ReviewRefusal::find(code).expect("catalogued").stage == stage),
            "the catalogue has no presenter-visible refusal for stage {}",
            stage.as_str()
        );
    }
    let templates = fixture["denial_templates"]
        .as_object()
        .expect("denial templates");
    assert!(!templates.is_empty());
    for (template, fields) in templates {
        assert!(
            source.contains(template),
            "UI-15 source is missing the denial template: {template}"
        );
        for field in fields.as_array().expect("template fields") {
            let field = field.as_str().expect("template field");
            let composed = template.replace("{field}", field);
            assert!(
                source.contains(&format!("\"{field}\"")),
                "UI-15 source never validates the {field} field"
            );
            assert!(
                fixtures.contains(&format!("\"{composed}\"")),
                "UI-15 fixtures never assert the composed denial code: {composed}"
            );
        }
    }

    // 5. required markers are documented in the baseline and the limitations stay explicit.
    let required = fixture["required"].as_array().expect("required");
    assert!(!required.is_empty());
    for marker in required {
        let marker = marker.as_str().expect("required marker");
        assert!(
            baseline.contains(marker),
            "UI-15 baseline is missing the required marker: {marker}"
        );
    }
    assert!(
        fixture["limitations"]
            .as_array()
            .expect("limitations")
            .len()
            >= 4
    );
    assert_eq!(fixture["diff"]["source"], "server_artifact_ref");
    assert_eq!(fixture["diff"]["client_side_diff"], "forbidden");
    assert_eq!(fixture["recovery"]["catalogue"], "REVIEW_REFUSALS");
    assert_eq!(
        fixture["receipt_golden_schema"],
        "kiana.workbench-receipt-golden.v1"
    );
    assert_eq!(
        fixture["receipt"]["golden"]["schema"],
        "kiana.workbench-receipt-golden.v1"
    );
    assert_eq!(fixture["receipt"]["golden"]["green"], false);
    assert_eq!(fixture["receipt"]["golden"]["display"], "result_unknown");

    // 6. the propagated wire types are real kiana-protocol / kiana-domain contracts.
    for marker in [
        "pub struct HumanActionCard",
        "pub struct UiActionV1",
        "allowed_decisions",
        "payload_digest",
    ] {
        assert!(
            protocol.contains(marker),
            "the propagated protocol contract is missing: {marker}"
        );
    }
    assert!(card.contains("step-ui-15"));

    // 7. the manual lane stays dispatch-only, read-only and bound to the same targets.
    assert!(lane.contains("workflow_dispatch:"));
    assert!(lane.contains("permissions:"));
    assert!(lane.contains("contents: read"));
    for forbidden in [
        "pull_request",
        "  push:",
        "schedule:",
        "continue-on-error",
        "|| true",
        "--ignored",
    ] {
        assert!(
            !lane.contains(forbidden),
            "UI-15 lane must not use {forbidden}"
        );
    }
    for command in [
        "cargo fetch --locked",
        "cargo fmt --all --check",
        "cargo test -p kiana-entrypoints --test ui15_workbench_review --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --test ui15_workbench_review_guard --locked -- --test-threads=1",
        "cargo test -p kiana-entrypoints --no-run --locked",
    ] {
        assert!(lane.contains(command), "UI-15 lane is missing: {command}");
    }

    // 8. the automatic shard book keeps the whole-crate shard that discovers both targets.
    let shards: serde_json::Value =
        serde_json::from_str(shards_raw).expect("test shards must stay valid JSON");
    assert!(
        shards["kiana-entrypoints"]["targets"].is_null(),
        "kiana-entrypoints must stay a whole-crate shard so new UI-15 targets are discovered"
    );
}
