//! SC-37/SC-38/SC-39 domain fixture: one named test per rejection in each card's
//! "先拒绝" column.  These are offline value contracts — no capability, provider, plugin, MCP
//! server or filesystem is touched here.

use kiana_domain::{
    json_digest, OperationId, PromptTrustClass, RequestId, RunId, Sc37DenyCase, Sc37DenyFamily,
    Sc37DenyMatrix, Sc37DispatchSurface, Sc38InputShape, Sc38Invariant, Sc38Outcome,
    Sc38PropertyCase, Sc38PropertyCorpus, Sc39AttackCase, Sc39AttackCorpus, Sc39AttackShape,
    Sc39ExpectedOutcome, ScFixtureCorrelation, SecurityReasonCode, SC37_DENY_MATRIX_SCHEMA,
    SC37_DENY_MATRIX_VERSION, SC38_PROPERTY_CORPUS_SCHEMA, SC39_ATTACK_CORPUS_SCHEMA,
};
use std::collections::BTreeSet;

fn digest_of(value: &serde_json::Value) -> String {
    json_digest(value)
}

fn deny_case(name: &str, family: Sc37DenyFamily, reason: SecurityReasonCode) -> Sc37DenyCase {
    Sc37DenyCase::new(name, family, reason, reason.as_str()).expect("SC-37 deny case")
}

// ---------------------------------------------------------------------------
// SC-37: 每类越权、重放、泄露、TOCTOU、删除和入口绕过
// ---------------------------------------------------------------------------

#[test]
fn sc37_privilege_escalation_rejected_with_zero_dispatch() {
    let case = deny_case(
        "sc37_privilege_escalation_forged_role",
        Sc37DenyFamily::PrivilegeEscalation,
        SecurityReasonCode::AuthRoleMismatch,
    );
    case.validate()
        .expect("role mismatch is a valid zero-effect deny");
    assert_eq!(case.family.as_str(), "privilege_escalation");
    assert_eq!(case.observed_reason, "AUTH_ROLE_MISMATCH");
    for surface in Sc37DispatchSurface::ALL {
        assert_eq!(case.dispatch_count(*surface), 0, "{surface:?} dispatched");
    }
    assert!(!case.limitations.is_empty());
}

#[test]
fn sc37_replay_rejected_with_zero_dispatch() {
    let case = deny_case(
        "sc37_replay_consumed_permit",
        Sc37DenyFamily::Replay,
        SecurityReasonCode::PolicyApprovalBindingMismatch,
    );
    case.validate()
        .expect("replayed permit is a valid zero-effect deny");
    assert_eq!(case.observed_reason, "POLICY_APPROVAL_BINDING_MISMATCH");
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::ControlPlaneHandler),
        0
    );
}

#[test]
fn sc37_leakage_rejected_with_zero_dispatch() {
    let case = deny_case(
        "sc37_leakage_secret_in_prompt",
        Sc37DenyFamily::Leakage,
        SecurityReasonCode::SecretRedactionFailed,
    );
    case.validate()
        .expect("secret in prompt is a valid zero-effect deny");
    assert_eq!(case.observed_reason, "SECRET_REDACTION_FAILED");
    assert!(case.secret_free);
}

#[test]
fn sc37_toctou_rejected_with_zero_dispatch() {
    let case = deny_case(
        "sc37_toctou_generation_drift",
        Sc37DenyFamily::Toctou,
        SecurityReasonCode::FilesystemToctou,
    );
    case.validate()
        .expect("TOCTOU drift is a valid zero-effect deny");
    assert_eq!(case.observed_reason, "FS_TOCTOU");
    assert_eq!(case.dispatch_count(Sc37DispatchSurface::ExternalAdapter), 0);
}

