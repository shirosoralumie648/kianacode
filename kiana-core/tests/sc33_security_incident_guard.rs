//! SC-33 source guard: an incident is a named, owned, time-boxed, evidence-citing record whose four
//! response steps are ordered, single-use and irreversible.
//!
//! These are source-text assertions, not behavioural ones. They pin the properties the card names
//! to the file that implements them so a later edit cannot quietly drop one: every marker below is
//! grepped for literally, and every forbidden string is checked to be genuinely absent. The absence
//! check is the load-bearing half. The module claims to be a read-only contract over supplied
//! facts, and the only thing that keeps that claim honest is that nothing in the file can reach the
//! filesystem, a process, a socket, a clock or the event log.

#[test]
fn sc33_module_pins_the_incident_record_and_its_admission_rules() {
    let source = include_str!("../src/security_incident.rs");

    // The record itself. `SecurityIncident` is the thing a security condition becomes once a person
    // is accountable for it, so every one of these types has to stay named in the file.
    for marker in [
        "SecurityIncident",
        "SecurityIncidentClass",
        "SecurityIncidentSeverity",
        "SecurityIncidentEvidence",
        "SecurityVulnerability",
        "SecurityIncidentState",
        "SecurityIncidentActionRequest",
        "SecurityIncidentClosure",
        "SecurityIncidentReport",
        "SecurityIncidentReportStatus",
        "SECURITY_INCIDENT_SCHEMA",
        "SECURITY_INCIDENT_STATE_SCHEMA",
        "SECURITY_INCIDENT_ACTION_SCHEMA",
        "SECURITY_INCIDENT_REPORT_SCHEMA",
        "SECURITY_VULNERABILITY_SCHEMA",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }

    // The three conditions the workflow exists for. A fourth catch-all bucket would be a way to
    // open an incident with no severity ladder, no deadline and no reviewer.
    for marker in [
        "UnknownOutcome",
        "SecretLeak",
        "SupplyChainDrift",
    ] {
        assert!(source.contains(marker), "SC-33 module lost class {marker}");
    }

    // Admission. "Nobody owns this", "this owner string is malformed" and "this deadline already
    // passed" have to stay three different refusals, or an operator cannot tell a missing owner
    // from a corrupt one.
    for marker in [
        "security_incident_owner_required",
        "security_incident_deadline_required",
        "security_incident_deadline_expired",
        "security_incident_evidence_required",
        "security_incident_evidence_exhausted",
        "security_incident_vulnerability_required",
        "security_incident_severity_below_vulnerability_floor",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }
}

#[test]
fn sc33_module_pins_the_four_step_order_and_its_terminal_state() {
    let source = include_str!("../src/security_incident.rs");

    // Exactly four actions, and the transitions between them. `Skip`, `Force` and `Waive` are the
    // escape hatches that would let an unknown become a success, so their absence is the rule.
    for marker in [
        "SecurityIncidentPhase",
        "SecurityIncidentAction",
        "fn next(",
        "fn required_state(",
        "fn resulting_state(",
        "Open",
        "Contained",
        "Fenced",
        "Reconciled",
        "Closed",
        "security_incident_action_out_of_order",
        "security_incident_reconcile_required",
        "security_incident_state_irreversible",
        "security_incident_state_order_invalid",
        "security_incident_deadline_exceeded",
        "security_incident_action_actor_required",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }

    // The three ordering rules are decided before the generic text guards, which is why the codes
    // above are literal and not `format!`-derived. If a future edit reorders `derive`, the codes
    // stop being reachable and these literals stop being the ones a caller sees.
    for marker in [
        "security_incident_state_irreversible",
        "security_incident_state_binding_invalid",
        "security_incident_deadline_exceeded",
    ] {
        assert!(
            source.contains(marker),
            "SC-33 decision order marker {marker} disappeared"
        );
    }
}

#[test]
fn sc33_module_pins_evidence_single_use_and_separated_closure() {
    let source = include_str!("../src/security_incident.rs");

    // One timeout must not be able to justify a contain, a reconcile and a close.
    for marker in [
        "consumed_evidence",
        "security_incident_evidence_replay",
        "security_incident_evidence_not_in_incident",
        "security_incident_evidence_duplicate",
        "security_incident_state_evidence_unknown",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }

    // Evidence is a reference plus digests, never a payload. That is what stops a SecretLeak record
    // from becoming the second copy of the secret.
    for marker in [
        "scan_secret_sentinels",
        "redact_text",
        "security_vulnerability_advisory_id",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }

    // Closure is a claim somebody signs for, and it is not the owner's signature.
    for marker in [
        "SecurityIncidentClosure",
        "root_cause",
        "impact_scope",
        "containment_actions",
        "residual_risk",
        "reviewer",
        "security_incident_closure_required",
        "security_incident_closure_not_allowed",
        "security_incident_closure_reviewer_conflict",
        "security_incident_closure_containment_required",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }
}

#[test]
fn sc33_report_is_re_derived_rather_than_trusted() {
    let source = include_str!("../src/security_incident.rs");

    // A report that can edit its own status is not a decision. `validate_against` re-runs the same
    // derivation over the same three inputs and refuses any mismatch.
    for marker in [
        "fn validate_against(",
        "fn derive(",
        "security_incident_report_binding_invalid",
        "security_incident_report_reason_state_mismatch",
        "security_incident_report_remediation_with_outcome",
        "security_incident_report_state_not_advanced",
        "security_incident_report_digest_mismatch",
        "security_incident_digest_mismatch",
        "security_incident_action_digest_mismatch",
        "security_incident_closure_digest_mismatch",
        "security_incident_state_digest_mismatch",
        "deny_unknown_fields",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }
}

#[test]
fn sc33_module_stays_a_read_only_contract_with_no_side_effect() {
    let source = include_str!("../src/security_incident.rs");

    // The load-bearing absence check. If any of these ever appears, the module has stopped being a
    // decision over supplied facts and has become a second execution or authority path, which
    // `ControlPlane::handle_command` is supposed to own exclusively.
    for forbidden in [
        "std::fs",
        "File::",
        "Command::",
        "std::process",
        "TcpStream",
        "reqwest",
        "tokio",
        "spawn",
        "thread::sleep",
        "SystemTime",
        "Instant::now",
        "EventStore",
        "append_event",
    ] {
        assert!(
            !source.contains(forbidden),
            "SC-33 module gained a side-effect token: {forbidden}"
        );
    }

    // The bounds are policy, not taste: they cap how much text can be sealed into a digest and how
    // much a single incident can accumulate. They are named constants so a fixture can assert them
    // rather than hard-coding a number.
    for marker in [
        "MAX_SECURITY_INCIDENT_TEXT",
        "MAX_SECURITY_INCIDENT_EVIDENCE",
        "MAX_SECURITY_INCIDENT_ACTIONS",
    ] {
        assert!(source.contains(marker), "SC-33 module lost {marker}");
    }
}
