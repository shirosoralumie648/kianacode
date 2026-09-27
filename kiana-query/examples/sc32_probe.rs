//! SC-32 verification probe: checks the exact-fold/rebuild-digest equivalence claim.
use kiana_domain::{json_digest, RequestId, RuntimeEvent};
use kiana_query::audit_projector::{
    append_audit_tail, audit_projection_is_append_only, project_audit_from_scratch,
    AuditProjectionPosition, AuditProjectorFreshness,
};
use serde_json::json;

const PROJECTOR: &str = "kiana-query/audit-projector";

fn ev(sequence: u64) -> RuntimeEvent {
    let run_id = format!("run:sc32:{}", sequence);
    RuntimeEvent::new(
        RequestId::new(),
        sequence,
        "run.authorized",
        json!({"actor_id": "principal:auditor", "run_id": run_id, "authority_epoch": 3,
               "data_epoch": 4, "data_class": "internal", "retention_class": "audit"}),
    )
    .unwrap()
    .with_stream_metadata("run", run_id, sequence)
}

fn main() {
    // 1. from-scratch with 2 events starting at cursor 5
    let a = project_audit_from_scratch(PROJECTOR, &[ev(1), ev(2)], 5).unwrap();
    // 2. same events, but split as a 1-event fold at cursor 5 then an appended tail at cursor 6
    let base = project_audit_from_scratch(PROJECTOR, &[ev(1)], 5).unwrap();
    let b = append_audit_tail(&base, &[ev(2)], 6).unwrap();
    println!(
        "ONE_SHOT state={} cursor={} gen={}",
        a.state_digest(),
        a.source_cursor(),
        a.generation
    );
    println!(
        "SPLIT     state={} cursor={} gen={}",
        b.state_digest(),
        b.source_cursor(),
        b.generation
    );
    println!("EXACT_EQUAL: {}", a.state_digest() == b.state_digest());

    // 3. Does the fold's from-scratch path accept a DUPLICATE event?
    let dup = project_audit_from_scratch(PROJECTOR, &[ev(1), ev(1)], 1);
    println!("DUP from_scratch: {:?}", dup.as_ref().err());

    // 4. Does append_audit_tail reject an empty tail?
    println!(
        "EMPTY tail: {:?}",
        append_audit_tail(&a, &[], a.source_cursor() + 1).err()
    );

    // 5. append_audit_tail when the REDUCER DROPS the record (event kind not in the taxonomy)
    let head = project_audit_from_scratch(PROJECTOR, &[ev(1)], 1).unwrap();
    println!(
        "head records={} cursor={}",
        head.records().len(),
        head.source_cursor()
    );
    let unknown_kind =
        RuntimeEvent::new(RequestId::new(), 2, "session.touched", json!({"note": "x"})).unwrap();
    let gapped = append_audit_tail(&head, &[unknown_kind], 2);
    println!(
        "UNKNOWN_KIND tail: {:?}",
        gapped.as_ref().err().map(|e| e.to_string())
    );
    if let Ok(f) = gapped {
        println!(
            "  -> records before={} after={}  cursor {} -> {}",
            head.records().len(),
            f.records().len(),
            head.source_cursor(),
            f.source_cursor()
        );
        println!(
            "  -> did the cache move FORWARD with ZERO new rows? {}",
            f.source_cursor() > head.source_cursor()
        );
    }

    // 6. Does the append-only rule catch a zero-row advance when called directly?
    let recs = head.records().to_vec();
    let mut ids = head.snapshot.source_event_ids.clone();
    ids.push(kiana_domain::EventId::new());
    println!(
        "APPEND_ONLY zero-row-advance: {:?}",
        audit_projection_is_append_only(&head, &recs, &ids)
            .err()
            .map(|e| e.to_string())
    );

    // 7. The guard's fresh-view fixture: a_view_ahead_of_the_published_head_is_refused_outright
    let pos = AuditProjectionPosition::new(head.generation, 1, &head.state_digest(), 4).unwrap();
    println!(
        "CURSOR_AHEAD: {:?}",
        AuditProjectorFreshness::evaluate(&pos, &head).err()
    );
}