#[test]
fn sc37_deletion_rejected_with_zero_dispatch() {
    let case = deny_case(
        "sc37_deletion_legal_hold",
        Sc37DenyFamily::Deletion,
        SecurityReasonCode::DataRetentionExpired,
    );
    case.validate()
        .expect("legal hold is a valid zero-effect deny");
    assert_eq!(case.observed_reason, "DATA_RETENTION_EXPIRED");
    assert!(case.same_spine);
}

#[test]
fn sc37_entrypoint_bypass_rejected_with_zero_dispatch() {
    let case = deny_case(
        "sc37_entrypoint_bypass_direct_broker_route",
        Sc37DenyFamily::EntrypointBypass,
        SecurityReasonCode::AuthCallerUntrusted,
    );
    case.validate()
        .expect("entrypoint bypass is a valid zero-effect deny");
    assert_eq!(case.observed_reason, "AUTH_CALLER_UNTRUSTED");
    assert_eq!(
        case.dispatch_count(Sc37DispatchSurface::CapabilityBroker),
        0
    );
}

#[test]
fn sc37_matrix_covers_every_family_and_totals_zero_dispatch() {
    let cases = vec![
        deny_case(
            "sc37_privilege_escalation_forged_role",
            Sc37DenyFamily::PrivilegeEscalation,
            SecurityReasonCode::AuthRoleMismatch,
        ),
        deny_case(
            "sc37_replay_consumed_permit",
            Sc37DenyFamily::Replay,
            SecurityReasonCode::PolicyApprovalBindingMismatch,
        ),
        deny_case(
            "sc37_leakage_secret_in_prompt",
            Sc37DenyFamily::Leakage,
            SecurityReasonCode::SecretRedactionFailed,
        ),
        deny_case(
            "sc37_toctou_generation_drift",
            Sc37DenyFamily::Toctou,
            SecurityReasonCode::FilesystemToctou,
        ),
        deny_case(
            "sc37_deletion_legal_hold",
            Sc37DenyFamily::Deletion,
            SecurityReasonCode::DataRetentionExpired,
        ),
        deny_case(
            "sc37_entrypoint_bypass_direct_broker_route",
            Sc37DenyFamily::EntrypointBypass,
            SecurityReasonCode::AuthCallerUntrusted,
        ),
    ];
    let matrix = Sc37DenyMatrix::new("sc37-deny-matrix", cases).expect("SC-37 matrix");
    matrix.validate().expect("matrix validates");
    assert_eq!(matrix.schema, SC37_DENY_MATRIX_SCHEMA);
    assert_eq!(matrix.total_dispatch_count(), 0);
    for family in Sc37DenyFamily::ALL {
        assert!(
            !matrix.family_cases(*family).is_empty(),
            "{family:?} uncovered"
        );
    }
}

#[test]
fn sc37_deny_with_nonzero_dispatch_and_unstable_reason_are_rejected() {
    let base = deny_case(
        "sc37_leakage_secret_in_prompt",
        Sc37DenyFamily::Leakage,
        SecurityReasonCode::SecretRedactionFailed,
    );

    // A deny that still reached a provider is not a deny: the contract must refuse to build it.
    let dispatched = base
        .clone()
        .with_dispatch(Sc37DispatchSurface::ModelProvider, 1)
        .unwrap_err();
    assert_eq!(dispatched, "sc37_deny_case_effect_not_zero");

    // An unstable/aliased reason string is refused: fixtures assert the exact stable code.
    let mut unstable = base.clone();
    unstable.observed_reason = "permission_denied".to_owned();
    unstable.case_digest = unstable.digest();
    assert_eq!(
        unstable.validate().unwrap_err(),
        "sc37_deny_case_reason_or_spine_invalid"
    );

    // A missing dispatch surface is refused rather than defaulting to zero implicitly.
    let mut partial = base;
    partial
        .dispatch_counts
        .remove(Sc37DispatchSurface::ExternalAdapter.as_str());
    partial.case_digest = partial.digest();
    assert_eq!(
        partial.validate().unwrap_err(),
        "sc37_deny_case_dispatch_surface_set_invalid"
    );
}

