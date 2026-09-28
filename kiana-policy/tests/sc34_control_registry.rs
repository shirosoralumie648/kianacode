//! SC-34 failure-first fixtures for the security control registry.
//!
//! The card names two rejections to prove before anything else: claiming a regulatory
//! certification from a unit test or a type, and a control whose evidence has no owner. Every test
//! before the last two names one of those, or one of the fail-closed rules that keeps a mapping
//! from silently becoming a claim.
//!
//! Nothing here writes a control into a running system, runs a scanner or contacts an assessor.
//! The module under test is a read-only decision over supplied facts, so these are contract
//! fixtures rather than runtime or audit evidence.
//!
//! The order of the file is the order of the argument. Ownership is shown to be mandatory before
//! any mapping is discussed; the proof ceiling is shown to be unable to outrun the evidence; scope
//! is shown to only narrow; `Unknown` is shown to stay `Unknown`; and only then does a fully
//! evidenced control get registered, and a child get derived from it.

use kiana_policy::{
    ControlEvidence, ControlScope, ControlScopeFacet, ControlStatus, FrameworkClause, ProofLevel,
    SecurityControl, SecurityControlRegistry, SecurityFramework,
};

const OWNER: &str = "control-owner-security";
const PARENT_SCOPE: [ControlScopeFacet; 4] = [
    ControlScopeFacet::Authorization,
    ControlScopeFacet::ExternalEffect,
    ControlScopeFacet::UnknownState,
    ControlScopeFacet::Audit,
];

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn evidence(seed: char, proves: ProofLevel) -> ControlEvidence {
    ControlEvidence::new(format!("event:{seed}"), digest(seed), proves).expect("SC-34 evidence")
}

fn sec_clause(clause: &str) -> FrameworkClause {
    FrameworkClause::new(SecurityFramework::Sec, 1, clause).expect("SC-34 SEC clause")
}

fn parent_scope() -> ControlScope {
    ControlScope::new(PARENT_SCOPE)
}

/// A `durable` claim backed by exactly one `durable` piece of evidence.
///
/// This is the only fixture shape that is allowed to reach `Effective`, and it is still nothing
/// more than a source-level statement that such an evidence reference was supplied.
fn durable_control() -> SecurityControl {
    SecurityControl::new(
        "ctl-incident-reconcile",
        "an unreconciled unknown never becomes a success",
        OWNER,
        None,
        ControlScope::new([ControlScopeFacet::UnknownState, ControlScopeFacet::Audit]),
        vec!["the EventLog is the sole fact source".to_owned()],
        vec![
            sec_clause("SEC-09"),
            FrameworkClause::new(SecurityFramework::Nist, 1, "MANAGE").expect("SC-34 NIST clause"),
        ],
        vec![evidence('a', ProofLevel::Durable)],
        Some(ProofLevel::Durable),
        ControlStatus::Effective,
    )
    .expect("SC-34 durable control")
}

// ---------------------------------------------------------------------------------------------
// Ownership: a control nobody owns is not a control.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_control_with_no_owner_cannot_be_registered() {
    let control = SecurityControl::new(
        "ctl-ownerless",
        "a control with nobody accountable for its evidence",
        "   ",
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_owner_required");
}

#[test]
fn an_owner_containing_a_control_character_is_malformed_not_missing() {
    // A newline cannot appear in an owner string: it would split one accountable party into two
    // lines of an audit record and let the second line read as a different owner.
    let control = SecurityControl::new(
        "ctl-newline-owner",
        "an owner spanning two lines names two parties, not one",
        "control-owner\ncontrol-owner",
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_owner_invalid");
}

#[test]
fn a_malformed_owner_is_rejected_as_malformed_not_as_missing() {
    let control = SecurityControl::new(
        "ctl-bad-owner",
        "an owner that carries a secret is a different defect from an absent one",
        "https://alice:hunter2@controls.internal/",
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_owner_secret_detected"
    );
}

#[test]
fn a_registry_refuses_to_even_hold_a_control_with_no_owner() {
    let mut registry = SecurityControlRegistry::new();
    // Hand-assemble the record so the owner check is exercised on the registry path rather than
    // only on the constructor. `register` re-validates, so the empty owner is still refused.
    let mut forged = durable_control();
    forged.owner = String::new();
    forged.control_digest = forged.digest();
    assert_eq!(
        registry.register(forged).unwrap_err(),
        "security_control_owner_required"
    );
    assert!(registry.controls.is_empty());
}

// ---------------------------------------------------------------------------------------------
// The proof ceiling may not outrun the evidence. This is the card's headline rejection.
// ---------------------------------------------------------------------------------------------

#[test]
fn source_evidence_cannot_be_registered_as_durable() {
    let control = SecurityControl::new(
        "ctl-type-is-not-certification",
        "a passing unit test and a well-typed struct are not a regulatory audit",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        // The claim: this control is proven durable. The evidence: a source-level fixture.
        Some(ProofLevel::Durable),
        ControlStatus::Effective,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_proof_ceiling_exceeds_evidence"
    );
}

#[test]
fn the_proof_ceiling_cannot_be_even_one_step_above_the_evidence() {
    let control = SecurityControl::new(
        "ctl-one-step-short",
        "local evidence does not become durable evidence by being restated",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::LocalBehavior)],
        Some(ProofLevel::Durable),
        ControlStatus::Partial,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_proof_ceiling_exceeds_evidence"
    );
}

