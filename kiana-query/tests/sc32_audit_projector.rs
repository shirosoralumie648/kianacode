//! SC-32 failure-first fixtures for the audit projector.
//!
//! Every test names one item from the card's rejected-first column. The committed events here are
//! ordinary `RuntimeEvent` values handed straight to the fold: no EventStore, no adapter and no
//! file is involved, so a green run here says the fold refuses a bad shape, not that a real log
//! was protected.

use kiana_domain::{json_digest, ProjectionLagStatus, RequestId, RuntimeEvent, SchemaVersion};
use kiana_query::audit_projector::{
    append_audit_tail, audit_fold_source_event_digest, audit_projection_is_append_only,
    project_audit_from_scratch, AuditProjectionPosition, AuditProjectorAuthorization,
    AuditProjectorDisposition, AuditProjectorError, AuditProjectorFold, AuditProjectorFreshness,
    AuditProjectorQuery, AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA, AUDIT_PROJECTOR_DENY_CODES,
    AUDIT_PROJECTOR_FOLD_SCHEMA, AUDIT_PROJECTOR_FRESHNESS_SCHEMA, AUDIT_PROJECTOR_POSITION_SCHEMA,
    AUDIT_PROJECTOR_QUERY_SCHEMA, AUDIT_PROJECTOR_VERSION, MAX_AUDIT_PROJECTOR_QUERY_PAGE,
};
use kiana_query::data_boundary::QueryDataBoundary;
use serde_json::json;

const PROJECTOR: &str = "kiana-query/audit-projector";
const PROJECT_ROOT: &str = "/workspace/kiana";
const SUBJECT: &str = "principal:auditor";
const DATA_EPOCH: u64 = 4;

fn digest(seed: char) -> String {
    format!(
        "sha256:{}",
        format!("{:04x}", (seed as u32) & 0xffff).repeat(16)
    )
}

/// A committed event the audit reducer recognises. `run.authorized` is an authorization decision
/// on a `run` aggregate, which is the shape the SC-31 taxonomy registers.
fn committed_event(sequence: u64) -> RuntimeEvent {
    let run_id = format!("run:sc32:{}", sequence);
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        "run.authorized",
        json!({
            "actor_id": SUBJECT,
            "run_id": run_id,
            "authority_epoch": 3,
            "data_epoch": DATA_EPOCH,
            "data_class": "internal",
            "retention_class": "audit",
        }),
    )
    .expect("SC-32 committed event")
    .with_stream_metadata("run", run_id, sequence)
}

fn tail_from(sequence: u64) -> Vec<RuntimeEvent> {
    vec![committed_event(sequence)]
}

/// A fold at generation 1 covering the given committed sequences, starting at cursor 1.
fn fold(sequences: &[u64]) -> AuditProjectorFold {
    let events = sequences
        .iter()
        .copied()
        .map(committed_event)
        .collect::<Vec<_>>();
    project_audit_from_scratch(PROJECTOR, &events, 1).expect("SC-32 fold")
}

/// The position a fold publishes once it has been committed.
fn position(fold: &AuditProjectorFold) -> AuditProjectionPosition {
    AuditProjectionPosition::new(
        fold.generation,
        fold.source_cursor(),
        &fold.state_digest(),
        DATA_EPOCH,
    )
    .expect("SC-32 position")
}

/// A published head one generation and one cursor ahead of the fold it is compared against. Used
/// wherever a test needs a view that is genuinely stale rather than merely re-labelled.
fn published_ahead_of(fold: &AuditProjectorFold) -> AuditProjectionPosition {
    let ahead = append_audit_tail(
        fold,
        &tail_from(fold.source_cursor() + 1),
        fold.source_cursor() + 1,
    )
    .expect("SC-32 advanced fold");
    position(&ahead)
}

fn boundary(scope_digest: &str) -> QueryDataBoundary {
    QueryDataBoundary::derive(PROJECT_ROOT, scope_digest, DATA_EPOCH, false, true, true)
        .expect("SC-32 boundary")
}

fn query(boundary: &QueryDataBoundary, after_cursor: u64, limit: usize) -> AuditProjectorQuery {
    AuditProjectorQuery::new(
        SUBJECT,
        PROJECT_ROOT,
        &boundary.scope_digest,
        &boundary.decision_digest,
        after_cursor,
        limit,
    )
    .expect("SC-32 query")
}

