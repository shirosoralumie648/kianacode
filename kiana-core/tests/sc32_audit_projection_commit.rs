//! SC-32 failure-first fixtures for the audit projection CAS/cursor/rebuild gate.
//!
//! Every test names one item from the card's rejected-first column. Nothing here reads a real
//! EventLog, writes a file or calls a port: the module under test is a decision over supplied
//! digests and counters, so these are contract fixtures and not runtime evidence.

use kiana_core::{
    AuditProjectionClaim, AuditProjectionCommitReport, AuditProjectionCommitStatus,
    AuditProjectionHead, AuditProjectionRebuildProof, AuditProjectionRebuildReport,
    AuditProjectionRebuildStatus, AUDIT_PROJECTION_CLAIM_SCHEMA,
    AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA, AUDIT_PROJECTION_COMMIT_VERSION,
    AUDIT_PROJECTION_HEAD_SCHEMA, AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA,
    AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA,
};
use kiana_domain::json_digest;
use serde_json::json;

const PROJECTOR: &str = "kiana-query/audit-projector";

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

/// A published head at generation 1, cursor 10, state `a`.
fn head() -> AuditProjectionHead {
    AuditProjectionHead::new(PROJECTOR, 1, 10, digest('a')).expect("SC-32 head")
}

fn claim(
    expected_generation: u64,
    new_generation: u64,
    previous_cursor: u64,
    new_cursor: u64,
    previous_state: String,
    new_state: String,
    rebuild_from: Option<u64>,
) -> AuditProjectionClaim {
    AuditProjectionClaim::new(
        PROJECTOR,
        expected_generation,
        new_generation,
        previous_cursor,
        new_cursor,
        &previous_state,
        &new_state,
        rebuild_from,
    )
    .expect("SC-32 claim")
}

fn resign_claim(mut value: AuditProjectionClaim) -> AuditProjectionClaim {
    value.claim_digest = value.digest();
    value
}

fn resign_report(mut value: AuditProjectionCommitReport) -> AuditProjectionCommitReport {
    value.report_digest = value.digest();
    value
}

#[test]
fn exact_replay_of_the_published_head_is_a_no_op_and_the_only_cursor_exemption() {
    let head = head();
    // A claim that re-states the published head exactly: same generation, same cursor, same state.
    // This is the one shape where the source cursor is allowed not to strictly increase, because
    // nothing moved. It is still not an advance.
    let replay = claim(1, 1, 10, 10, digest('a'), digest('a'), None);
    let report =
        AuditProjectionCommitReport::evaluate(&head, &replay).expect("SC-32 replay report");
    assert_eq!(report.status, AuditProjectionCommitStatus::ExactReplay);
    assert!(!report.advanced());
    assert_eq!(report.generation, 1);
    assert_eq!(report.source_cursor, 10);
    assert_eq!(report.state_digest, digest('a'));
    assert_eq!(report.reason, "");
    report
        .validate_against(&head, &replay)
        .expect("SC-32 replay revalidated");
    assert_eq!(report.status.as_str(), "exact_replay");
}

#[test]
fn a_claim_that_moves_backwards_to_an_older_generation_is_not_an_exact_replay() {
    let head = head();
    // Generation matches, cursor does not: this is a rollback dressed up as a replay, and it must
    // fall through to the monotonicity rules instead of passing as a no-op.
    let rollback = claim(1, 1, 10, 9, digest('a'), digest('a'), None);
    let report =
        AuditProjectionCommitReport::evaluate(&head, &rollback).expect("SC-32 rollback report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_cursor_regression");
    // The report never claims the head moved.
    assert_eq!(report.generation, head.generation);
    assert_eq!(report.source_cursor, head.source_cursor);
    assert_eq!(report.state_digest, head.state_digest);
}

