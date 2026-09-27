//! SC-37/SC-38/SC-39 source guards.
//!
//! These assert on exact source text. Every marker below was grepped in the source it points at
//! before this file was written; every forbidden string was confirmed absent.

#[test]
fn sc37_denies_cover_every_family_with_a_zero_effect_dispatch_budget() {
    let source = include_str!("../../kiana-domain/src/security_fixture_contracts.rs");
    for marker in [
        "SC37_DENY_MATRIX_SCHEMA",
        "SC37_DENY_CASE_SCHEMA",
        "Sc37DenyFamily",
        "Sc37DispatchSurface",
        "sc37_deny_case_effect_not_zero",
        "sc37_deny_case_dispatch_surface_set_invalid",
        "sc37_deny_family_uncovered",
        "Sc37DenyCase::zero_dispatch_counts",
    ] {
        assert!(source.contains(marker), "SC-37 marker missing: {marker}");
    }
    for family in [
        "PrivilegeEscalation",
        "Replay",
        "Leakage",
        "Toctou",
        "Deletion",
        "EntrypointBypass",
    ] {
        assert!(source.contains(family), "SC-37 family missing: {family}");
    }
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("CapabilityBrokerPort"));
}

#[test]
fn sc38_property_corpus_is_seeded_deterministic_and_has_no_allow_outcome() {
    let source = include_str!("../../kiana-domain/src/security_fixture_contracts.rs");
    for marker in [
        "SC38_PROPERTY_CORPUS_SCHEMA",
        "SC38_PROPERTY_CASE_SCHEMA",
        "Sc38InputShape",
        "Sc38Invariant",
        "sc38_property_case_replay_diverged",
        "sc38_property_shape_uncovered",
        "sc38_property_case_seed_mismatch",
        "replay_reversed",
    ] {
        assert!(source.contains(marker), "SC-38 marker missing: {marker}");
    }
    for shape in [
        "RandomPayload",
        "TruncatedFrame",
        "DuplicateFrame",
        "OutOfOrderCursor",
        "UnknownMajor",
    ] {
        assert!(source.contains(shape), "SC-38 input shape missing: {shape}");
    }
    // The corpus has no wall-clock or unseeded randomness.
    assert!(!source.contains("SystemTime"));
    assert!(!source.contains("UNIX_EPOCH"));
    assert!(!source.contains("rand::"));
    // A deny-first corpus has no Allow outcome: a fuzzed input is always a refusal.
    assert!(!source.contains("SC38_ALLOW"));
    assert!(source.contains("The only legal outcome for a fuzzed input"));
}

#[test]
fn sc39_attack_corpus_keeps_only_digests_and_never_promotes_injection_to_policy() {
    let source = include_str!("../../kiana-domain/src/security_fixture_contracts.rs");
    for marker in [
        "SC39_ATTACK_CORPUS_SCHEMA",
        "SC39_ATTACK_CASE_SCHEMA",
        "Sc39AttackShape",
        "Sc39ExpectedOutcome",
        "PromptTrustClass",
        "sc39_attack_case_effect_not_zero",
        "sc39_attack_case_prompt_authority_invalid",
        "attack_text_digest",
    ] {
        assert!(source.contains(marker), "SC-39 marker missing: {marker}");
    }
    for shape in [
        "PromptInjection",
        "IndirectInjection",
        "SecretExfiltration",
        "MaliciousPlugin",
        "MaliciousMcpDescription",
    ] {
        assert!(
            source.contains(shape),
            "SC-39 attack shape missing: {shape}"
        );
    }
    // The corpus stores a digest, never the attack text or a live effect path.
    assert!(!source.contains("std::fs"));
    assert!(!source.contains("reqwest"));
    assert!(!source.contains("CapabilityBrokerPort"));
}