fn freshness(
    position: &AuditProjectionPosition,
    fold: &AuditProjectorFold,
) -> AuditProjectorFreshness {
    AuditProjectorFreshness::evaluate(position, fold).expect("SC-32 freshness")
}

fn decide(
    query: &AuditProjectorQuery,
    fold: &AuditProjectorFold,
    position: &AuditProjectionPosition,
    boundary: &QueryDataBoundary,
) -> AuditProjectorAuthorization {
    let view = freshness(position, fold);
    AuditProjectorAuthorization::evaluate(query, fold, position, boundary, &view)
        .expect("SC-32 authorization")
}

#[test]
fn from_scratch_replay_and_incremental_replay_land_on_the_same_state() {
    // The card's positive requirement: two paths, one answer. This is the only fixture that
    // asserts convergence, and it asserts it on the digests both paths actually publish.
    let all = fold(&[1, 2, 3]);
    let head = fold(&[1, 2]);
    let incremental = append_audit_tail(&head, &tail_from(3), 3).expect("SC-32 incremental tail");

    assert_eq!(all.state_digest(), incremental.state_digest());
    assert_eq!(all.source_event_digest, incremental.source_event_digest);
    assert_eq!(all.source_cursor(), incremental.source_cursor());
    assert_eq!(all.records().len(), incremental.records().len());
    // The two paths took a different number of publishes and still agree on history; the
    // generation is the CAS counter, not part of the answer.
    assert_eq!(all.generation, 1);
    assert_eq!(incremental.generation, 2);
    // Every earlier record is byte-identical in the incrementally advanced fold.
    for record in head.records() {
        let same = incremental
            .records()
            .iter()
            .find(|candidate| candidate.audit_id == record.audit_id)
            .expect("SC-32 retained record");
        assert_eq!(same.record_digest, record.record_digest);
    }
}

#[test]
fn a_projection_that_would_cover_a_fact_is_refused_by_the_append_only_rule() {
    let head = fold(&[1, 2]);
    let mut records = head.records().to_vec();
    let identities = head.snapshot.source_event_ids.clone();

    // The extension the reducer produces is the one shape the rule admits.
    let appended = fold(&[1, 2, 3]);
    audit_projection_is_append_only(
        &head,
        appended.records(),
        &appended.snapshot.source_event_ids,
    )
    .expect("SC-32 append-only extension");

    // A cache that dropped a row has un-answered a question it already answered.
    records.pop();
    assert_eq!(
        audit_projection_is_append_only(&head, &records, &identities).unwrap_err(),
        AuditProjectorError::ProjectionNotAppendOnly
    );
    // Fewer rows still leaves the dropped identity behind, so the identity prefix also catches it.
    assert_eq!(
        audit_projection_is_append_only(
            &head,
            &records,
            &head.snapshot.source_event_ids[..head.snapshot.source_event_ids.len() - 1]
        )
        .unwrap_err(),
        AuditProjectorError::ProjectionSourceRewritten
    );

    // A row edited in place under the same identity is a cache that changed its answer.
    records = head.records().to_vec();
    records[0].reason = "amended".to_owned();
    records[0].record_digest = records[0].digest();
    assert_eq!(
        audit_projection_is_append_only(&head, &records, &identities).unwrap_err(),
        AuditProjectorError::ProjectionRewroteRecord
    );

    // A row swapped for a different identity while the list length stays put. The length guard
    // cannot see this, so the per-identity check is the only thing that notices the answer the
    // projection already gave is no longer present under the identity it gave it.
    let mut substituted = head.records().to_vec();
    substituted[0].audit_id = "audit:sc32:substituted".to_owned();
    substituted[0].record_digest = substituted[0].digest();
    records = substituted;
    assert_eq!(
        audit_projection_is_append_only(&head, &records, &identities).unwrap_err(),
        AuditProjectorError::ProjectionDroppedRecord
    );

    // A history that is not the previous one plus more is a rewrite of what the log said.
    let mut rewritten = head.snapshot.source_event_ids.clone();
    rewritten.remove(0);
    assert_eq!(
        audit_projection_is_append_only(&head, head.records(), &rewritten).unwrap_err(),
        AuditProjectorError::ProjectionSourceRewritten
    );
}