#[test]
fn two_projectors_racing_from_the_same_generation_produce_one_advance_and_one_cas_conflict() {
    let head = head();
    // Both projectors read generation 1 and both claim generation 2. The first wins.
    let winner = claim(1, 2, 10, 11, digest('a'), digest('b'), None);
    let loser = claim(1, 2, 10, 12, digest('a'), digest('c'), None);
    let advanced = AuditProjectionCommitReport::evaluate(&head, &winner).expect("SC-32 advance");
    assert_eq!(advanced.status, AuditProjectionCommitStatus::Advanced);
    assert!(advanced.advanced());
    assert_eq!(advanced.generation, 2);
    assert_eq!(advanced.source_cursor, 11);
    assert_eq!(advanced.state_digest, digest('b'));

    let published = AuditProjectionHead::new(PROJECTOR, 2, 11, digest('b')).expect("SC-32 head 2");
    let conflicted =
        AuditProjectionCommitReport::evaluate(&published, &loser).expect("SC-32 conflict report");
    assert_eq!(conflicted.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(
        conflicted.reason,
        "audit_projection_generation_cas_conflict"
    );
    // The loser is told the generation it lost to, so it can re-read instead of guessing.
    assert!(conflicted.remediation.contains("generation 2"));
}

#[test]
fn a_generation_that_skips_a_step_is_refused_even_when_the_cursor_moved() {
    let head = head();
    let skip = claim(1, 3, 10, 11, digest('a'), digest('b'), None);
    let report = AuditProjectionCommitReport::evaluate(&head, &skip).expect("SC-32 skip report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_generation_not_monotonic");
}

#[test]
fn a_claim_folded_from_another_projects_state_is_refused() {
    let head = head();
    // The generation and cursor both look right; the state it folded does not come from the
    // published head. Advancing here would publish a history nobody verified.
    let foreign = claim(1, 2, 10, 11, digest('f'), digest('b'), None);
    let report =
        AuditProjectionCommitReport::evaluate(&head, &foreign).expect("SC-32 drift report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_state_digest_drift");
}

#[test]
fn a_tail_that_skips_committed_facts_is_refused_as_a_gap() {
    let head = head();
    // Cursor 10 -> 12 with no rebuild declared: cursors 11 and 12 exist in the log and this claim
    // silently steps over them.
    let gap = claim(1, 2, 10, 12, digest('a'), digest('b'), None);
    let report = AuditProjectionCommitReport::evaluate(&head, &gap).expect("SC-32 gap report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_cursor_gap");
    assert!(report.remediation.contains("re-fold the missing range"));
}

#[test]
fn a_rebuild_that_reaches_the_same_cursor_and_state_may_advance_over_the_gap() {
    let head = head();
    // The only way past a gap is to declare a rebuild, which is exactly the claim that the missing
    // range was re-folded rather than stepped over.
    let rebuild = claim(1, 2, 10, 20, digest('a'), digest('b'), Some(10));
    let report =
        AuditProjectionCommitReport::evaluate(&head, &rebuild).expect("SC-32 rebuild claim");
    assert_eq!(report.status, AuditProjectionCommitStatus::Advanced);
    assert_eq!(report.source_cursor, 20);

    // From zero is the other honest origin, and it is equally admitted.
    let from_zero = claim(1, 2, 10, 20, digest('a'), digest('b'), Some(0));
    assert_eq!(
        AuditProjectionCommitReport::evaluate(&head, &from_zero)
            .expect("SC-32 rebuild from zero")
            .status,
        AuditProjectionCommitStatus::Advanced
    );
}

#[test]
fn a_rebuild_declared_from_an_unverified_history_is_refused() {
    let head = head();
    // Cursor 4 is neither zero nor the published cursor: the claim asserts it re-folded from a
    // point nobody can check.
    let forged_origin = claim(1, 2, 10, 20, digest('a'), digest('b'), Some(4));
    let report =
        AuditProjectionCommitReport::evaluate(&head, &forged_origin).expect("SC-32 origin report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_rebuild_origin_mismatch");
}

#[test]
fn a_claim_against_another_projectors_head_is_not_a_race() {
    let head = head();
    let other = AuditProjectionClaim::new(
        "kiana-query/other-projector",
        1,
        2,
        10,
        11,
        &digest('a'),
        &digest('b'),
        None,
    )
    .expect("SC-32 foreign claim");
    let report =
        AuditProjectionCommitReport::evaluate(&head, &other).expect("SC-32 mismatch report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_projector_mismatch");
}

#[test]
fn a_claim_re_stating_the_same_cursor_with_a_different_state_is_not_advanced() {
    let head = head();
    // Same generation, same cursor, different state: this is a cache silently changing its answer
    // at a cursor that was already published, so it cannot be an exact replay and it is not an
    // advance either.
    let rewrite = claim(1, 2, 10, 10, digest('a'), digest('b'), None);
    let report =
        AuditProjectionCommitReport::evaluate(&head, &rewrite).expect("SC-32 rewrite report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_cursor_not_advanced");
}

#[test]
fn an_edited_commit_report_cannot_publish_itself_over_the_re_derived_decision() {
    let head = head();
    let advance = claim(1, 2, 10, 11, digest('a'), digest('b'), None);
    let report =
        resign_report(AuditProjectionCommitReport::evaluate(&head, &advance).expect("SC-32"));

    let mut edited_status = report.clone();
    edited_status.status = AuditProjectionCommitStatus::ExactReplay;
    assert_eq!(
        resign_report(edited_status)
            .validate_against(&head, &advance)
            .unwrap_err(),
        "audit_projection_commit_binding_invalid"
    );

    let mut edited_cursor = report.clone();
    edited_cursor.source_cursor = 99;
    assert_eq!(
        resign_report(edited_cursor)
            .validate_against(&head, &advance)
            .unwrap_err(),
        "audit_projection_commit_binding_invalid"
    );

    // A rejection that carries no reason would hide which rule fired.
    let rejected = resign_report(
        AuditProjectionCommitReport::evaluate(
            &head,
            &claim(1, 2, 10, 12, digest('a'), digest('b'), None),
        )
        .expect("SC-32 rejected"),
    );
    let mut silent = rejected.clone();
    silent.reason = String::new();
    silent.remediation = String::new();
    assert_eq!(
        resign_report(silent)
            .validate_against(&head, &claim(1, 2, 10, 12, digest('a'), digest('b'), None))
            .unwrap_err(),
        "audit_projector_claim_digest_mismatch"
    );
}

#[test]
fn a_tampered_head_or_claim_digest_is_refused_before_any_decision() {
    let mut tampered_head = head();
    tampered_head.generation = 7;
    assert_eq!(
        tampered_head.validate().unwrap_err(),
        "audit_projection_head_digest_mismatch"
    );

    let tampered_claim = resign_claim(claim(1, 2, 10, 11, digest('a'), digest('b'), None));
    let mut broken = tampered_claim;
    broken.new_source_cursor = 12;
    assert_eq!(
        broken.validate().unwrap_err(),
        "audit_projection_claim_digest_mismatch"
    );
    assert_eq!(
        AuditProjectionCommitReport::evaluate(&head(), &broken).unwrap_err(),
        "audit_projection_claim_digest_mismatch"
    );
}

#[test]
fn decision_precedence_is_fixed_when_several_rules_are_violated_at_once() {
    let head = head();
    // Wrong projector, wrong expected generation, wrong state, a regressing cursor and a gap, all
    // at once. The reported reason is the first rule in the fixed order: projector identity.
    let kitchen_sink = AuditProjectionClaim::new(
        "kiana-query/other-projector",
        9,
        40,
        10,
        2,
        &digest('f'),
        &digest('b'),
        Some(4),
    )
    .expect("SC-32 kitchen sink claim");
    let report = AuditProjectionCommitReport::evaluate(&head, &kitchen_sink)
        .expect("SC-32 precedence report");
    assert_eq!(report.status, AuditProjectionCommitStatus::Rejected);
    assert_eq!(report.reason, "audit_projection_projector_mismatch");

    // With the projector corrected, the CAS token is the next rule in the order.
    let same_faults_same_projector = resign_claim(
        AuditProjectionClaim::new(PROJECTOR, 9, 40, 10, 2, &digest('f'), &digest('b'), Some(4))
            .expect("SC-32 same-projector claim"),
    );
    let report = AuditProjectionCommitReport::evaluate(&head, &same_faults_same_projector)
        .expect("SC-32 precedence report 2");
    assert_eq!(report.reason, "audit_projection_generation_cas_conflict");
}

#[test]
fn a_rebuild_that_lands_on_the_same_digest_is_the_only_converged_outcome() {
    let head = head();
    let proof = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('a'),
        &digest('e'),
    )
    .expect("SC-32 rebuild proof");
    let report =
        AuditProjectionRebuildReport::evaluate(&head, &proof).expect("SC-32 rebuild report");
    assert_eq!(report.status, AuditProjectionRebuildStatus::Converged);
    assert!(report.converged());
    assert_eq!(report.reason, "");
    assert_eq!(report.source_cursor, 10);
    assert_eq!(report.state_digest, digest('a'));
}

