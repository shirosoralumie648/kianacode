//! SC-36 source guard: the comparator is a read-only comparison, and the three authority rules —
//! locally computed decision, locally applied approval, per-surface redaction — are what make it
//! more than a diff.
//!
//! The absence check is load-bearing: a parity comparator that could open a transport or execute
//! something would be a second execution path wearing a comparison's clothes.

#[test]
fn sc36_pins_the_five_surfaces_and_the_daemon_as_reference() {
    let source = include_str!("../src/protocol_parity.rs");

    for marker in [
        "ProtocolSurface",
        "Cli",
        "Workbench",
        "Web",
        "Desktop",
        "Daemon",
        "is_daemon",
        "reference_surface",
        "PROTOCOL_PARITY_SCHEMA",
        "PROTOCOL_PARITY_REPORT_SCHEMA",
        "compare_protocol_parity",
        "ProtocolParityReport",
    ] {
        assert!(source.contains(marker), "SC-36 module lost {marker}");
    }
}

#[test]
fn sc36_pins_the_three_authority_rules() {
    let source = include_str!("../src/protocol_parity.rs");

    // "Renders the right answer for the wrong reason" is the failure this card exists for, and it
    // is invisible to a pure agreement check. Each rule needs both its type and its refusal.
    for marker in [
        "DecisionSource",
        "ServerRendered",
        "LocallyComputed",
        "ApprovalPresentation",
        "AppliedLocally",
        "RenderedDecided",
        "RenderedPending",
        "redaction_profile_digest",
        "RedactionProfileMismatch",
        "LocallyComputedDecision",
        "ApprovalAppliedLocally",
        "ApprovalReferenceMissing",
        "ApprovalReferenceMismatch",
        "UnknownRenderedAsSettled",
    ] {
        assert!(source.contains(marker), "SC-36 module lost {marker}");
    }
}

#[test]
fn sc36_pins_the_agreement_fields_ui31_already_established() {
    let source = include_str!("../src/protocol_parity.rs");

    // Same vocabulary as UI-31 on purpose, so one fixture can be checked by both.
    for marker in [
        "\"accepted\" | \"applied\" | \"rejected\" | \"unknown\"",
        "query_original",
        "safe_retry",
        "do_not_retry",
        "cursor_epoch",
        "cursor_sequence",
        "revision",
        "receipt_digest",
        "error_code",
        "ReceiptMissing",
        "SensitiveFieldVisible",
        "LimitationsRequired",
    ] {
        assert!(source.contains(marker), "SC-36 module lost {marker}");
    }
}

#[test]
fn sc36_stays_a_read_only_comparator() {
    let source = include_str!("../src/protocol_parity.rs");

    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "EventStore",
        "append_event",
        "ControlPlane",
    ] {
        assert!(
            !source.contains(forbidden),
            "SC-36 comparator gained a token it must not have: {forbidden}"
        );
    }
}