#[test]
fn sc37_sc38_sc39_corpora_are_wired_into_the_domain_crate() {
    // The corpora are only useful if they are reachable; a guard that greps the module file but
    // not its registration would pass while the whole slice is dead code.
    let domain_lib = include_str!("../../kiana-domain/src/lib.rs");
    assert!(
        domain_lib.contains("mod security_fixture_contracts;"),
        "security_fixture_contracts module is not registered in kiana-domain/src/lib.rs"
    );
    assert!(
        domain_lib.contains("pub use security_fixture_contracts::*;"),
        "security_fixture_contracts is not re-exported from kiana-domain"
    );
}

#[test]
fn sc37_sc38_sc39_fixtures_name_every_rejection_in_their_card() {
    let domain_fixture = include_str!("../../kiana-domain/tests/sc37_sc39_security_fixtures.rs");
    for marker in [
        "sc37_privilege_escalation_rejected_with_zero_dispatch",
        "sc37_replay_rejected_with_zero_dispatch",
        "sc37_leakage_rejected_with_zero_dispatch",
        "sc37_toctou_rejected_with_zero_dispatch",
        "sc37_deletion_rejected_with_zero_dispatch",
        "sc37_entrypoint_bypass_rejected_with_zero_dispatch",
        "sc38_random_payload_rejected_and_replay_is_deterministic",
        "sc38_truncated_frame_rejected",
        "sc38_duplicate_frame_is_deduplicated_not_re_effect",
        "sc38_out_of_order_cursor_rejected",
        "sc38_unknown_major_is_rejected_not_upgraded",
        "sc39_prompt_injection_cannot_grant_authority",
        "sc39_indirect_injection_cannot_trigger_effect",
        "sc39_secret_exfiltration_cannot_leave_the_broker",
        "sc39_malicious_plugin_cannot_widen_capabilities",
        "sc39_malicious_mcp_description_requires_approval_or_unknown",
    ] {
        assert!(
            domain_fixture.contains(marker),
            "SC fixture test missing: {marker}"
        );
    }
    let core_fixture = include_str!("../tests/sc37_deny_matrix.rs");
    for marker in [
        "sc37_privilege_escalation_forged_role_is_denied_with_zero_dispatch",
        "sc37_replay_unbound_permit_is_denied_with_zero_dispatch",
        "sc37_leakage_secret_sentinel_in_receipt_text_is_denied_with_zero_dispatch",
        "sc37_toctou_fence_scope_drift_is_denied_with_zero_dispatch",
        "sc37_deletion_legal_hold_is_denied_with_zero_dispatch",
        "sc37_entrypoint_bypass_denied_command_with_handler_call_is_rejected",
        "sc37_deny_matrix_covers_all_six_families_with_zero_total_dispatch",
    ] {
        assert!(
            core_fixture.contains(marker),
            "SC-37 core test missing: {marker}"
        );
    }
    let property_fixture = include_str!("../tests/sc38_property_replay.rs");
    for marker in [
        "sc38_random_payloads_never_decode_into_a_dispatchable_value",
        "sc38_truncated_frames_never_partially_accept",
        "sc38_duplicate_frames_deduplicate_instead_of_re_effecting",
        "sc38_out_of_order_and_gapped_cursors_are_refused",
        "sc38_unknown_major_versions_are_refused_not_upgraded",
        "sc38_corpus_is_deterministic_across_rebuilds_and_order_replays",
    ] {
        assert!(
            property_fixture.contains(marker),
            "SC-38 core test missing: {marker}"
        );
    }
    let red_team_fixture = include_str!("../tests/sc39_red_team_corpus.rs");
    for marker in [
        "sc39_prompt_injection_cannot_become_product_authority",
        "sc39_indirect_injection_from_repository_text_cannot_widen_a_grant",
        "sc39_secret_exfiltration_cannot_leave_the_broker",
        "sc39_malicious_plugin_manifest_cannot_self_authorize",
        "sc39_malicious_mcp_description_is_denied_before_dispatch",
        "sc39_attack_corpus_covers_every_shape_with_zero_effect_and_no_real_secret",
    ] {
        assert!(
            red_team_fixture.contains(marker),
            "SC-39 core test missing: {marker}"
        );
    }
}