#[test]
fn sc37_matrix_missing_family_is_rejected() {
    // Only one family is covered, so the matrix must refuse to seal.
    let cases = vec![deny_case(
        "sc37_privilege_escalation_forged_role",
        Sc37DenyFamily::PrivilegeEscalation,
        SecurityReasonCode::AuthRoleMismatch,
    )];
    assert_eq!(
        Sc37DenyMatrix::new("sc37-deny-matrix", cases).unwrap_err(),
        "sc37_deny_family_uncovered"
    );
}

#[test]
fn sc37_matrix_duplicate_case_name_is_rejected() {
    let duplicate = deny_case(
        "sc37_privilege_escalation_forged_role",
        Sc37DenyFamily::PrivilegeEscalation,
        SecurityReasonCode::AuthRoleMismatch,
    );
    let mut matrix = Sc37DenyMatrix {
        schema: SC37_DENY_MATRIX_SCHEMA.to_owned(),
        version: SC37_DENY_MATRIX_VERSION,
        matrix_id: "sc37-deny-matrix".to_owned(),
        cases: vec![duplicate.clone(), duplicate],
        matrix_digest: String::new(),
    };
    matrix.matrix_digest = matrix.digest();
    assert_eq!(matrix.validate().unwrap_err(), "sc37_deny_case_duplicate");
}

// ---------------------------------------------------------------------------
// SC-38: 随机 payload、截断事件、重复 frame、乱序 cursor、unknown major 导致 allow
// ---------------------------------------------------------------------------

fn property_case(
    name: &str,
    shape: Sc38InputShape,
    invariant: Sc38Invariant,
    reason: SecurityReasonCode,
) -> Sc38PropertyCase {
    let input = digest_of(&serde_json::json!({"case": name, "shape": shape.as_str()}));
    let decision = digest_of(&serde_json::json!({"case": name, "reason": reason.as_str()}));
    Sc38PropertyCase::new(
        name,
        0x5C37_0000_0000_0001,
        0,
        shape,
        invariant,
        reason,
        input,
        decision,
    )
    .expect("SC-38 property case")
}

#[test]
fn sc38_random_payload_rejected_and_replay_is_deterministic() {
    let case = property_case(
        "sc38_random_payload",
        Sc38InputShape::RandomPayload,
        Sc38Invariant::DecodeFailsClosed,
        SecurityReasonCode::UnknownUnclassified,
    );
    case.validate().expect("random payload is a valid refusal");
    assert_eq!(case.observed_reason, "UNKNOWN_UNCLASSIFIED");
    assert!(case.outcome.is_refusal());
    assert_ne!(case.decision_digest, case.input_digest);
}

#[test]
fn sc38_truncated_frame_rejected() {
    let case = property_case(
        "sc38_truncated_frame",
        Sc38InputShape::TruncatedFrame,
        Sc38Invariant::DecodeFailsClosed,
        SecurityReasonCode::FactSchemaUnknownMajor,
    );
    case.validate().expect("truncated frame is a valid refusal");
    assert_eq!(case.outcome, Sc38Outcome::Denied);
}

#[test]
fn sc38_duplicate_frame_is_deduplicated_not_re_effect() {
    let mut case = property_case(
        "sc38_duplicate_frame",
        Sc38InputShape::DuplicateFrame,
        Sc38Invariant::DuplicateDeduplicated,
        SecurityReasonCode::PolicyApprovalBindingMismatch,
    );
    case.outcome = Sc38Outcome::Deduplicated;
    case.case_digest = case.digest();
    case.validate().expect("duplicate frame deduplicates");
    assert_eq!(case.outcome, Sc38Outcome::Deduplicated);

    // A duplicate frame recorded as a fresh denial outcome is refused: it must dedup.
    let mut wrong = property_case(
        "sc38_duplicate_frame_wrong_outcome",
        Sc38InputShape::DuplicateFrame,
        Sc38Invariant::DuplicateDeduplicated,
        SecurityReasonCode::PolicyApprovalBindingMismatch,
    );
    wrong.outcome = Sc38Outcome::Denied;
    wrong.case_digest = wrong.digest();
    assert_eq!(
        wrong.validate().unwrap_err(),
        "sc38_property_case_duplicate_outcome_invalid"
    );
}

