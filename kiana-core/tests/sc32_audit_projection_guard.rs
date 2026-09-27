//! SC-32 source guard: the audit projector is a rebuildable cache with a CAS, a monotonic cursor
//! and a rebuild that has to agree with incremental replay.
//!
//! These are source-text assertions, not behavioural ones. They pin the properties the card names
//! to the file that implements them so a later edit cannot quietly drop one: every marker below is
//! grepped for literally, and every forbidden string is checked to be genuinely absent.

#[test]
fn sc32_commit_module_pins_the_cas_and_the_cursor_rules() {
    let commit = include_str!("../src/audit_projection_commit.rs");
    for marker in [
        "AuditProjectionHead",
        "AuditProjectionClaim",
        "AuditProjectionCommitReport",
        "AuditProjectionCommitStatus",
        "AuditProjectionRebuildProof",
        "AuditProjectionRebuildReport",
        "AuditProjectionRebuildStatus",
        "AUDIT_PROJECTION_HEAD_SCHEMA",
        "AUDIT_PROJECTION_CLAIM_SCHEMA",
        "AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA",
        "AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA",
        "AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA",
        // The three publish outcomes, and the one that is exempt from cursor monotonicity.
        "Advanced",
        "ExactReplay",
        "audit_projection_generation_cas_conflict",
        "audit_projection_generation_not_monotonic",
        "audit_projection_cursor_regression",
        "audit_projection_cursor_not_advanced",
        "audit_projection_cursor_gap",
        "audit_projection_state_digest_drift",
        "audit_projection_rebuild_origin_mismatch",
        "audit_projection_projector_mismatch",
        "audit_projection_rebuild_source_identity_divergent",
        "audit_projection_rebuild_state_divergent",
        "audit_projection_rebuild_cursor_not_reaching_head",
        "validate_against",
        "deny_unknown_fields",
    ] {
        assert!(
            commit.contains(marker),
            "SC-32 commit marker missing: {marker}"
        );
    }
    // The CAS rule is the load-bearing one: the claim carries the generation it *read*, and only a
    // claim whose expected generation is the published one may advance it.
    assert!(
        commit.contains("if claim.expected_generation != head.generation {"),
        "SC-32 must compare the expected generation against the published head"
    );
    assert!(
        commit.contains("if claim.new_generation != head.generation.saturating_add(1) {"),
        "SC-32 must advance the generation by exactly one"
    );
    // A report is re-derived, never trusted: `evaluate` and `validate_against` both call `derive`.
    assert_eq!(
        commit.matches("derive(head, claim)").count(),
        2,
        "SC-32 must re-derive the commit decision in both evaluate and validate_against"
    );
    assert_eq!(
        commit.matches("derive_rebuild(head, proof)").count(),
        2,
        "SC-32 must re-derive the rebuild decision in both evaluate and validate_against"
    );
    // This is a decision over supplied digests. It stores nothing, calls nothing and writes no
    // fact, and the guard has to keep it that way.
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "ProjectionStorePort",
        "CapabilityBrokerPort",
        "ControlPlane",
        "std::sync::",
        "Mutex",
        "RwLock",
    ] {
        assert!(
            !commit.contains(forbidden),
            "SC-32 commit module crossed the effect boundary: {forbidden}"
        );
    }
}