#[test]
fn a_tail_that_re_states_a_projected_source_event_is_refused() {
    // The identity check compares event_id, and committed_event mints a fresh random one on every
    // call, so re-stating a sequence is not the same as re-stating an event. Fold and re-tail the
    // SAME event object. That fold publishes cursor 1, so the tail starts at 2: both cursor checks
    // pass and the identity check is the only thing left that can fire.
    let replayed_event = committed_event(2);
    let head =
        project_audit_from_scratch(PROJECTOR, &[replayed_event.clone()], 1).expect("SC-32 fold");
    assert_eq!(
        append_audit_tail(&head, &[replayed_event], 2).unwrap_err(),
        AuditProjectorError::ProjectionEventReplay
    );
}

#[test]
fn a_tail_that_starts_before_the_published_cursor_is_a_regression() {
    let head = fold(&[1, 2]);
    // The head published cursor 2, so a tail starting below it is a regression. A tail starting
    // exactly at 2 is a gap instead, which the next fixture covers.
    assert_eq!(
        append_audit_tail(&head, &tail_from(2), 1).unwrap_err(),
        AuditProjectorError::CursorRegression
    );
}

#[test]
fn a_tail_that_skips_committed_cursors_is_a_gap() {
    let head = fold(&[1, 2]);
    // Cursor 4 when the projection stops at 2: cursor 3 exists in the log and is being stepped
    // over. That is a different fault from a regression, so it gets a different code.
    assert_eq!(
        append_audit_tail(&head, &tail_from(4), 4).unwrap_err(),
        AuditProjectorError::CursorGap
    );
}

#[test]
fn a_source_that_cannot_be_folded_is_not_a_projection() {
    assert_eq!(
        project_audit_from_scratch(PROJECTOR, &[], 1).unwrap_err(),
        AuditProjectorError::SourceEmpty
    );
    assert_eq!(
        project_audit_from_scratch(PROJECTOR, &tail_from(1), 0).unwrap_err(),
        AuditProjectorError::CursorRequired
    );
    // An untrusted `audit.*` event cannot establish its own provenance, so the SC-31 taxonomy
    // refuses it and no projection is produced from it.
    let forged = RuntimeEvent::new(
        RequestId::new(),
        1,
        "audit.record",
        json!({"audit_id": "x"}),
    )
    .expect("SC-32 forged event");
    assert_eq!(
        project_audit_from_scratch(PROJECTOR, &[forged], 1).unwrap_err(),
        AuditProjectorError::ReduceFailed("audit_event_kind_untrusted".to_owned())
    );
}

#[test]
fn the_current_view_is_fresh_and_agrees_with_the_shared_lag_vocabulary() {
    let head = fold(&[1, 2]);
    let position = position(&head);
    let view = freshness(&position, &head);
    assert!(view.fresh);
    assert_eq!(view.reason, "");
    assert_eq!(view.remediation, "");
    assert_eq!(view.view_generation, head.generation);
    assert_eq!(view.view_state_digest, head.state_digest());
    assert_eq!(view.schema, AUDIT_PROJECTOR_FRESHNESS_SCHEMA);
    // A caught-up projection reads as caught up through the existing domain lag contract, not a
    // SC-32-specific vocabulary.
    assert_eq!(view.lag.status, ProjectionLagStatus::CaughtUp);
    assert!(view.lag.read_consistent());
}

#[test]
fn a_view_from_an_older_publish_is_never_fresh() {
    let head = fold(&[1, 2]);
    let published = published_ahead_of(&head);
    // The same projection, answered against a head that has moved on.
    let stale = freshness(&published, &head);
    assert!(!stale.fresh);
    assert_eq!(stale.reason, "audit_projector_view_generation_stale");
    assert_eq!(stale.view_generation, head.generation);
    assert!(!stale.lag.read_consistent());
    assert_eq!(stale.lag.status, ProjectionLagStatus::Pending);
    // The lag view names the projection generation the head is at, which is what a caller needs
    // to re-read the right thing.
    assert_eq!(stale.lag.projection_generation, published.generation);
    assert_eq!(stale.lag.data_epoch, DATA_EPOCH);
}