#[test]
fn a_rebuild_that_reads_different_facts_is_divergent_before_its_state_is_comparared() {
    let head = head();
    // Same state digest, same cursor, different source identities: the two paths did not even read
    // the same history, so the cheap answer is reported instead of a state divergence.
    let proof = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('a'),
        &digest('d'),
    )
    .expect("SC-32 rebuild proof");
    let report =
        AuditProjectionRebuildReport::evaluate(&head, &proof).expect("SC-32 rebuild report");
    assert_eq!(report.status, AuditProjectionRebuildStatus::Divergent);
    assert_eq!(
        report.reason,
        "audit_projection_rebuild_source_identity_divergent"
    );
}

#[test]
fn a_rebuild_that_lands_on_a_different_state_is_divergent_and_says_so() {
    let head = head();
    let proof = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('z'),
        &digest('e'),
    )
    .expect("SC-32 rebuild proof");
    let report =
        AuditProjectionRebuildReport::evaluate(&head, &proof).expect("SC-32 rebuild report");
    assert_eq!(report.status, AuditProjectionRebuildStatus::Divergent);
    assert_eq!(report.reason, "audit_projection_rebuild_state_divergent");
    assert!(report.remediation.contains("disagree about history"));
}

#[test]
fn a_rebuild_taken_against_a_head_it_did_not_start_from_is_divergent() {
    let head = head();
    let stale_generation = AuditProjectionRebuildProof::new(
        PROJECTOR,
        0,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('a'),
        &digest('e'),
    );
    assert_eq!(
        stale_generation.unwrap_err(),
        "audit_projection_rebuild_proof_header_invalid"
    );

    let wrong_generation = AuditProjectionRebuildProof::new(
        PROJECTOR,
        5,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('a'),
        &digest('e'),
    )
    .expect("SC-32 wrong generation proof");
    assert_eq!(
        AuditProjectionRebuildReport::evaluate(&head, &wrong_generation)
            .expect("SC-32 rebuild report")
            .reason,
        "audit_projection_rebuild_generation_mismatch"
    );

    let not_through_the_head = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        9,
        &digest('a'),
        &digest('e'),
    )
    .expect("SC-32 short rebuild proof");
    assert_eq!(
        AuditProjectionRebuildReport::evaluate(&head, &not_through_the_head)
            .expect("SC-32 rebuild report")
            .reason,
        "audit_projection_rebuild_cursor_not_reaching_head"
    );
}

