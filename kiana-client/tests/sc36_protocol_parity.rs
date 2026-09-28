//! SC-36: five-surface protocol parity — CLI, Workbench, Web, Desktop against the daemon.
//!
//! Deny-first. The four failures this card names are the ones a trace comparison cannot see on its
//! own: a surface that renders the right answer for the wrong reason still looks correct. So each
//! test below builds an otherwise-agreeing set of five observations and breaks exactly one thing.

use kiana_client::{
    compare_protocol_parity, ApprovalPresentation, DecisionSource, ProtocolObservation,
    ProtocolParityError, ProtocolSurface, PROTOCOL_PARITY_SCHEMA,
};

const RECEIPT: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const REDACTION: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";
const APPROVAL_REF: &str = "approval-7f3a";

fn observation(surface: ProtocolSurface) -> ProtocolObservation {
    ProtocolObservation {
        schema: PROTOCOL_PARITY_SCHEMA.to_owned(),
        surface,
        command_id: "cmd-1".to_owned(),
        operation: "run.turn.v2".to_owned(),
        disposition: "applied".to_owned(),
        retry: "do_not_retry".to_owned(),
        cursor_epoch: "epoch-1".to_owned(),
        cursor_sequence: 12,
        revision: 3,
        receipt_digest: Some(RECEIPT.to_owned()),
        error_code: None,
        decision_source: DecisionSource::ServerRendered,
        approval: ApprovalPresentation::NotApplicable,
        approval_reference: None,
        redaction_profile_digest: REDACTION.to_owned(),
        sensitive_field_count: 0,
        limitations: vec!["no transport was opened".to_owned()],
    }
}

/// Five surfaces that all agree. Every deny test below starts from this and breaks one thing, so a
/// passing test is a statement about the specific rule rather than about the fixture.
fn agreeing() -> Vec<ProtocolObservation> {
    ProtocolSurface::ALL
        .iter()
        .map(|surface| observation(*surface))
        .collect()
}

#[test]
fn a_surface_that_computed_the_decision_for_itself_is_refused() {
    // The card's "本地计算权限". This surface agrees on every field and is still wrong: it reached
    // the right answer without asking. A UI that holds a policy engine is a second policy engine.
    let mut observations = agreeing();
    observations[1].decision_source = DecisionSource::LocallyComputed;
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::LocallyComputedDecision
    );
}

#[test]
fn a_surface_that_applied_an_approval_without_a_server_reference_is_refused() {
    // The card's "局部 approve". The approval exists only here, so no other surface, the receipt
    // and the audit trail will ever know it happened.
    let mut observations = agreeing();
    observations[2].approval = ApprovalPresentation::AppliedLocally;
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::ApprovalAppliedLocally
    );
}

#[test]
fn an_approval_shown_as_decided_without_naming_the_reference_is_refused() {
    let mut observations = agreeing();
    for entry in &mut observations {
        entry.approval = ApprovalPresentation::RenderedDecided;
    }
    observations[0].approval_reference = Some(APPROVAL_REF.to_owned());
    // The other four show the button as decided and name nothing.
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::ApprovalReferenceMismatch
    );
}

#[test]
fn two_surfaces_showing_different_approvals_are_refused() {
    // Agreeing that "it was approved" is not agreement. Without the reference, two different
    // approvals look identical from the outside.
    let mut observations = agreeing();
    for entry in &mut observations {
        entry.approval = ApprovalPresentation::RenderedDecided;
        entry.approval_reference = Some(APPROVAL_REF.to_owned());
    }
    observations[3].approval_reference = Some("approval-0000".to_owned());
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::ApprovalReferenceMismatch
    );
}

#[test]
fn a_surface_that_redacts_differently_is_refused() {
    // The card's "不同 redaction". Redacting less is not "more useful", it is a leak, and it is
    // the kind of difference no disposition comparison would ever notice.
    let mut observations = agreeing();
    observations[4].redaction_profile_digest =
        "sha256:3333333333333333333333333333333333333333333333333333333333333333".to_owned();
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::RedactionProfileMismatch
    );
}