#[test]
fn several_weak_pieces_of_evidence_do_not_add_up_to_a_stronger_claim() {
    // Three `source` pieces are not stronger than one `source` piece. Max, not sum: this is the
    // reason `demonstrated_proof` takes a maximum instead of counting evidence.
    let control = SecurityControl::new(
        "ctl-evidence-stacking",
        "stacking source-level evidence does not manufacture durable proof",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![
            evidence('a', ProofLevel::Source),
            evidence('b', ProofLevel::Source),
            evidence('c', ProofLevel::Source),
        ],
        Some(ProofLevel::LocalBehavior),
        ControlStatus::Partial,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_proof_ceiling_exceeds_evidence"
    );
}

#[test]
fn claiming_a_ceiling_with_no_evidence_at_all_is_refused() {
    let control = SecurityControl::new(
        "ctl-ceiling-without-evidence",
        "a ceiling with nothing under it is a wish, not a control",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_evidence_required");
}

#[test]
fn a_control_with_no_evidence_cannot_claim_a_pass() {
    let control = SecurityControl::new(
        "ctl-unevidenced-pass",
        "an unverified control is not a passing control",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![],
        None,
        ControlStatus::Effective,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_status_not_derivable"
    );
}

// ---------------------------------------------------------------------------------------------
// Unknown must stay Unknown.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_control_with_no_evidence_derives_unknown_and_not_effective() {
    let control = SecurityControl::new(
        "ctl-unknown-honest",
        "an unverified control honestly reports itself as unknown",
        OWNER,
        None,
        parent_scope(),
        vec!["nothing has been verified yet".to_owned()],
        vec![sec_clause("SEC-10")],
        vec![],
        None,
        ControlStatus::Unknown,
    )
    .expect("SC-34 unknown control");
    assert_eq!(control.honest_status(), ControlStatus::Unknown);
    assert_eq!(control.demonstrated_proof(), None);
}

#[test]
fn evidence_that_falls_short_of_the_claim_derives_partial_not_effective() {
    let control = SecurityControl::new(
        "ctl-partial-honest",
        "a control short of its own ceiling reports partial",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        // No ceiling is claimed, so nothing is overstated. The evidence is real but weak, and
        // the honest name for that is Partial, never Effective.
        None,
        ControlStatus::Partial,
    )
    .expect("SC-34 partial control");
    assert_eq!(control.honest_status(), ControlStatus::Partial);
    assert_ne!(control.honest_status(), ControlStatus::Effective);
}

#[test]
fn a_control_with_no_ceiling_cannot_be_promoted_to_effective_by_its_status_field() {
    // `proof_ceiling: None` means the control claims nothing. Claiming nothing while writing
    // `Effective` is the same category of error as claiming too much: the status field is not a
    // place where a conclusion may be written directly.
    let control = SecurityControl::new(
        "ctl-status-not-a-conclusion",
        "the status field is not a place to write a conclusion",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![],
        None,
        ControlStatus::Partial,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_status_not_derivable"
    );
}

// ---------------------------------------------------------------------------------------------
// Unknown framework, unknown major, unknown clause all fail closed.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_unknown_framework_is_refused() {
    assert_eq!(
        SecurityFramework::parse("soc2").unwrap_err(),
        "security_control_framework_unknown"
    );
}

#[test]
fn an_unknown_framework_major_is_refused() {
    let mapping = FrameworkClause::new(SecurityFramework::Owasp, 2, "LLM01");
    assert_eq!(
        mapping.unwrap_err(),
        "security_control_framework_major_unknown"
    );
}

