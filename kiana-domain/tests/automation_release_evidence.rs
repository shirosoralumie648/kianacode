use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn row(
    step: &str,
    status: Aut24FeatureStatus,
    proof: Aut24ProofLevel,
    byte: char,
) -> Aut24EvidenceRow {
    let mut value = Aut24EvidenceRow {
        step: step.to_owned(),
        feature_status: status,
        proof_level: proof,
        evidence_ref: format!("evidence:{step}"),
        next_gate: "GitHub CI focused gate".to_owned(),
        limitations: vec!["runtime/live evidence remains scoped".to_owned()],
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    let _ = byte;
    value
}

fn gate() -> Aut24ReleaseGate {
    let mut gate = Aut24ReleaseGate {
        schema: AUT24_RELEASE_GATE_SCHEMA.to_owned(),
        snapshot_digest: hash('a'),
        workflow_ref: ".github/workflows/aut22-automation-snapshot.yml".to_owned(),
        rows: vec![
            row(
                "AUT-22",
                Aut24FeatureStatus::Partial,
                Aut24ProofLevel::Source,
                'b',
            ),
            row(
                "AUT-23",
                Aut24FeatureStatus::Partial,
                Aut24ProofLevel::Source,
                'c',
            ),
        ],
        blanket_completion: false,
        digest: String::new(),
    };
    gate.digest = gate.canonical_digest();
    gate
}

#[test]
fn release_gate_keeps_source_rows_and_next_gates_explicit() {
    let value = gate();
    value.validate().expect("release gate");
    assert!(value
        .rows
        .iter()
        .all(|row| row.proof_level == Aut24ProofLevel::Source));
}

#[test]
fn blanket_completion_live_without_evidence_and_duplicate_steps_are_rejected() {
    let mut blanket = gate();
    blanket.blanket_completion = true;
    blanket.digest = blanket.canonical_digest();
    assert_eq!(
        blanket.validate().unwrap_err(),
        "aut24_gate_header_or_blanket_completion_invalid"
    );

    let mut live = gate();
    live.rows[0].feature_status = Aut24FeatureStatus::Partial;
    live.rows[0].proof_level = Aut24ProofLevel::Live;
    live.rows[0].digest = live.rows[0].canonical_digest();
    live.digest = live.canonical_digest();
    assert_eq!(
        live.validate().unwrap_err(),
        "aut24_live_proof_requires_implemented_status"
    );

    let mut duplicate = gate();
    duplicate.rows.push(duplicate.rows[0].clone());
    duplicate.digest = duplicate.canonical_digest();
    assert_eq!(duplicate.validate().unwrap_err(), "aut24_step_duplicate");
}
