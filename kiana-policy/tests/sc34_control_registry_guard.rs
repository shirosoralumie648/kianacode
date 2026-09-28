//! SC-34 source guard: the control registry must stay a read-only contract.
//!
//! The card's whole value is that it can *refuse* a certification claim. That refusal is only
//! meaningful if the module has no way to make one true by itself — if it could write an audit
//! record, run a scanner, or reach a real boundary, then "source level" would stop being the
//! ceiling of what it can honestly claim. So this guard pins the absence of side-effect tokens
//! in addition to the presence of the contract markers.

const SOURCE: &str = include_str!("../src/security_control_registry.rs");

#[test]
fn sc34_registry_keeps_its_contract_markers() {
    for marker in [
        "SecurityControlRegistry",
        "SecurityControl",
        "FrameworkClause",
        "SecurityFramework",
        "ControlScope",
        "ControlEvidence",
        "ControlStatus",
        "ProofLevel",
        "control_digest",
        "registry_digest",
        "security_control_owner_required",
        "security_control_proof_ceiling_exceeds_evidence",
        "security_control_scope_widened",
        "security_control_scope_intersection_empty",
        "security_control_status_not_derivable",
        "security_control_framework_clause_unknown",
        "security_control_framework_major_unknown",
        "security_control_evidence_required",
        "deny_unknown_fields",
        "is_subset_of",
        "intersect",
        "demonstrated_proof",
        "honest_status",
    ] {
        assert!(SOURCE.contains(marker), "missing SC-34 marker: {marker}");
    }
}

#[test]
fn sc34_registry_has_no_side_effect_tokens() {
    // A control registry that could write, dispatch, append or reach a network would be able to
    // manufacture the very evidence it is supposed to be auditing. Keep the list explicit rather
    // than pattern-based so a future addition has to be argued for in this file.
    for forbidden in [
        "std::fs",
        "std::net",
        "std::process",
        "std::thread",
        "Command",
        "TcpStream",
        "reqwest",
        "tokio::",
        "use tokio",
        "EventStore",
        "append_event",
        "ControlPlane",
        "handle_command",
        "write_all",
        "File::",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "SC-34 registry must not reference {forbidden}"
        );
    }
}

#[test]
fn sc34_registry_does_not_declare_a_new_execution_loop() {
    // The single-execution-spine rule from AGENTS.md: a policy-crate module that grew its own
    // command surface or its own model/tool dispatch would be a second spine. None of these may
    // appear as an import or a call target.
    for forbidden in [
        "KianaHarness",
        "CapabilityBroker",
        "RunRequest",
        "shell",
        "apply_patch",
        "memory.search",
        "memory.write",
    ] {
        assert!(
            !SOURCE.contains(forbidden),
            "SC-34 registry must not reference {forbidden}"
        );
    }
}

#[test]
fn sc34_registry_is_registered_in_the_crate_root() {
    let lib = include_str!("../src/lib.rs");
    assert!(lib.contains("mod security_control_registry;"));
    assert!(lib.contains("pub use security_control_registry::*;"));
}