#[test]
fn a_view_that_rewrote_its_state_under_an_unchanged_generation_is_drift_not_lag() {
    let head = fold(&[1, 2]);
    // Same generation and cursor, different folded bytes: the projection under this name is not
    // the one that was published, so it must not be answerable as fresh.
    let other = fold(&[1, 2, 3]);
    let forged = AuditProjectionPosition::new(
        head.generation,
        head.source_cursor(),
        &other.state_digest(),
        DATA_EPOCH,
    )
    .expect("SC-32 forged position");
    let view = freshness(&forged, &head);
    assert!(!view.fresh);
    assert_eq!(view.reason, "audit_projector_view_state_drift");
    // Drift is still a lag in the shared vocabulary, because the fold is behind the published
    // cursor only in name; the reason is what tells the two apart.
    assert!(!view.lag.read_consistent());
}

#[test]
fn a_view_ahead_of_the_published_head_is_refused_outright() {
    let head = fold(&[1, 2]);
    // An unpublished projection is not a stale one. Labelling it stale would tell the caller to
    // re-read a head that is behind it.
    let behind = AuditProjectionPosition::new(head.generation, 1, &head.state_digest(), DATA_EPOCH)
        .expect("SC-32 behind position");
    assert_eq!(
        AuditProjectorFreshness::evaluate(&behind, &head).unwrap_err(),
        "audit_projector_view_cursor_ahead"
    );
}

#[test]
fn an_edited_freshness_view_cannot_declare_itself_fresh() {
    let head = fold(&[1, 2]);
    let published = published_ahead_of(&head);
    let stale = freshness(&published, &head);

    let mut forged = stale.clone();
    forged.fresh = true;
    forged.reason = String::new();
    forged.remediation = String::new();
    forged.view_digest = forged.digest();
    assert_eq!(
        forged.validate_against(&published, &head).unwrap_err(),
        "audit_projector_view_freshness_mismatch"
    );
    // The untouched stale view is still self-consistent: it is the edit above that fails, not the
    // staleness itself.
    stale
        .validate_against(&published, &head)
        .expect("SC-32 stale view revalidates");
    assert!(!stale.fresh);
}

#[test]
fn a_query_for_another_project_root_is_denied() {
    let head = fold(&[1, 2]);
    let position = position(&head);
    let boundary = boundary(&digest('s'));
    // The caller names its own project root while the boundary was derived for another. This is
    // the scope crossing the card names, and it is refused before any freshness reasoning.
    let crossing = AuditProjectorQuery::new(
        SUBJECT,
        "/workspace/other",
        &boundary.scope_digest,
        &boundary.decision_digest,
        0,
        10,
    )
    .expect("SC-32 crossing query");
    let authorization = decide(&crossing, &head, &position, &boundary);
    assert_eq!(authorization.disposition, AuditProjectorDisposition::Denied);
    assert_eq!(authorization.reason, AUDIT_PROJECTOR_DENY_CODES[0]);
    assert!(!authorization.allowed());
    let view = freshness(&position, &head);
    assert_eq!(
        authorization
            .page(&crossing, &head, &position, &boundary, &view)
            .unwrap_err(),
        AuditProjectorError::AuthorizationDenied(AUDIT_PROJECTOR_DENY_CODES[0].to_owned())
    );
}

#[test]
fn a_query_with_a_self_asserted_scope_digest_is_denied() {
    let head = fold(&[1, 2]);
    let position = position(&head);
    let boundary = boundary(&digest('s'));
    // The caller recomputes the scope digest it likes but cannot produce the server-derived
    // decision digest, so the scope it presents is not the scope that was granted.
    let self_asserted = AuditProjectorQuery::new(
        SUBJECT,
        PROJECT_ROOT,
        &digest('x'),
        &boundary.decision_digest,
        0,
        10,
    )
    .expect("SC-32 self-asserted query");
    let authorization = decide(&self_asserted, &head, &position, &boundary);
    assert_eq!(
        authorization.reason,
        "audit_projector_query_scope_digest_mismatch"
    );
    assert_eq!(authorization.reason, AUDIT_PROJECTOR_DENY_CODES[1]);
}

