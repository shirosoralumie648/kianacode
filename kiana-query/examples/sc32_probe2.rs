use kiana_domain::{RequestId, RuntimeEvent};
use kiana_query::audit_projector::{
    append_audit_tail, project_audit_from_scratch, AuditProjectionPosition, AuditProjectorFreshness,
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
    // Duplicate event inside a from-scratch fold: does it produce a projection at all?
    let dup = project_audit_from_scratch(PROJECTOR, &[ev(1), ev(1)], 1);
    println!(
        "DUP from_scratch err: {:?}",
        dup.as_ref().err().map(|e| e.to_string())
    );
    if let Ok(d) = &dup {
        println!(
            "  records={} source_event_ids={}",
            d.records().len(),
            d.snapshot.source_event_ids.len()
        );
    }

    // Does the doc's stated rule hold? ("a tail must begin at exactly source_cursor + 1")
    // head cursor 2; tail_first_cursor 2 -> regression
    let head = project_audit_from_scratch(PROJECTOR, &[ev(1), ev(2)], 1).unwrap();
    println!(
        "tail_first=2 cursor=2: {:?}",
        append_audit_tail(&head, &[ev(3)], 2)
            .err()
            .map(|e| e.to_string())
    );
    println!(
        "tail_first=1 cursor=2: {:?}",
        append_audit_tail(&head, &[ev(3)], 1)
            .err()
            .map(|e| e.to_string())
    );
    println!(
        "tail_first=4 cursor=2: {:?}",
        append_audit_tail(&head, &[ev(3)], 4)
            .err()
            .map(|e| e.to_string())
    );

    // Verify the guard marker line exactly as the guard asserts it.
    let head1 = project_audit_from_scratch(PROJECTOR, &[ev(1)], 1).unwrap();
    let behind =
        AuditProjectionPosition::new(head1.generation, 1, &head1.state_digest(), 4).unwrap();
    println!(
        "cursor_ahead err: {:?}",
        AuditProjectorFreshness::evaluate(&behind, &head1).err()
    );
    println!(
        "behind cursor={} fold cursor={}",
        behind.source_cursor,
        head1.source_cursor()
    );
}