#[test]
fn sc38_out_of_order_cursor_rejected() {
    let case = property_case(
        "sc38_out_of_order_cursor",
        Sc38InputShape::OutOfOrderCursor,
        Sc38Invariant::CursorOrderEnforced,
        SecurityReasonCode::FactSequenceRollback,
    );
    case.validate()
        .expect("out-of-order cursor is a valid refusal");
    assert_eq!(case.observed_reason, "FACT_SEQUENCE_ROLLBACK");
}

#[test]
fn sc38_unknown_major_is_rejected_not_upgraded() {
    let case = property_case(
        "sc38_unknown_major",
        Sc38InputShape::UnknownMajor,
        Sc38Invariant::UnknownMajorRejected,
        SecurityReasonCode::FactSchemaUnknownMajor,
    );
    case.validate().expect("unknown major is a valid refusal");
    assert_eq!(case.observed_reason, "FACT_SCHEMA_UNKNOWN_MAJOR");
    assert_eq!(case.shape, Sc38InputShape::UnknownMajor);
}

#[test]
fn sc38_unknown_major_invariant_requires_unknown_major_shape() {
    let case = property_case(
        "sc38_mismatched_shape",
        Sc38InputShape::RandomPayload,
        Sc38Invariant::UnknownMajorRejected,
        SecurityReasonCode::FactSchemaUnknownMajor,
    );
    assert_eq!(
        case.validate().unwrap_err(),
        "sc38_property_case_unknown_major_shape_invalid"
    );
}

#[test]
fn sc38_replay_divergence_is_rejected() {
    let case = property_case(
        "sc38_replay_divergent",
        Sc38InputShape::RandomPayload,
        Sc38Invariant::ReplayDeterministic,
        SecurityReasonCode::UnknownUnclassified,
    );
    let mut diverged = case;
    diverged.replay_decision_digest = digest_of(&serde_json::json!({"other": true}));
    diverged.case_digest = diverged.digest();
    assert_eq!(
        diverged.validate().unwrap_err(),
        "sc38_property_case_replay_diverged"
    );
}

#[test]
fn sc38_corpus_covers_every_shape_and_replays_deterministically() {
    let cases = vec![
        property_case(
            "sc38_random_payload",
            Sc38InputShape::RandomPayload,
            Sc38Invariant::DecodeFailsClosed,
            SecurityReasonCode::UnknownUnclassified,
        ),
        property_case(
            "sc38_truncated_frame",
            Sc38InputShape::TruncatedFrame,
            Sc38Invariant::DecodeFailsClosed,
            SecurityReasonCode::FactSchemaUnknownMajor,
        ),
        {
            let mut duplicate = property_case(
                "sc38_duplicate_frame",
                Sc38InputShape::DuplicateFrame,
                Sc38Invariant::DuplicateDeduplicated,
                SecurityReasonCode::PolicyApprovalBindingMismatch,
            );
            duplicate.outcome = Sc38Outcome::Deduplicated;
            duplicate.case_digest = duplicate.digest();
            duplicate
        },
        property_case(
            "sc38_out_of_order_cursor",
            Sc38InputShape::OutOfOrderCursor,
            Sc38Invariant::CursorOrderEnforced,
            SecurityReasonCode::FactSequenceRollback,
        ),
        property_case(
            "sc38_unknown_major",
            Sc38InputShape::UnknownMajor,
            Sc38Invariant::UnknownMajorRejected,
            SecurityReasonCode::FactSchemaUnknownMajor,
        ),
    ];
    let corpus =
        Sc38PropertyCorpus::new("sc38-corpus", 0x5C37_0000_0000_0001, "source:sc38", cases)
            .expect("SC-38 corpus");
    corpus.validate().expect("corpus validates");
    assert_eq!(corpus.schema, SC38_PROPERTY_CORPUS_SCHEMA);

    // Same seed, same order → byte-identical digest.
    let replay = Sc38PropertyCorpus::new(
        "sc38-corpus",
        0x5C37_0000_0000_0001,
        "source:sc38",
        corpus.cases.clone(),
    )
    .expect("deterministic rebuild");
    assert_eq!(replay.corpus_digest, corpus.corpus_digest);

    // Order-independent: reversing cases and re-sealing yields a valid corpus with a distinct
    // ordering digest, proving the corpus itself is deterministic given a fixed case order.
    let reversed = corpus.replay_reversed().expect("reversed replay validates");
    reversed.validate().expect("reversed corpus validates");
    assert_eq!(reversed.cases.len(), corpus.cases.len());
}