#[test]
fn sc32_projector_module_pins_append_only_freshness_and_scope() {
    let projector = include_str!("../../kiana-query/src/audit_projector.rs");
    for marker in [
        "AuditProjectorFold",
        "AuditProjectionPosition",
        "AuditProjectorFreshness",
        "AuditProjectorQuery",
        "AuditProjectorAuthorization",
        "AuditProjectorDisposition",
        "AUDIT_PROJECTOR_FOLD_SCHEMA",
        "AUDIT_PROJECTOR_POSITION_SCHEMA",
        "AUDIT_PROJECTOR_FRESHNESS_SCHEMA",
        "AUDIT_PROJECTOR_QUERY_SCHEMA",
        "AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA",
        "AUDIT_PROJECTOR_DENY_CODES",
        // A cache that can cover a fact.
        "audit_projection_is_append_only",
        "assert_append_only",
        "ProjectionDroppedRecord",
        "ProjectionRewroteRecord",
        "ProjectionNotAppendOnly",
        "ProjectionSourceRewritten",
        "ProjectionEventReplay",
        "CursorRegression",
        "CursorGap",
        // A stale view presented as current.
        "audit_projector_view_generation_stale",
        "audit_projector_view_state_drift",
        "audit_projector_view_cursor_ahead",
        "ProjectionLagView",
        // A query outside its scope.
        "audit_projector_query_project_mismatch",
        "audit_projector_query_scope_digest_mismatch",
        "audit_projector_query_authority_digest_mismatch",
        "audit_projector_query_boundary_denied",
        "audit_projector_query_after_cursor_ahead",
        "QueryDataBoundary",
        "QueryDataDisposition",
        "deny_unknown_fields",
    ] {
        assert!(
            projector.contains(marker),
            "SC-32 projector marker missing: {marker}"
        );
    }
    // The two replay paths have to land on the same answer. The projector exposes both of them, and
    // the fixture is what actually compares them; the guard pins the two entry points so neither
    // can be deleted without the comparison losing an arm.
    assert!(
        projector.contains("pub fn project_audit_from_scratch("),
        "SC-32 must keep a from-scratch replay path"
    );
    assert!(
        projector.contains("pub fn append_audit_tail("),
        "SC-32 must keep an incremental replay path"
    );
    let fixture = include_str!("../../kiana-query/tests/sc32_audit_projector.rs");
    assert!(
        fixture.contains("assert_eq!(all.state_digest(), incremental.state_digest());"),
        "SC-32 fixture must compare the incremental and from-scratch state digests"
    );
    assert!(
        projector.contains("AuditProjectionSnapshot::new(1, last_cursor"),
        "SC-32 must derive a projection from the committed source, not a caller-supplied one"
    );
    // The projector folds. It does not append, persist or read a store.
    for forbidden in [
        "std::fs",
        "std::process",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "ControlPlane",
        "append_event",
        "persist",
    ] {
        assert!(
            !projector.contains(forbidden),
            "SC-32 projector crossed the effect boundary: {forbidden}"
        );
    }
    // The projector reuses the existing governance and recovery vocabularies rather than
    // inventing a second one for either.
    for reused in [
        "kiana_domain::reduce_audit_records",
        "AuditProjectionSnapshot",
        "ProjectionLagView",
        "QueryDataBoundary",
        "MAX_SOURCE_EVENT_IDS",
        "redact_text",
        "scan_secret_sentinels",
    ] {
        assert!(
            projector.contains(reused),
            "SC-32 projector must reuse {reused}"
        );
    }
}

#[test]
fn sc32_crates_are_wired_without_touching_the_shared_domain_registry() {
    let core_lib = include_str!("../src/lib.rs");
    let query_lib = include_str!("../../kiana-query/src/lib.rs");
    assert!(
        core_lib.contains("mod audit_projection_commit;"),
        "SC-32 core module must be declared"
    );
    assert!(
        core_lib.contains("AUDIT_PROJECTION_CLAIM_SCHEMA"),
        "SC-32 core re-exports must name the claim schema"
    );
    assert!(
        query_lib.contains("pub mod audit_projector;"),
        "SC-32 query module must be declared"
    );
    assert!(
        query_lib.contains("AUDIT_PROJECTOR_FRESHNESS_SCHEMA"),
        "SC-32 query re-exports must name the freshness schema"
    );
    // SC-32 adds no new kiana-domain module, so the shared registry the integration owner owns is
    // untouched by this slice.
    assert!(
        !core_lib.contains("mod audit_projection_commit_v2;"),
        "SC-32 must not fork the commit contract"
    );
}
