use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn observation(
    adapter: Er32AdapterKind,
    operation: Er32Operation,
    result: Er32ResultClass,
    durable: bool,
    sync_ack: bool,
    byte: char,
) -> Er32AdapterObservation {
    Er32AdapterObservation::new(
        adapter,
        operation,
        hash('s'),
        hash(byte),
        result,
        durable,
        sync_ack,
    )
}

fn report() -> Er32ConformanceReport {
    Er32ConformanceReport::new(
        32,
        vec![
            observation(
                Er32AdapterKind::Memory,
                Er32Operation::Commit,
                Er32ResultClass::Committed,
                false,
                false,
                'a',
            ),
            observation(
                Er32AdapterKind::Jsonl,
                Er32Operation::Commit,
                Er32ResultClass::Committed,
                true,
                true,
                'a',
            ),
            observation(
                Er32AdapterKind::Memory,
                Er32Operation::Replay,
                Er32ResultClass::Replayed,
                false,
                false,
                'b',
            ),
            observation(
                Er32AdapterKind::Jsonl,
                Er32Operation::Replay,
                Er32ResultClass::Replayed,
                true,
                true,
                'b',
            ),
            observation(
                Er32AdapterKind::Memory,
                Er32Operation::Conflict,
                Er32ResultClass::Conflict,
                false,
                false,
                'c',
            ),
            observation(
                Er32AdapterKind::Jsonl,
                Er32Operation::Conflict,
                Er32ResultClass::Conflict,
                true,
                true,
                'c',
            ),
            observation(
                Er32AdapterKind::Memory,
                Er32Operation::CursorGap,
                Er32ResultClass::SnapshotRequired,
                false,
                false,
                'd',
            ),
            observation(
                Er32AdapterKind::Jsonl,
                Er32Operation::CursorGap,
                Er32ResultClass::SnapshotRequired,
                true,
                true,
                'd',
            ),
            observation(
                Er32AdapterKind::Memory,
                Er32Operation::Unknown,
                Er32ResultClass::Unknown,
                false,
                false,
                'e',
            ),
            observation(
                Er32AdapterKind::Jsonl,
                Er32Operation::Unknown,
                Er32ResultClass::Unknown,
                true,
                true,
                'e',
            ),
        ],
    )
}

#[test]
fn memory_and_jsonl_share_logical_outcomes_but_only_jsonl_can_claim_sync_durable() {
    let report = report();
    report.validate().expect("conformance report");
    assert_eq!(
        report.observations[0].output_digest,
        report.observations[1].output_digest
    );
    assert!(!report.observations[0].durable_claim);
    assert!(report.observations[1].durable_claim);
}

#[test]
fn adapter_conformance_rejects_memory_durability_and_wrong_fault_results() {
    let mut memory = observation(
        Er32AdapterKind::Memory,
        Er32Operation::Commit,
        Er32ResultClass::Committed,
        true,
        true,
        'f',
    );
    memory.digest = memory.canonical_digest();
    assert_eq!(
        memory.validate().unwrap_err(),
        "er32_memory_cannot_claim_durable"
    );

    let wrong_replay = observation(
        Er32AdapterKind::Jsonl,
        Er32Operation::Replay,
        Er32ResultClass::Committed,
        true,
        true,
        'g',
    );
    assert_eq!(
        wrong_replay.validate().unwrap_err(),
        "er32_replay_result_invalid"
    );

    let mut secret = observation(
        Er32AdapterKind::Memory,
        Er32Operation::Redaction,
        Er32ResultClass::Committed,
        false,
        false,
        'h',
    );
    secret.secret_free = false;
    secret.digest = secret.canonical_digest();
    assert_eq!(
        secret.validate().unwrap_err(),
        "er32_observation_safety_invariant_failed"
    );
}

#[test]
fn report_rejects_duplicate_adapter_operation_observation() {
    let mut report = report();
    report.observations.push(report.observations[0].clone());
    report.digest = report.canonical_digest();
    assert_eq!(report.validate().unwrap_err(), "er32_observation_duplicate");
}
