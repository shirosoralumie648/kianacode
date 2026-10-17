use kiana_daemon::eval_runtime::{EvalFaultDisposition, EvalFaultKind, EvalFaultPlan};
use kiana_domain::EvalVerdict;

#[test]
fn fault_plan_preserves_unknown_and_stop_evidence() {
    let mut cancel = EvalFaultPlan::new(EvalFaultKind::CancelRace);
    cancel.effect_started = true;
    cancel.stop_confirmed = false;
    cancel.plan_digest = cancel.digest();
    let evidence = cancel.apply().unwrap();
    assert_eq!(evidence.disposition, EvalFaultDisposition::UnknownReconcile);
    assert!(!evidence.effect_known);
    assert!(evidence.effect_started);
    assert!(!evidence.stop_confirmed);
    assert!(evidence.requires_reconciliation);
    assert_eq!(evidence.terminal_code, "cancel_race_unknown");
    // An unconfirmed started effect can never be reported as a passing evaluation.
    evidence.validate().unwrap();
    assert!(evidence.blocks_eval_pass());
    assert_eq!(
        evidence.apply_fault_invariant(EvalVerdict::Pass),
        EvalVerdict::Blocked
    );
    // Non-passing verdicts are preserved unchanged; only `Pass` is downgraded.
    assert_eq!(
        evidence.apply_fault_invariant(EvalVerdict::Fail),
        EvalVerdict::Fail
    );

    let mut stopped = EvalFaultPlan::new(EvalFaultKind::CancelRace);
    stopped.stop_confirmed = true;
    let evidence = stopped.apply().unwrap();
    assert_eq!(
        evidence.disposition,
        EvalFaultDisposition::CancelledNotStarted
    );
    assert!(evidence.effect_known);
    assert!(!evidence.requires_reconciliation);
    // A cancel that is confirmed before any effect is a known outcome and clears the gate.
    assert!(!evidence.blocks_eval_pass());
    assert_eq!(
        evidence.apply_fault_invariant(EvalVerdict::Pass),
        EvalVerdict::Pass
    );
}

#[test]
fn fault_plan_covers_approval_lease_restart_and_unknown_paths() {
    for (kind, terminal) in [
        (EvalFaultKind::ApprovalDenied, "approval_denied"),
        (EvalFaultKind::ApprovalExpired, "approval_expired"),
        (EvalFaultKind::StaleLease, "stale_lease"),
    ] {
        let mut plan = EvalFaultPlan::new(kind);
        if kind == EvalFaultKind::StaleLease {
            plan.observed_lease_epoch = 2;
            plan.plan_digest = plan.digest();
        }
        let evidence = plan.apply().unwrap();
        assert_eq!(evidence.disposition, EvalFaultDisposition::Denied);
        assert!(evidence.effect_known);
        assert_eq!(evidence.terminal_code, terminal);
    }

    let mut crash = EvalFaultPlan::new(EvalFaultKind::CrashAfterEffect);
    crash.effect_started = true;
    crash.stop_confirmed = false;
    crash.plan_digest = crash.digest();
    assert_eq!(
        crash.apply().unwrap().disposition,
        EvalFaultDisposition::UnknownReconcile
    );
    assert_eq!(
        EvalFaultPlan::new(EvalFaultKind::Restart)
            .apply()
            .unwrap()
            .terminal_code,
        "restart_reconcile_required"
    );
    assert_eq!(
        EvalFaultPlan::new(EvalFaultKind::ResultUnknown)
            .apply()
            .unwrap()
            .terminal_code,
        "result_unknown"
    );
}

#[test]
fn fault_plan_rejects_tamper_and_unsafe_combinations() {
    let mut tampered = EvalFaultPlan::new(EvalFaultKind::ResultUnknown);
    tampered.plan_digest = "sha256:tampered".to_owned();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "eval_fault_plan_header_invalid"
    );

    let mut approval = EvalFaultPlan::new(EvalFaultKind::ApprovalDenied);
    approval.effect_started = true;
    approval.plan_digest = approval.digest();
    assert_eq!(
        approval.validate().unwrap_err(),
        "eval_fault_approval_effect_started"
    );

    let mut lease = EvalFaultPlan::new(EvalFaultKind::StaleLease);
    lease.plan_digest = lease.digest();
    assert_eq!(lease.validate().unwrap_err(), "eval_fault_lease_not_stale");
}

#[test]
fn fault_evidence_rejects_digest_or_disposition_tamper() {
    let evidence = EvalFaultPlan::new(EvalFaultKind::ResultUnknown)
        .apply()
        .unwrap();
    evidence.validate().unwrap();
    assert!(evidence.requires_reconciliation);
    assert!(evidence.blocks_eval_pass());

    // A rewritten evidence digest is rejected before it can be trusted.
    let mut tampered_digest = evidence.clone();
    tampered_digest.evidence_digest = "sha256:tampered".to_owned();
    assert_eq!(
        tampered_digest.validate().unwrap_err(),
        "eval_fault_evidence_invalid"
    );

    // Rewriting the disposition without re-sealing the digest is caught by the same binding.
    let mut tampered_disposition = evidence.clone();
    tampered_disposition.disposition = EvalFaultDisposition::Denied;
    assert_eq!(
        tampered_disposition.validate().unwrap_err(),
        "eval_fault_evidence_invalid"
    );

    // A missing plan digest is not a valid fault evidence record.
    let mut missing_plan = evidence.clone();
    missing_plan.plan_digest = String::new();
    assert_eq!(
        missing_plan.validate().unwrap_err(),
        "eval_fault_evidence_invalid"
    );
}