#[test]
fn sc38_missing_shape_or_seed_mismatch_is_rejected() {
    let single = property_case(
        "sc38_random_payload",
        Sc38InputShape::RandomPayload,
        Sc38Invariant::DecodeFailsClosed,
        SecurityReasonCode::UnknownUnclassified,
    );
    assert_eq!(
        Sc38PropertyCorpus::new("sc38-corpus", 1, "source:sc38", vec![single.clone()]).unwrap_err(),
        "sc38_property_shape_uncovered"
    );

    let mut wrong_seed = single.clone();
    wrong_seed.seed = 0x5C37_0000_0000_0002;
    wrong_seed.case_digest = wrong_seed.digest();
    let mut corpus = Sc38PropertyCorpus {
        schema: SC38_PROPERTY_CORPUS_SCHEMA.to_owned(),
        version: kiana_domain::SchemaVersion::new(1, 0),
        corpus_id: "sc38-corpus".to_owned(),
        seed: 0x5C37_0000_0000_0001,
        source_snapshot: "source:sc38".to_owned(),
        cases: vec![wrong_seed],
        corpus_digest: String::new(),
    };
    corpus.corpus_digest = corpus.digest();
    assert_eq!(
        corpus.validate().unwrap_err(),
        "sc38_property_case_seed_mismatch"
    );
}

// ---------------------------------------------------------------------------
// SC-39: prompt injection、间接注入、secret exfil、恶意插件/MCP 描述能触发 effect
// ---------------------------------------------------------------------------

fn attack_case(
    id: &str,
    shape: Sc39AttackShape,
    outcome: Sc39ExpectedOutcome,
    reason: SecurityReasonCode,
) -> Sc39AttackCase {
    let text_digest = digest_of(&serde_json::json!({"attack": id, "shape": shape.as_str()}));
    Sc39AttackCase::new(
        id,
        shape,
        text_digest,
        PromptTrustClass::Untrusted,
        outcome,
        reason,
    )
    .expect("SC-39 attack case")
}

#[test]
fn sc39_prompt_injection_cannot_grant_authority() {
    let case = attack_case(
        "sc39_prompt_injection_claims_system_policy",
        Sc39AttackShape::PromptInjection,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::AuthRoleMismatch,
    );
    case.validate()
        .expect("prompt injection is a valid zero-effect refusal");
    assert!(!case.prompt_authority.is_authority());
    assert_eq!(
        case.handler_calls + case.provider_calls + case.effect_count,
        0
    );
}

#[test]
fn sc39_indirect_injection_cannot_trigger_effect() {
    let case = attack_case(
        "sc39_indirect_injection_repository_text",
        Sc39AttackShape::IndirectInjection,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::PolicyGrantWidening,
    );
    case.validate()
        .expect("indirect injection is a valid zero-effect refusal");
    assert_eq!(case.observed_reason, "POLICY_GRANT_WIDENING");
    assert!(case.synthetic_text_only);
}