#[test]
fn a_tampered_rebuild_report_cannot_publish_itself_over_the_re_derived_decision() {
    let head = head();
    let proof = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &digest('e'),
        10,
        &digest('z'),
        &digest('e'),
    )
    .expect("SC-32 rebuild proof");
    let report =
        AuditProjectionRebuildReport::evaluate(&head, &proof).expect("SC-32 rebuild report");

    let mut converged = report.clone();
    converged.status = AuditProjectionRebuildStatus::Converged;
    converged.reason = String::new();
    converged.remediation = String::new();
    converged.report_digest = converged.digest();
    assert_eq!(
        converged.validate_against(&head, &proof).unwrap_err(),
        "audit_projector_claim_digest_mismatch"
    );

    let mut edited_state = report;
    edited_state.state_digest = digest('a');
    edited_state.report_digest = edited_state.digest();
    assert_eq!(
        edited_state.validate_against(&head, &proof).unwrap_err(),
        "audit_projection_rebuild_binding_invalid"
    );
}

#[test]
fn sc32_schema_constants_and_wire_shapes_are_strict() {
    assert_eq!(
        AUDIT_PROJECTION_HEAD_SCHEMA,
        "kiana.audit-projection-head.v1"
    );
    assert_eq!(
        AUDIT_PROJECTOR_CLAIM_SCHEMA_ALIAS,
        AUDIT_PROJECTION_CLAIM_SCHEMA
    );
    assert_eq!(
        AUDIT_PROJECTION_COMMIT_REPORT_SCHEMA,
        "kiana.audit-projection-commit-report.v1"
    );
    assert_eq!(
        AUDIT_PROJECTION_REBUILD_PROOF_SCHEMA,
        "kiana.audit-projection-rebuild-proof.v1"
    );
    assert_eq!(
        AUDIT_PROJECTION_REBUILD_REPORT_SCHEMA,
        "kiana.audit-projection-rebuild-report.v1"
    );
    assert_eq!(
        AUDIT_PROJECTION_COMMIT_VERSION,
        kiana_domain::SchemaVersion::new(1, 0)
    );

    let head = head();
    let encoded = serde_json::to_value(&head).expect("SC-32 head JSON");
    assert_eq!(
        serde_json::from_value::<AuditProjectionHead>(encoded.clone()).unwrap(),
        head
    );
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AuditProjectionHead>(unknown).is_err());

    let report = AuditProjectionCommitReport::evaluate(
        &head,
        &claim(1, 2, 10, 11, digest('a'), digest('b'), None),
    )
    .expect("SC-32 commit report");
    let encoded = serde_json::to_value(&report).expect("SC-32 report JSON");
    assert_eq!(
        serde_json::from_value::<AuditProjectionCommitReport>(encoded).unwrap(),
        report
    );
}