#[test]
fn a_query_without_the_boundary_decision_digest_is_denied() {
    let head = fold(&[1, 2]);
    let position = position(&head);
    let boundary = boundary(&digest('s'));
    // Right project, right scope digest, but the authority is the caller's own string.
    let forged = AuditProjectorQuery::new(
        SUBJECT,
        PROJECT_ROOT,
        &boundary.scope_digest,
        &digest('q'),
        0,
        10,
    )
    .expect("SC-32 forged authority query");
    let authorization = decide(&forged, &head, &position, &boundary);
    assert_eq!(authorization.reason, AUDIT_PROJECTOR_DENY_CODES[2]);
}

#[test]
fn a_revoked_boundary_denies_the_whole_projection() {
    let head = fold(&[1, 2]);
    let position = position(&head);
    let revoked =
        QueryDataBoundary::derive(PROJECT_ROOT, digest('s'), DATA_EPOCH, true, true, true)
            .expect("SC-32 revoked boundary");
    let request = query(&revoked, 0, 10);
    let authorization = decide(&request, &head, &position, &revoked);
    assert_eq!(authorization.reason, AUDIT_PROJECTOR_DENY_CODES[3]);
    assert_eq!(
        authorization.reason,
        "audit_projector_query_boundary_denied"
    );
}

#[test]
fn an_unbounded_page_is_refused() {
    assert_eq!(
        AuditProjectorQuery::new(SUBJECT, PROJECT_ROOT, &digest('s'), &digest('a'), 0, 0)
            .unwrap_err(),
        "audit_projector_query_header_invalid"
    );
    assert_eq!(
        AuditProjectorQuery::new(
            SUBJECT,
            PROJECT_ROOT,
            &digest('s'),
            &digest('a'),
            0,
            MAX_AUDIT_PROJECTOR_QUERY_PAGE + 1,
        )
        .unwrap_err(),
        "audit_projector_query_header_invalid"
    );
}

#[test]
fn a_stale_view_is_refused_the_read_even_within_scope() {
    let head = fold(&[1, 2]);
    let published = published_ahead_of(&head);
    let boundary = boundary(&digest('s'));
    // An old cached projection, asked a question it is no longer the answer to.
    let request = query(&boundary, 0, 10);
    let authorization = decide(&request, &head, &published, &boundary);
    assert_eq!(
        authorization.reason,
        "audit_projector_view_generation_stale"
    );
    let view = freshness(&published, &head);
    assert_eq!(
        authorization
            .page(&request, &head, &published, &boundary, &view)
            .unwrap_err(),
        AuditProjectorError::AuthorizationDenied(
            "audit_projector_view_generation_stale".to_owned()
        )
    );
}

#[test]
fn a_cursor_past_the_projection_is_denied() {
    let head = fold(&[1, 2]);
    let position = position(&head);
    let boundary = boundary(&digest('s'));
    let ahead = query(&boundary, 99, 10);
    let authorization = decide(&ahead, &head, &position, &boundary);
    assert_eq!(authorization.reason, AUDIT_PROJECTOR_DENY_CODES[4]);
    assert_eq!(
        authorization.reason,
        "audit_projector_query_after_cursor_ahead"
    );
}

#[test]
fn an_in_scope_query_on_a_fresh_view_returns_only_rows_after_the_cursor() {
    let head = fold(&[1, 2, 3]);
    let position = position(&head);
    let boundary = boundary(&digest('s'));
    let request = query(&boundary, 1, 10);
    let view = freshness(&position, &head);
    let authorization =
        AuditProjectorAuthorization::evaluate(&request, &head, &position, &boundary, &view)
            .expect("SC-32 in-scope authorization");
    assert_eq!(
        authorization.disposition,
        AuditProjectorDisposition::Allowed
    );
    assert_eq!(authorization.reason, "");
    let page = authorization
        .page(&request, &head, &position, &boundary, &view)
        .expect("SC-32 page");
    assert_eq!(page.len(), 2);
    assert!(page.iter().all(|record| record.source_cursor > 1));
    // The page is pinned to the exact read model the decision was made on.
    assert_eq!(authorization.fold_state_digest, head.state_digest());
    assert_eq!(authorization.schema, AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA);
}