#[test]
fn surfaces_showing_different_states_are_refused_one_field_at_a_time() {
    // The card's "入口显示不同状态". Each field is broken separately so the error names which one
    // drifted instead of reporting a generic disagreement.
    for (mutate, expected) in [
        (
            Box::new(|o: &mut ProtocolObservation| o.disposition = "rejected".to_owned())
                as Box<dyn Fn(&mut ProtocolObservation)>,
            "disposition",
        ),
        (
            Box::new(|o: &mut ProtocolObservation| o.retry = "safe_retry".to_owned()),
            "retry",
        ),
        (
            Box::new(|o: &mut ProtocolObservation| o.cursor_sequence = 13),
            "cursor",
        ),
        (
            Box::new(|o: &mut ProtocolObservation| o.revision = 4),
            "revision",
        ),
        (
            Box::new(|o: &mut ProtocolObservation| o.receipt_digest = None),
            "receipt",
        ),
        (
            Box::new(|o: &mut ProtocolObservation| o.error_code = Some("denied".to_owned())),
            "error_code",
        ),
    ] {
        let mut observations = agreeing();
        mutate(&mut observations[1]);
        assert_eq!(
            compare_protocol_parity(&observations).unwrap_err(),
            ProtocolParityError::Mismatch(expected),
            "field {expected} should be named"
        );
    }
}

#[test]
fn an_unknown_outcome_rendered_as_settled_is_refused() {
    // Unknown is a first-class fact. A surface that shows it as finished has invented an outcome,
    // and the user will act on the invention.
    let mut observations = agreeing();
    for entry in &mut observations {
        entry.disposition = "unknown".to_owned();
        entry.receipt_digest = None;
    }
    observations[0].approval = ApprovalPresentation::RenderedDecided;
    observations[0].approval_reference = Some(APPROVAL_REF.to_owned());
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::UnknownRenderedAsSettled
    );
}

#[test]
fn a_surface_that_still_shows_a_sensitive_field_is_refused() {
    let mut observations = agreeing();
    observations[2].sensitive_field_count = 1;
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::SensitiveFieldVisible
    );
}

#[test]
fn an_applied_outcome_without_a_receipt_is_refused() {
    let mut observations = agreeing();
    observations[3].receipt_digest = None;
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::ReceiptMissing
    );
}

#[test]
fn a_missing_or_duplicated_surface_is_refused() {
    // Four surfaces is a coverage gap: the whole point is that the fifth one agrees.
    let mut observations = agreeing();
    observations.pop();
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::SurfacesMissing
    );
    // Six is a fabrication.
    let mut duplicated = agreeing();
    duplicated.push(observation(ProtocolSurface::Web));
    assert_eq!(
        compare_protocol_parity(&duplicated).unwrap_err(),
        ProtocolParityError::DuplicateSurface
    );
}

#[test]
fn a_comparison_with_no_limitations_is_refused() {
    // A parity check that claims it saw everything has not seen anything.
    let mut observations = agreeing();
    for entry in &mut observations {
        entry.limitations.clear();
    }
    assert_eq!(
        compare_protocol_parity(&observations).unwrap_err(),
        ProtocolParityError::LimitationsRequired
    );
}

#[test]
fn five_agreeing_surfaces_produce_a_report_naming_the_daemon_as_the_reference() {
    let report = compare_protocol_parity(&agreeing()).expect("five agreeing surfaces");
    assert_eq!(report.reference_surface, ProtocolSurface::Daemon);
    assert_eq!(report.surfaces.len(), 5);
    assert_eq!(report.disposition, "applied");
    assert_eq!(report.receipt_digest.as_deref(), Some(RECEIPT));
    // The limitations are unioned rather than taken from one surface, so a limitation only one
    // surface knows about still reaches the report.
    assert_eq!(report.limitations, vec!["no transport was opened".to_owned()]);
}