#[test]
fn sc39_secret_exfiltration_cannot_leave_the_broker() {
    let case = attack_case(
        "sc39_secret_exfiltration_echo_secret",
        Sc39AttackShape::SecretExfiltration,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::SecretReferenceInvalid,
    );
    case.validate()
        .expect("secret exfiltration is a valid zero-effect refusal");
    assert_eq!(case.observed_reason, "SECRET_REFERENCE_INVALID");
    assert!(case.no_external_effect);
}

#[test]
fn sc39_malicious_plugin_cannot_widen_capabilities() {
    let case = attack_case(
        "sc39_malicious_plugin_ungranted_capability",
        Sc39AttackShape::MaliciousPlugin,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::PolicyGrantWidening,
    );
    case.validate()
        .expect("malicious plugin is a valid zero-effect refusal");
    assert_eq!(case.effect_count, 0);
}

#[test]
fn sc39_malicious_mcp_description_requires_approval_or_unknown() {
    let denied = attack_case(
        "sc39_mcp_description_denied",
        Sc39AttackShape::MaliciousMcpDescription,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::AuthAudienceMismatch,
    );
    denied.validate().expect("mcp denial is a valid refusal");

    let approval = attack_case(
        "sc39_mcp_description_approval",
        Sc39AttackShape::MaliciousMcpDescription,
        Sc39ExpectedOutcome::RequiresApproval,
        SecurityReasonCode::PolicyApprovalRequired,
    );
    approval
        .validate()
        .expect("approval-gated mcp attack is a valid refusal");
    assert_eq!(
        approval.expected_outcome,
        Sc39ExpectedOutcome::RequiresApproval
    );

    let unknown = attack_case(
        "sc39_mcp_description_unknown",
        Sc39AttackShape::MaliciousMcpDescription,
        Sc39ExpectedOutcome::Unknown,
        SecurityReasonCode::UnknownUnclassified,
    );
    unknown
        .validate()
        .expect("unknown mcp attack is a valid refusal");
    assert_eq!(unknown.expected_outcome, Sc39ExpectedOutcome::Unknown);
}

#[test]
fn sc39_attack_with_effect_or_product_authority_is_rejected() {
    let case = attack_case(
        "sc39_prompt_injection_claims_system_policy",
        Sc39AttackShape::PromptInjection,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::AuthRoleMismatch,
    );

    let mut effected = case.clone();
    effected.provider_calls = 1;
    effected.case_digest = effected.digest();
    assert_eq!(
        effected.validate().unwrap_err(),
        "sc39_attack_case_effect_not_zero"
    );

    let mut authoritative = case;
    authoritative.prompt_authority = PromptTrustClass::Product;
    authoritative.case_digest = authoritative.digest();
    assert_eq!(
        authoritative.validate().unwrap_err(),
        "sc39_attack_case_prompt_authority_invalid"
    );
}

#[test]
fn sc39_attack_corpus_covers_every_shape_with_no_real_secret() {
    let cases = vec![
        attack_case(
            "sc39_prompt_injection_claims_system_policy",
            Sc39AttackShape::PromptInjection,
            Sc39ExpectedOutcome::Denied,
            SecurityReasonCode::AuthRoleMismatch,
        ),
        attack_case(
            "sc39_indirect_injection_repository_text",
            Sc39AttackShape::IndirectInjection,
            Sc39ExpectedOutcome::Denied,
            SecurityReasonCode::PolicyGrantWidening,
        ),
        attack_case(
            "sc39_secret_exfiltration_echo_secret",
            Sc39AttackShape::SecretExfiltration,
            Sc39ExpectedOutcome::Denied,
            SecurityReasonCode::SecretReferenceInvalid,
        ),
        attack_case(
            "sc39_malicious_plugin_ungranted_capability",
            Sc39AttackShape::MaliciousPlugin,
            Sc39ExpectedOutcome::Denied,
            SecurityReasonCode::PolicyGrantWidening,
        ),
        attack_case(
            "sc39_mcp_description_unknown",
            Sc39AttackShape::MaliciousMcpDescription,
            Sc39ExpectedOutcome::Unknown,
            SecurityReasonCode::UnknownUnclassified,
        ),
    ];
    let corpus =
        Sc39AttackCorpus::new("sc39-attack-corpus", "source:sc39", cases).expect("SC-39 corpus");
    corpus.validate().expect("corpus validates");
    assert_eq!(corpus.schema, SC39_ATTACK_CORPUS_SCHEMA);
    assert_eq!(corpus.total_effect_count(), 0);
    for shape in Sc39AttackShape::ALL {
        assert!(
            corpus.cases.iter().any(|case| case.shape == *shape),
            "{shape:?} uncovered"
        );
    }
    // No case stores attack text — only a digest — so the corpus itself carries no secret.
    let encoded = serde_json::to_string(&corpus).expect("corpus encodes");
    assert!(!encoded.contains("Authorization: Bearer"));
    assert!(!encoded.contains("sk-"));
}