#[test]
fn a_deserialized_allowed_authorization_cannot_be_replayed_against_another_projection() {
    let head = fold(&[1, 2]);
    let head_position = position(&head);
    let boundary = boundary(&digest('s'));
    let request = query(&boundary, 0, 10);
    let view = freshness(&head_position, &head);
    let authorization =
        AuditProjectorAuthorization::evaluate(&request, &head, &head_position, &boundary, &view)
            .expect("SC-32 in-scope authorization");
    assert!(authorization.allowed());

    // A different projection at the same generation and cursor, then the same authorization
    // against it. The decision is re-derived, so a grant does not follow the object.
    let other = fold(&[1, 2, 3]);
    let other_position = position(&other);
    assert_eq!(
        authorization
            .page(&request, &other, &other_position, &boundary, &view)
            .unwrap_err(),
        AuditProjectorError::AuthorizationDenied(
            "audit_projector_view_invalid:audit_projector_view_binding_invalid".to_owned()
        )
    );
    // The grant still works for the projection it was decided on.
    assert_eq!(
        authorization
            .page(&request, &head, &head_position, &boundary, &view)
            .expect("SC-32 authorized page")
            .len(),
        2
    );
}

#[test]
fn a_tampered_fold_is_refused_before_it_is_read() {
    let head = fold(&[1, 2]);
    let mut tampered = head.clone();
    tampered.snapshot.source_cursor = 99;
    assert!(matches!(
        tampered.validate(),
        Err(AuditProjectorError::SnapshotInvalid(_))
    ));

    // The source identity digest is what makes the two replay paths comparable, so an edited
    // identity list is refused even when the snapshot still validates.
    let mut rewritten = head.clone();
    rewritten.source_event_digest = audit_fold_source_event_digest(&[]);
    assert_eq!(
        rewritten.validate().unwrap_err(),
        AuditProjectorError::ProjectionSourceRewritten
    );
}

#[test]
fn sc32_schema_constants_and_wire_shapes_are_strict() {
    assert_eq!(AUDIT_PROJECTOR_FOLD_SCHEMA, "kiana.audit-projector-fold.v1");
    assert_eq!(
        AUDIT_PROJECTOR_POSITION_SCHEMA,
        "kiana.audit-projector-position.v1"
    );
    assert_eq!(
        AUDIT_PROJECTOR_QUERY_SCHEMA,
        "kiana.audit-projector-query.v1"
    );
    assert_eq!(
        AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA,
        "kiana.audit-projector-authorization.v1"
    );
    assert_eq!(AUDIT_PROJECTOR_VERSION, SchemaVersion::new(1, 0));
    assert_eq!(AUDIT_PROJECTOR_DENY_CODES.len(), 6);
    assert_eq!(
        AUDIT_PROJECTOR_DENY_CODES[0],
        "audit_projector_query_project_mismatch"
    );
    assert_eq!(
        AUDIT_PROJECTOR_DENY_CODES[5],
        "audit_projector_view_generation_stale"
    );

    let head = fold(&[1, 2]);
    let encoded = serde_json::to_value(&head).expect("SC-32 fold JSON");
    assert_eq!(
        serde_json::from_value::<AuditProjectorFold>(encoded.clone()).unwrap(),
        head
    );
    let mut unknown = encoded;
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<AuditProjectorFold>(unknown).is_err());

    // The source identity digest is a digest of a sorted, deduplicated list, so order and repeats
    // cannot move it. Both replay paths depend on that.
    let ids = head.snapshot.source_event_ids.clone();
    let mut shuffled = ids.clone();
    shuffled.reverse();
    assert_eq!(
        audit_fold_source_event_digest(&ids),
        audit_fold_source_event_digest(&shuffled)
    );
    let mut repeated = ids.clone();
    repeated.extend(ids.iter().copied());
    assert_eq!(
        audit_fold_source_event_digest(&ids),
        audit_fold_source_event_digest(&repeated)
    );
    assert_eq!(
        json_digest(&json!({"probe": true})).len(),
        "sha256:".len() + 64
    );
}