#[test]
fn a_clause_from_a_different_framework_is_refused() {
    // `LLM01` is a real OWASP identifier, but it is not a SEC identifier. A cross-framework
    // identifier is exactly the kind of near-miss that a fuzzy matcher would accept.
    let mapping = FrameworkClause::new(SecurityFramework::Sec, 1, "LLM01");
    assert_eq!(
        mapping.unwrap_err(),
        "security_control_framework_clause_unknown"
    );
}

#[test]
fn an_invented_clause_is_refused() {
    let mapping = FrameworkClause::new(SecurityFramework::Nist, 1, "GOVERN-MAYBE");
    assert_eq!(
        mapping.unwrap_err(),
        "security_control_framework_clause_unknown"
    );
}

#[test]
fn a_near_miss_clause_number_is_refused() {
    // SEC-12 exists, SEC-13 does not. One digit of drift must not read as a mapped control.
    let mapping = FrameworkClause::new(SecurityFramework::Sec, 1, "SEC-13");
    assert_eq!(
        mapping.unwrap_err(),
        "security_control_framework_clause_unknown"
    );
}

#[test]
fn a_control_with_no_framework_mapping_is_refused() {
    let control = SecurityControl::new(
        "ctl-unmapped",
        "a control mapped to nothing controls nothing",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_mapping_required");
}

#[test]
fn a_repeated_mapping_is_refused() {
    let control = SecurityControl::new(
        "ctl-duplicate-mapping",
        "the same clause listed twice is one mapping with extra ink",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10"), sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_mapping_duplicate");
}

#[test]
fn an_unknown_field_in_a_control_is_refused() {
    // `deny_unknown_fields` is the only reason a future field cannot be silently ignored by an
    // older reader. Adding a field here must fail, not default.
    let json = serde_json::json!({
        "schema": kiana_policy::SECURITY_CONTROL_SCHEMA,
        "version": {"major": 1, "minor": 0},
        "control_id": "ctl-extra-field",
        "title": "a record with a field nobody defined",
        "owner": OWNER,
        "parent": null,
        "scope": ["audit"],
        "assumptions": [],
        "mappings": [{"framework": "sec", "major_version": 1, "clause": "SEC-10"}],
        "evidence": [],
        "proof_ceiling": null,
        "status": "unknown",
        "control_digest": digest('a'),
        "attested_by": "someone"
    });
    assert!(serde_json::from_value::<SecurityControl>(json).is_err());
}

#[test]
fn an_unknown_status_word_is_refused() {
    assert_eq!(
        ControlStatus::parse("certified").unwrap_err(),
        "security_control_status_invalid"
    );
}

#[test]
fn an_unknown_proof_level_word_is_refused() {
    assert_eq!(
        ProofLevel::parse("mostly_trusted").unwrap_err(),
        "security_control_proof_level_invalid"
    );
}

// ---------------------------------------------------------------------------------------------
// Scope only narrows.
// ---------------------------------------------------------------------------------------------

#[test]
fn an_empty_scope_is_refused() {
    let control = SecurityControl::new(
        "ctl-no-scope",
        "a control that covers nothing is not a control",
        OWNER,
        None,
        ControlScope::new([]),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_scope_empty");
}

#[test]
fn a_child_scope_is_the_intersection_with_its_parent() {
    let parent = durable_control();
    let requested = ControlScope::new([
        // Inherited from the parent.
        ControlScopeFacet::UnknownState,
        // NOT in the parent: must be dropped by the intersection.
        ControlScopeFacet::Secret,
    ]);
    let child = parent
        .derive_child(
            "ctl-incident-reconcile-audit-only",
            "the audit-facing half of the reconcile control",
            OWNER,
            &requested,
            vec![],
            vec![sec_clause("SEC-09")],
            vec![evidence('b', ProofLevel::Source)],
            Some(ProofLevel::Source),
            ControlStatus::Partial,
        )
        .expect("SC-34 derived child");
    assert!(child
        .scope
        .facets
        .contains(&ControlScopeFacet::UnknownState));
    assert!(!child.scope.facets.contains(&ControlScopeFacet::Secret));
    assert_eq!(child.parent.as_deref(), Some("ctl-incident-reconcile"));
}

#[test]
fn a_child_that_requests_only_facets_outside_its_parent_ends_up_covering_nothing() {
    let parent = durable_control();
    let requested = ControlScope::new([ControlScopeFacet::ResourceExhaustion]);
    let child = parent.derive_child(
        "ctl-disjoint",
        "a child asking only for a facet the parent does not cover",
        OWNER,
        &requested,
        vec![],
        vec![sec_clause("SEC-09")],
        vec![evidence('b', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(
        child.unwrap_err(),
        "security_control_scope_intersection_empty"
    );
}

#[test]
fn a_hand_widened_child_is_refused_at_registration() {
    // `derive_child` cannot widen, but a serialized record can be edited. The registry re-checks
    // the subset property rather than trusting that every writer went through `derive_child`.
    let mut registry = SecurityControlRegistry::new();
    registry.register(durable_control()).expect("SC-34 parent");
    let mut widened = durable_control();
    widened.control_id = "ctl-widened".to_owned();
    widened.parent = Some("ctl-incident-reconcile".to_owned());
    widened.scope = ControlScope::new([ControlScopeFacet::Secret]);
    widened.control_digest = widened.digest();
    assert_eq!(
        registry.register(widened).unwrap_err(),
        "security_control_scope_widened"
    );
}

#[test]
fn a_widened_child_is_caught_even_if_it_lands_in_the_registry_without_register() {
    // Same property, but discovered by re-validating a deserialized registry: a record edited on
    // disk and loaded back must not be accepted just because `register` was bypassed.
    let mut registry = SecurityControlRegistry::new();
    registry.register(durable_control()).expect("SC-34 parent");
    let mut widened = durable_control();
    widened.control_id = "ctl-widened-reload".to_owned();
    widened.parent = Some("ctl-incident-reconcile".to_owned());
    widened.scope = ControlScope::new([ControlScopeFacet::Secret]);
    widened.control_digest = widened.digest();
    registry
        .controls
        .insert(widened.control_id.clone(), widened);
    registry.registry_digest = registry.digest();
    assert_eq!(
        registry.validate().unwrap_err(),
        "security_control_scope_widened"
    );
}

#[test]
fn a_child_whose_parent_was_never_registered_is_refused() {
    let mut registry = SecurityControlRegistry::new();
    let mut orphan = durable_control();
    orphan.control_id = "ctl-orphan".to_owned();
    orphan.parent = Some("ctl-never-registered".to_owned());
    orphan.control_digest = orphan.digest();
    assert_eq!(
        registry.register(orphan).unwrap_err(),
        "security_control_parent_unknown"
    );
}

#[test]
fn a_control_that_is_its_own_parent_is_refused() {
    let control = SecurityControl::new(
        "ctl-self",
        "a control that inherits from itself describes no inheritance at all",
        OWNER,
        Some("ctl-self".to_owned()),
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![evidence('a', ProofLevel::Source)],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(
        control.unwrap_err(),
        "security_control_parent_self_reference"
    );
}

#[test]
fn an_unknown_scope_facet_is_refused() {
    assert_eq!(
        ControlScopeFacet::parse("teleportation").unwrap_err(),
        "security_control_scope_facet_unknown"
    );
}

// ---------------------------------------------------------------------------------------------
// Digest binding: a record cannot be edited and republished as if it had been decided here.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_tampered_control_digest_is_refused() {
    let mut control = durable_control();
    control.status = ControlStatus::Partial;
    // The digest still covers the old status.
    assert_eq!(
        control.validate().unwrap_err(),
        "security_control_digest_mismatch"
    );
}

#[test]
fn a_tampered_registry_digest_is_refused() {
    let mut registry = SecurityControlRegistry::new();
    registry
        .register(durable_control())
        .expect("SC-34 register");
    registry.registry_digest = digest('b');
    assert_eq!(
        registry.validate().unwrap_err(),
        "security_control_registry_digest_mismatch"
    );
}

#[test]
fn a_control_swapped_inside_a_registry_is_caught_by_the_registry_digest() {
    let mut registry = SecurityControlRegistry::new();
    registry
        .register(durable_control())
        .expect("SC-34 register");
    let mut swapped = durable_control();
    swapped.control_id = "ctl-swapped".to_owned();
    swapped.owner = "someone-else".to_owned();
    swapped.control_digest = swapped.digest();
    registry.controls.insert("ctl-swapped".to_owned(), swapped);
    // The registry digest was not recomputed, so the substitution is visible.
    assert_eq!(
        registry.validate().unwrap_err(),
        "security_control_registry_digest_mismatch"
    );
}

#[test]
fn a_duplicate_control_id_is_refused() {
    let mut registry = SecurityControlRegistry::new();
    registry.register(durable_control()).expect("SC-34 first");
    assert_eq!(
        registry.register(durable_control()).unwrap_err(),
        "security_control_duplicate"
    );
    assert_eq!(registry.controls.len(), 1);
}

#[test]
fn the_same_evidence_cannot_be_listed_twice_on_one_control() {
    let control = SecurityControl::new(
        "ctl-duplicate-evidence",
        "the same committed fact listed twice is one piece of evidence",
        OWNER,
        None,
        parent_scope(),
        vec![],
        vec![sec_clause("SEC-10")],
        vec![
            evidence('a', ProofLevel::Source),
            evidence('a', ProofLevel::Source),
        ],
        Some(ProofLevel::Source),
        ControlStatus::Partial,
    );
    assert_eq!(control.unwrap_err(), "security_control_evidence_duplicate");
}

#[test]
fn an_evidence_reference_that_carries_a_secret_is_refused() {
    // Evidence is a reference plus a digest, never a payload. A reference that needs to contain
    // the value in order to be useful is a second copy of the secret.
    let evidence = ControlEvidence::new(
        "https://alice:hunter2@evidence.internal/",
        digest('a'),
        ProofLevel::Source,
    );
    assert_eq!(
        evidence.unwrap_err(),
        "security_control_evidence_ref_secret_detected"
    );
}

#[test]
fn a_control_cannot_publish_itself_through_a_bare_digest() {
    let control = durable_control();
    assert!(control.control_digest.starts_with("sha256:"));
    // A digest that does not cover its own contents is refused, so the seal cannot be recomputed
    // around an edit.
    let mut relabelled = control.clone();
    relabelled.owner = "control-owner-security-v2".to_owned();
    assert!(relabelled.control_digest != relabelled.digest());
}

// ---------------------------------------------------------------------------------------------
// The success paths, last.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_fully_evidenced_control_registers_and_derives_a_narrower_child() {
    let mut registry = SecurityControlRegistry::new();
    registry
        .register(durable_control())
        .expect("SC-34 register parent");

    let parent = registry
        .get("ctl-incident-reconcile")
        .expect("SC-34 parent present");
    assert_eq!(parent.demonstrated_proof(), Some(ProofLevel::Durable));
    assert_eq!(parent.honest_status(), ControlStatus::Effective);

    let child = parent
        .derive_child(
            "ctl-incident-reconcile-secrets",
            "the secret-leak half of the reconcile control",
            "control-owner-secrets",
            &ControlScope::new([ControlScopeFacet::UnknownState, ControlScopeFacet::Secret]),
            vec!["a secret never appears in the record".to_owned()],
            vec![sec_clause("SEC-05")],
            vec![evidence('c', ProofLevel::Source)],
            Some(ProofLevel::Source),
            ControlStatus::Partial,
        )
        .expect("SC-34 derive child");
    registry.register(child).expect("SC-34 register child");

    registry.validate().expect("SC-34 registry validates");
    assert_eq!(registry.controls.len(), 2);
    let child = registry
        .get("ctl-incident-reconcile-secrets")
        .expect("SC-34 child present");
    // The intersection dropped `Secret` because the parent never covered it.
    assert_eq!(
        child.scope.facets.iter().copied().collect::<Vec<_>>(),
        vec![ControlScopeFacet::UnknownState]
    );
}

#[test]
fn a_registered_registry_round_trips_through_json_without_losing_its_binding() {
    let mut registry = SecurityControlRegistry::new();
    registry
        .register(durable_control())
        .expect("SC-34 register");
    let encoded = serde_json::to_string(&registry).expect("SC-34 encode");
    let decoded: SecurityControlRegistry = serde_json::from_str(&encoded).expect("SC-34 decode");
    assert_eq!(decoded, registry);
    decoded
        .validate()
        .expect("SC-34 reloaded registry validates");
}

#[test]
fn an_unevidenced_control_may_still_be_registered_as_unknown() {
    // A placeholder control is legitimate: it says "this is what we would have to prove, and we
    // have not proved it". What is not legitimate is promoting that placeholder past `Unknown`.
    let mut registry = SecurityControlRegistry::new();
    let pending = SecurityControl::new(
        "ctl-provider-isolation",
        "a real provider boundary, not yet demonstrated",
        "control-owner-providers",
        None,
        ControlScope::new([ControlScopeFacet::ExternalEffect]),
        vec!["no live provider has been exercised".to_owned()],
        vec![sec_clause("SEC-04")],
        vec![],
        None,
        ControlStatus::Unknown,
    )
    .expect("SC-34 pending control");
    assert_eq!(pending.honest_status(), ControlStatus::Unknown);
    registry.register(pending).expect("SC-34 register pending");
    registry.validate().expect("SC-34 registry validates");
}