#[test]
fn sc39_corpus_missing_shape_is_rejected() {
    let single = attack_case(
        "sc39_prompt_injection_claims_system_policy",
        Sc39AttackShape::PromptInjection,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::AuthRoleMismatch,
    );
    assert_eq!(
        Sc39AttackCorpus::new("sc39-attack-corpus", "source:sc39", vec![single]).unwrap_err(),
        "sc39_attack_shape_uncovered"
    );
}

#[test]
fn sc39_corpus_duplicate_attack_id_is_rejected() {
    let duplicate = attack_case(
        "sc39_prompt_injection_claims_system_policy",
        Sc39AttackShape::PromptInjection,
        Sc39ExpectedOutcome::Denied,
        SecurityReasonCode::AuthRoleMismatch,
    );
    assert_eq!(
        Sc39AttackCorpus::new(
            "sc39-attack-corpus",
            "source:sc39",
            vec![duplicate.clone(), duplicate],
        )
        .unwrap_err(),
        "sc39_attack_case_duplicate"
    );
}

// ---------------------------------------------------------------------------
// Shared correlation binding
// ---------------------------------------------------------------------------

#[test]
fn fixture_correlation_binds_request_run_and_cursor_without_granting_authority() {
    let request_id = RequestId::new();
    let run_id = RunId::new();
    let correlation =
        ScFixtureCorrelation::new(request_id, Some(run_id), Some(OperationId::new()), Some(7))
            .expect("SC fixture correlation");
    correlation.validate().expect("correlation validates");
    assert_eq!(correlation.request_id, request_id);
    assert_eq!(correlation.run_id, Some(run_id));
    assert_eq!(correlation.source_cursor, Some(7));

    let mut tampered = correlation;
    tampered.source_cursor = Some(8);
    tampered.correlation_digest = tampered.digest();
    assert_eq!(
        tampered.validate().unwrap_err(),
        "sc_fixture_correlation_digest_mismatch"
    );
}

#[test]
fn every_refusal_family_reason_is_a_stable_registered_code() {
    // A fixture may only assert on a reason the domain actually defines; an unregistered code
    // would silently become UNKNOWN_UNCLASSIFIED at parse time.
    let codes: BTreeSet<&str> = SecurityReasonCode::ALL
        .iter()
        .map(|code| code.as_str())
        .collect();
    for expected in [
        SecurityReasonCode::AuthRoleMismatch,
        SecurityReasonCode::PolicyApprovalBindingMismatch,
        SecurityReasonCode::SecretRedactionFailed,
        SecurityReasonCode::FilesystemToctou,
        SecurityReasonCode::DataRetentionExpired,
        SecurityReasonCode::AuthCallerUntrusted,
        SecurityReasonCode::FactSchemaUnknownMajor,
        SecurityReasonCode::FactSequenceRollback,
        SecurityReasonCode::UnknownUnclassified,
    ] {
        assert!(
            codes.contains(expected.as_str()),
            "{} missing",
            expected.as_str()
        );
    }
}
