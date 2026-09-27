// Replicates the exact fixture bodies so the assertions can be checked without `cargo test`.
use kiana_domain::{RequestId, RuntimeEvent};
use kiana_query::audit_projector::{
    append_audit_tail, audit_projection_is_append_only, project_audit_from_scratch,
    AuditProjectionPosition, AuditProjectorError, AuditProjectorFold, AuditProjectorFreshness,
};
use serde_json::json;

const PROJECTOR: &str = "kiana-query/audit-projector";
const SUBJECT: &str = "principal:auditor";
const DATA_EPOCH: u64 = 4;

fn committed_event(sequence: u64) -> RuntimeEvent {
    let run_id = format!("run:sc32:{}", sequence);
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        "run.authorized",
        json!({"actor_id": SUBJECT, "run_id": run_id, "authority_epoch": 3,
               "data_epoch": DATA_EPOCH, "data_class": "internal", "retention_class": "audit"}),
    )
    .unwrap()
    .with_stream_metadata("run", run_id, sequence)
}
fn tail_from(sequence: u64) -> Vec<RuntimeEvent> {
    vec![committed_event(sequence)]
}
fn fold(sequences: &[u64]) -> AuditProjectorFold {
    project_audit_from_scratch(
        PROJECTOR,
        &sequences
            .iter()
            .copied()
            .map(committed_event)
            .collect::<Vec<_>>(),
        1,
    )
    .unwrap()
}
fn position(f: &AuditProjectorFold) -> AuditProjectionPosition {
    AuditProjectionPosition::new(
        f.generation,
        f.source_cursor(),
        &f.state_digest(),
        DATA_EPOCH,
    )
    .unwrap()
}
fn check<T>(name: &str, got: Result<T, AuditProjectorError>, want: AuditProjectorError) {
    match got {
        Ok(_) => println!("FAIL  {name}: NO ERROR (expected {want:?})"),
        Err(e) if e == want => println!("PASS  {name}"),
        Err(e) => println!("FAIL  {name}: got {e:?}, want {want:?}"),
    }
}

fn main() {
    println!("--- a_tail_that_starts_before_the_published_cursor_is_a_regression ---");
    let head = fold(&[1, 2]);
    check(
        "assert#1 tail_first=1",
        append_audit_tail(&head, &tail_from(2), 1).map_err(|e| e),
        AuditProjectorError::CursorRegression,
    );
    check(
        "assert#2 tail_first=2",
        append_audit_tail(&head, &tail_from(1), 2).map_err(|e| e),
        AuditProjectorError::CursorRegression,
    );

    println!("--- a_tail_that_re_states_a_projected_source_event_is_refused ---");
    check(
        "replay tail_first=3",
        append_audit_tail(&head, &tail_from(2), 3).map_err(|e| e),
        AuditProjectorError::ProjectionEventReplay,
    );

    println!("--- a_projection_that_would_cover_a_fact_is_refused_by_the_append_only_rule ---");
    let head = fold(&[1, 2]);
    let identities = head.snapshot.source_event_ids.clone();
    let mut records = head.records().to_vec();
    records.pop();
    check(
        "pop -> NotAppendOnly",
        audit_projection_is_append_only(&head, &records, &identities).map_err(|e| e),
        AuditProjectorError::ProjectionNotAppendOnly,
    );
    check(
        "pop + short identities -> SourceRewritten",
        audit_projection_is_append_only(
            &head,
            &records,
            &head.snapshot.source_event_ids[..head.snapshot.source_event_ids.len() - 1],
        )
        .map_err(|e| e),
        AuditProjectorError::ProjectionSourceRewritten,
    );
    let mut records = head.records().to_vec();
    records[0].reason = "amended".to_owned();
    records[0].record_digest = records[0].digest();
    check(
        "edited reason -> RewroteRecord",
        audit_projection_is_append_only(&head, &records, &identities).map_err(|e| e),
        AuditProjectorError::ProjectionRewroteRecord,
    );
    let mut records = head.records().to_vec();
    records.remove(0);
    check(
        "remove row -> DroppedRecord",
        audit_projection_is_append_only(&head, &records, &identities).map_err(|e| e),
        AuditProjectorError::ProjectionDroppedRecord,
    );
    let mut rewritten = head.snapshot.source_event_ids.clone();
    rewritten.remove(0);
    check(
        "short identities, full records -> SourceRewritten",
        audit_projection_is_append_only(&head, head.records(), &rewritten).map_err(|e| e),
        AuditProjectorError::ProjectionSourceRewritten,
    );

    println!(
        "--- a_view_that_rewrote_its_state_under_an_unchanged_generation_is_drift_not_lag ---"
    );
    let h2 = fold(&[1, 2]);
    let other = fold(&[1, 2, 3]);
    let forged = AuditProjectionPosition::new(
        h2.generation,
        h2.source_cursor(),
        &other.state_digest(),
        DATA_EPOCH,
    )
    .unwrap();
    println!(
        "evaluate result: {:?}",
        AuditProjectorFreshness::evaluate(&forged, &h2)
            .map(|v| v.reason)
            .map_err(|e| e)
    );

    println!("--- from_scratch_replay_and_incremental_replay_land_on_the_same_state ---");
    let all = fold(&[1, 2, 3]);
    let head2 = fold(&[1, 2]);
    let inc = append_audit_tail(&head2, &tail_from(3), 3).unwrap();
    println!(
        "all.state == inc.state : {}",
        all.state_digest() == inc.state_digest()
    );
    println!(
        "all.src_evt == inc     : {}",
        all.source_event_digest == inc.source_event_digest
    );
    println!(
        "all.cursor == inc      : {}",
        all.source_cursor() == inc.source_cursor()
    );
    println!(
        "all.rows={} inc.rows={}",
        all.records().len(),
        inc.records().len()
    );
}
