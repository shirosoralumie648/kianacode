use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn surfaces(
    flow_id: &str,
    authority: &str,
    cursor: u64,
    status: Cp30FlowStatus,
    receipt: Option<String>,
) -> Vec<Cp30SurfaceReceipt> {
    [
        Cp30Surface::Cli,
        Cp30Surface::Workbench,
        Cp30Surface::Web,
        Cp30Surface::Desktop,
    ]
    .into_iter()
    .map(|surface| {
        Cp30SurfaceReceipt::new(surface, flow_id, authority, cursor, status, receipt.clone())
    })
    .collect()
}

fn read_only() -> Cp30ProductFlowEvidence {
    Cp30ProductFlowEvidence::new(
        "flow-read-only",
        Cp30FlowKind::ReadOnly,
        Cp30FlowStatus::Completed,
        hash('a'),
        11,
        1,
        0,
        0,
        false,
        false,
        true,
        surfaces(
            "flow-read-only",
            &hash('a'),
            11,
            Cp30FlowStatus::Completed,
            None,
        ),
        None,
        vec!["fake model cassette".to_owned()],
    )
}

fn approval_write() -> Cp30ProductFlowEvidence {
    Cp30ProductFlowEvidence::new(
        "flow-approval-write",
        Cp30FlowKind::ApprovalWrite,
        Cp30FlowStatus::Completed,
        hash('b'),
        22,
        2,
        1,
        1,
        true,
        false,
        true,
        surfaces(
            "flow-approval-write",
            &hash('b'),
            22,
            Cp30FlowStatus::Completed,
            Some(hash('c')),
        ),
        Some(hash('c')),
        vec!["fake broker effect fixture".to_owned()],
    )
}

fn cancel_recovery() -> Cp30ProductFlowEvidence {
    Cp30ProductFlowEvidence::new(
        "flow-cancel-recovery",
        Cp30FlowKind::CancelRecovery,
        Cp30FlowStatus::Reconciled,
        hash('d'),
        33,
        1,
        0,
        0,
        false,
        true,
        true,
        surfaces(
            "flow-cancel-recovery",
            &hash('d'),
            33,
            Cp30FlowStatus::Reconciled,
            Some(hash('e')),
        ),
        Some(hash('e')),
        vec!["cancel/reconcile remains CI-only".to_owned()],
    )
}

#[test]
fn product_bundle_covers_read_only_approval_write_and_cancel_recovery_on_four_surfaces() {
    let bundle = Cp30ProductFlowBundle::new(vec![read_only(), approval_write(), cancel_recovery()]);
    bundle.validate().expect("product flow bundle");
}

#[test]
fn approval_write_cannot_complete_without_consumed_approval_or_receipt() {
    let mut pending = approval_write();
    pending.status = Cp30FlowStatus::Completed;
    pending.approval_consumed = false;
    pending.digest = pending.canonical_digest();
    assert_eq!(
        pending.validate().unwrap_err(),
        "cp30_approval_write_requires_consumed_receipt"
    );

    let mut stale_surface = approval_write();
    stale_surface.surfaces[2].authority_digest = hash('f');
    stale_surface.surfaces[2].digest = stale_surface.surfaces[2].canonical_digest();
    stale_surface.digest = stale_surface.canonical_digest();
    assert_eq!(
        stale_surface.validate().unwrap_err(),
        "cp30_surface_parity_mismatch"
    );
}

#[test]
fn read_only_and_cancel_recovery_never_claim_unbounded_write_or_blind_restart() {
    let mut read = read_only();
    read.effect_count = 1;
    read.digest = read.canonical_digest();
    assert_eq!(
        read.validate().unwrap_err(),
        "cp30_read_only_flow_effect_or_approval_invalid"
    );

    let mut recovery = cancel_recovery();
    recovery.restored_from_facts = false;
    recovery.digest = recovery.canonical_digest();
    assert_eq!(
        recovery.validate().unwrap_err(),
        "cp30_cancel_recovery_evidence_incomplete"
    );

    let mut incomplete =
        Cp30ProductFlowBundle::new(vec![read_only(), approval_write(), cancel_recovery()]);
    incomplete.flows.pop();
    incomplete.digest = incomplete.canonical_digest();
    assert_eq!(
        incomplete.validate().unwrap_err(),
        "cp30_product_bundle_header_invalid"
    );
}