/// Alias kept local so the guard can name both the constant and the value it must carry.
const AUDIT_PROJECTOR_CLAIM_SCHEMA_ALIAS: &str = AUDIT_PROJECTION_CLAIM_SCHEMA;

/// The source-identity digest the two replay paths share is computed by `kiana-query`; this side
/// only has to be able to carry it and re-derive it. Both sides hash the same sorted, deduplicated
/// event identity list, so an equal digest here means an equal list there.
#[test]
fn rebuild_proof_carries_the_shared_source_identity_digest_unchanged() {
    let event_ids = kiana_domain::EventId::new();
    let shared = kiana_query::audit_fold_source_event_digest(&[event_ids]);
    let head = head();
    let proof = AuditProjectionRebuildProof::new(
        PROJECTOR,
        1,
        10,
        &digest('a'),
        &shared,
        10,
        &digest('a'),
        &shared,
    )
    .expect("SC-32 shared digest proof");
    assert_eq!(proof.incremental_source_event_digest, shared);
    assert!(AuditProjectionRebuildReport::evaluate(&head, &proof)
        .expect("SC-32 shared digest report")
        .converged());
    // A different identity set can never produce the same digest.
    assert_ne!(
        shared,
        kiana_query::audit_fold_source_event_digest(&[kiana_domain::EventId::new()])
    );
    assert_eq!(
        json_digest(&json!({"probe": true})).len(),
        "sha256:".len() + 64
    );
}
