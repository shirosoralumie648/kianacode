use kiana_protocol::{CellId, ChildHarnessOutcome, ChildHarnessOutcomeKind, RunId};

#[test]
fn child_outcome_wire_shape_carries_refs_not_transcript() {
    let outcome = ChildHarnessOutcome::new(
        RunId::new(),
        RunId::new(),
        CellId::new(),
        ChildHarnessOutcomeKind::Completed,
        "summary",
        vec!["artifact:one".to_owned()],
        vec!["event:one".to_owned()],
    )
    .unwrap();
    let encoded = serde_json::to_value(outcome).unwrap();
    assert_eq!(encoded["schema"], "kiana.child-harness-outcome.v1");
    assert_eq!(encoded["transcript_forwarded"], false);
    assert!(encoded.get("transcript").is_none());
}
