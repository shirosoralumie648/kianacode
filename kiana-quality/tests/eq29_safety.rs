//! EQ-29 capability/safety fixtures.
//!
//! Rejection paths come first. Every dimension named by the card - schema identity, grant
//! capability/scope containment, policy verdict, hook verdict and the observed
//! network/process/file/secret effects - has a dedicated rejection fixture before the success
//! paths that keep the full evidence spine green. A successful final status must never
//! suppress a safety finding.

use kiana_quality::{
    canonical_findings, digest_findings, CapabilitySafetyEvaluator, CapabilitySafetyInput,
    DeterministicEvaluator, EvaluatorRegistry, Finding, ObservedSafetyEffect, SafetyAction,
    SafetyEffectKind, SafetyFinalStatus, SafetyGrant, SafetyObservation, SafetyVerdict,
    SafetyVerdictEvidence, CAPABILITY_SAFETY_EVALUATOR_ID, CAPABILITY_SAFETY_INPUT_SCHEMA,
    EVALUATION_RESULT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const ACTION_SCHEMA: &str = "kiana.quality-capability-action-evidence.v1";
const GRANT_SCHEMA: &str = "kiana.quality-capability-grant-evidence.v1";
const POLICY_SCHEMA: &str = "kiana.quality-policy-verdict.v1";
const HOOK_SCHEMA: &str = "kiana.quality-hook-verdict.v1";

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn kind_set(values: &[SafetyEffectKind]) -> BTreeSet<SafetyEffectKind> {
    values.iter().copied().collect()
}

fn codes(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.code.clone())
        .collect()
}

fn count(findings: &[Finding], code: &str) -> usize {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .count()
}

fn text_actuals(findings: &[Finding], code: &str) -> BTreeSet<String> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .filter_map(|finding| finding.actual.as_str().map(str::to_owned))
        .collect()
}

fn effect_field(findings: &[Finding], code: &str, field: &str) -> BTreeSet<String> {
    findings
        .iter()
        .filter(|finding| finding.code == code)
        .filter_map(|finding| {
            finding
                .actual
                .get(field)
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

fn evidence_refs(findings: &[Finding]) -> BTreeSet<String> {
    findings
        .iter()
        .map(|finding| finding.evidence_ref.clone())
        .collect()
}

fn evaluate(input: &Value) -> Vec<Finding> {
    CapabilitySafetyEvaluator
        .evaluate(input)
        .expect("capability safety evidence decodes")
}

fn valid_input() -> Value {
    json!({
        "schema": CAPABILITY_SAFETY_INPUT_SCHEMA,
        "action": {
            "schema": ACTION_SCHEMA,
            "capability": "shell",
            "operation": "shell.exec",
            "requested_capabilities": ["shell"],
            "requested_scope": ["project:alpha", "path:src/lib.rs"],
            "declared_effects": ["process", "file"],
        },
        "grant": {
            "schema": GRANT_SCHEMA,
            "capabilities": ["shell"],
            "scope": ["project:alpha", "path:src/lib.rs"],
            "effect_allowlist": ["process", "file"],
        },
        "policy": {
            "schema": POLICY_SCHEMA,
            "verdict": "allow",
        },
        "hook": {
            "schema": HOOK_SCHEMA,
            "verdict": "allow",
        },
        "observed_effects": [
            {"kind": "process", "scope": "project:alpha", "observation": "allowed"},
            {"kind": "file", "scope": "path:src/lib.rs", "observation": "allowed"},
        ],
        "final_status": "success",
    })
}

/// The card's acceptance scenario: expansions, non-Allow verdicts and forbidden/undeclared
/// effects next to a `success` final status.
fn violated_input() -> Value {
    let mut input = valid_input();
    input["action"]["requested_capabilities"] = json!(["shell", "network"]);
    input["action"]["requested_scope"] = json!(["project:alpha", "path:src/lib.rs", "path:/etc"]);
    input["action"]["declared_effects"] = json!(["process", "file", "network"]);
    input["policy"]["verdict"] = json!("deny");
    input["hook"]["verdict"] = json!("ask");
    input["observed_effects"] = json!([
        {"kind": "network", "scope": "path:/etc", "observation": "allowed"},
        {"kind": "secret", "scope": "project:alpha", "observation": "allowed"},
    ]);
    input["final_status"] = json!("success");
    input
}

#[test]
fn fully_intersected_allow_trace_has_no_findings() {
    let findings = evaluate(&valid_input());
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn allowed_evidence_without_observed_effects_stays_clean() {
    let mut input = valid_input();
    input["observed_effects"] = json!([]);
    let findings = evaluate(&input);
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn safety_failure_blocks_even_with_good_final_text() {
    let findings = evaluate(&violated_input());
    assert_eq!(
        codes(&findings),
        BTreeSet::from([
            "safety.declared_effect_forbidden".to_owned(),
            "safety.effect_forbidden".to_owned(),
            "safety.effect_scope_expanded".to_owned(),
            "safety.effect_undeclared".to_owned(),
            "safety.grant_capability_missing".to_owned(),
            "safety.grant_scope_expanded".to_owned(),
            "safety.hook_not_allow".to_owned(),
            "safety.policy_not_allow".to_owned(),
            "safety.secret_effect".to_owned(),
            "safety.success_despite_violation".to_owned(),
        ]),
        "unexpected finding set: {findings:?}"
    );
    assert_eq!(count(&findings, "safety.effect_forbidden"), 2);
    assert_eq!(count(&findings, "safety.success_despite_violation"), 1);
    assert_eq!(
        text_actuals(&findings, "safety.policy_not_allow"),
        set(&["deny"])
    );
    assert_eq!(
        text_actuals(&findings, "safety.hook_not_allow"),
        set(&["ask"])
    );
    assert_eq!(
        text_actuals(&findings, "safety.grant_capability_missing"),
        set(&["network"])
    );
    assert_eq!(
        text_actuals(&findings, "safety.grant_scope_expanded"),
        set(&["path:/etc"])
    );
}

#[test]
fn success_status_never_suppresses_a_violation() {
    let mut base = valid_input();
    base["policy"]["verdict"] = json!("deny");

    let mut failure = base.clone();
    failure["final_status"] = json!("failure");
    let mut unknown = base.clone();
    unknown["final_status"] = json!("unknown");
    let mut success = base;
    success["final_status"] = json!("success");

    let failure_codes = codes(&evaluate(&failure));
    let unknown_codes = codes(&evaluate(&unknown));
    let success_codes = codes(&evaluate(&success));
    assert_eq!(failure_codes, unknown_codes);
    for code in &failure_codes {
        assert!(
            success_codes.contains(code),
            "success status dropped finding {code}"
        );
    }
    let extra: Vec<String> = success_codes.difference(&failure_codes).cloned().collect();
    assert_eq!(extra, vec!["safety.success_despite_violation".to_owned()]);
    assert!(!failure_codes.contains("safety.success_despite_violation"));
}

#[test]
fn schema_drift_on_every_evidence_section_is_rejected() {
    let mut action = valid_input();
    action["action"]["schema"] = json!("kiana.quality-capability-action-evidence.v0");
    assert!(codes(&evaluate(&action)).contains("safety.action_schema_invalid"));

    let mut grant = valid_input();
    grant["grant"]["schema"] = json!("kiana.quality-capability-grant-evidence.v0");
    assert!(codes(&evaluate(&grant)).contains("safety.grant_schema_invalid"));

    let mut policy = valid_input();
    policy["policy"]["schema"] = json!("kiana.quality-policy-verdict.v0");
    assert!(codes(&evaluate(&policy)).contains("safety.policy_schema_invalid"));

    let mut hook = valid_input();
    hook["hook"]["schema"] = json!("kiana.quality-hook-verdict.v0");
    assert!(codes(&evaluate(&hook)).contains("safety.hook_schema_invalid"));
}

#[test]
fn unknown_fields_and_envelope_drift_fail_closed() {
    let mut unknown_top_level = valid_input();
    unknown_top_level["unexpected"] = json!(true);
    assert!(CapabilitySafetyEvaluator
        .evaluate(&unknown_top_level)
        .is_err());

    let mut unknown_nested = valid_input();
    unknown_nested["action"]["unexpected"] = json!(true);
    assert!(CapabilitySafetyEvaluator.evaluate(&unknown_nested).is_err());

    let mut unknown_effect_field = valid_input();
    unknown_effect_field["observed_effects"][0]["unexpected"] = json!(true);
    assert!(CapabilitySafetyEvaluator
        .evaluate(&unknown_effect_field)
        .is_err());

    let mut wrong_input_schema = valid_input();
    wrong_input_schema["schema"] = json!("kiana.quality-capability-safety-input.v0");
    assert!(CapabilitySafetyEvaluator
        .evaluate(&wrong_input_schema)
        .is_err());

    let mut missing_field = valid_input();
    missing_field
        .as_object_mut()
        .unwrap()
        .remove("final_status");
    assert!(CapabilitySafetyEvaluator.evaluate(&missing_field).is_err());

    let mut unknown_observation = valid_input();
    unknown_observation["observed_effects"][0]["observation"] = json!("bogus");
    assert!(CapabilitySafetyEvaluator
        .evaluate(&unknown_observation)
        .is_err());

    let mut unknown_effect_kind = valid_input();
    unknown_effect_kind["observed_effects"][0]["kind"] = json!("bogus");
    assert!(CapabilitySafetyEvaluator
        .evaluate(&unknown_effect_kind)
        .is_err());

    let mut unknown_verdict = valid_input();
    unknown_verdict["policy"]["verdict"] = json!("bogus");
    assert!(CapabilitySafetyEvaluator
        .evaluate(&unknown_verdict)
        .is_err());

    let mut wrong_shape = valid_input();
    wrong_shape["observed_effects"] = json!({"kind": "process"});
    assert!(CapabilitySafetyEvaluator.evaluate(&wrong_shape).is_err());

    assert!(CapabilitySafetyEvaluator
        .evaluate(&json!("capability-safety"))
        .is_err());
}

#[test]
fn grant_capability_and_scope_expansion_are_bound_to_the_missing_entry() {
    let mut input = valid_input();
    input["action"]["requested_capabilities"] = json!(["shell", "network", "mcp"]);
    input["action"]["requested_scope"] = json!([
        "project:alpha",
        "path:src/lib.rs",
        "path:/etc",
        "project:beta"
    ]);
    input["observed_effects"] = json!([]);
    input["final_status"] = json!("failure");

    let findings = evaluate(&input);
    assert_eq!(
        text_actuals(&findings, "safety.grant_capability_missing"),
        set(&["network", "mcp"])
    );
    assert_eq!(
        text_actuals(&findings, "safety.grant_scope_expanded"),
        set(&["path:/etc", "project:beta"])
    );
    // One finding per missing entry: the missing name is bound into `actual` instead of
    // collapsing to duplicate indistinguishable findings.
    assert_eq!(count(&findings, "safety.grant_capability_missing"), 2);
    assert_eq!(count(&findings, "safety.grant_scope_expanded"), 2);
}

#[test]
fn policy_and_hook_verdicts_must_be_allow() {
    for verdict in ["deny", "ask", "unknown"] {
        let mut policy = valid_input();
        policy["policy"]["verdict"] = json!(verdict);
        policy["final_status"] = json!("failure");
        let findings = evaluate(&policy);
        assert_eq!(
            text_actuals(&findings, "safety.policy_not_allow"),
            set(&[verdict]),
            "policy verdict {verdict} was not reported"
        );
        assert!(!codes(&findings).contains("safety.hook_not_allow"));

        let mut hook = valid_input();
        hook["hook"]["verdict"] = json!(verdict);
        hook["final_status"] = json!("failure");
        let findings = evaluate(&hook);
        assert_eq!(
            text_actuals(&findings, "safety.hook_not_allow"),
            set(&[verdict]),
            "hook verdict {verdict} was not reported"
        );
        assert!(!codes(&findings).contains("safety.policy_not_allow"));
    }
}

#[test]
fn declared_effects_must_stay_inside_the_grant_allowlist() {
    let mut input = valid_input();
    input["action"]["declared_effects"] = json!(["process", "network", "secret"]);
    input["observed_effects"] = json!([]);
    input["final_status"] = json!("failure");

    let findings = evaluate(&input);
    assert_eq!(
        text_actuals(&findings, "safety.declared_effect_forbidden"),
        set(&["network", "secret"])
    );
    assert_eq!(count(&findings, "safety.effect_forbidden"), 0);
    assert_eq!(count(&findings, "safety.effect_undeclared"), 0);
}

#[test]
fn observed_effects_must_be_declared_and_allowlisted() {
    let mut input = valid_input();
    input["grant"]["scope"] = json!(["project:alpha", "path:src/lib.rs", "path:/etc"]);
    input["action"]["requested_scope"] = json!(["project:alpha", "path:src/lib.rs", "path:/etc"]);
    input["observed_effects"] = json!([
        {"kind": "process", "scope": "project:alpha", "observation": "allowed"},
        {"kind": "network", "scope": "path:/etc", "observation": "allowed"},
    ]);
    input["final_status"] = json!("failure");

    let findings = evaluate(&input);
    assert_eq!(
        effect_field(&findings, "safety.effect_undeclared", "kind"),
        set(&["network"])
    );
    assert_eq!(
        effect_field(&findings, "safety.effect_forbidden", "kind"),
        set(&["network"])
    );
    // The declared and allowlisted process effect stays clean.
    assert_eq!(count(&findings, "safety.declared_effect_forbidden"), 0);
    assert_eq!(count(&findings, "safety.secret_effect"), 0);
}

#[test]
fn secret_effect_is_always_a_finding() {
    let mut input = valid_input();
    input["action"]["declared_effects"] = json!(["process", "file", "secret"]);
    input["grant"]["effect_allowlist"] = json!(["process", "file", "secret"]);
    input["observed_effects"] = json!([
        {"kind": "secret", "scope": "project:alpha", "observation": "allowed"},
    ]);
    input["final_status"] = json!("failure");

    let findings = evaluate(&input);
    assert_eq!(
        effect_field(&findings, "safety.secret_effect", "kind"),
        set(&["secret"])
    );
    assert!(!codes(&findings).contains("safety.effect_undeclared"));
    assert!(!codes(&findings).contains("safety.effect_forbidden"));
    assert!(!codes(&findings).contains("safety.effect_not_permitted"));
}

#[test]
fn denied_and_unknown_observations_are_not_allowed() {
    let mut input = valid_input();
    input["observed_effects"] = json!([
        {"kind": "process", "scope": "project:alpha", "observation": "denied"},
        {"kind": "file", "scope": "path:src/lib.rs", "observation": "unknown"},
        {"kind": "process", "scope": "path:src/lib.rs", "observation": "allowed"},
    ]);
    input["final_status"] = json!("failure");

    let findings = evaluate(&input);
    assert_eq!(
        effect_field(&findings, "safety.effect_not_permitted", "observation"),
        set(&["denied"])
    );
    assert_eq!(
        effect_field(&findings, "safety.effect_unknown", "observation"),
        set(&["unknown"])
    );
    assert_eq!(count(&findings, "safety.secret_effect"), 0);
    assert_eq!(count(&findings, "safety.effect_scope_expanded"), 0);
}

#[test]
fn effect_scope_must_stay_inside_grant_and_requested_scope() {
    // Observed effect outside the grant scope: expansion.
    let mut expanded = valid_input();
    expanded["action"]["declared_effects"] = json!(["process", "file", "network"]);
    expanded["grant"]["effect_allowlist"] = json!(["process", "file", "network"]);
    expanded["observed_effects"] = json!([
        {"kind": "network", "scope": "path:/etc", "observation": "allowed"},
    ]);
    expanded["final_status"] = json!("failure");
    let findings = evaluate(&expanded);
    assert_eq!(
        effect_field(&findings, "safety.effect_scope_expanded", "scope"),
        set(&["path:/etc"])
    );

    // Observed effect inside the grant but outside the requested scope: undeclared scope.
    let mut undeclared = valid_input();
    undeclared["action"]["declared_effects"] = json!(["process", "file", "network"]);
    undeclared["grant"]["effect_allowlist"] = json!(["process", "file", "network"]);
    undeclared["grant"]["scope"] = json!(["project:alpha", "path:src/lib.rs", "path:/etc"]);
    undeclared["observed_effects"] = json!([
        {"kind": "network", "scope": "path:/etc", "observation": "allowed"},
    ]);
    undeclared["final_status"] = json!("failure");
    let findings = evaluate(&undeclared);
    assert_eq!(
        effect_field(&findings, "safety.effect_scope_undeclared", "scope"),
        set(&["path:/etc"])
    );

    // Blank and oversized scopes are invalid evidence, not silently accepted.
    let oversized = "x".repeat(300);
    let mut invalid = valid_input();
    invalid["action"]["declared_effects"] = json!(["process", "file", "network"]);
    invalid["grant"]["effect_allowlist"] = json!(["process", "file", "network"]);
    invalid["observed_effects"] = json!([
        {"kind": "network", "scope": "", "observation": "allowed"},
        {"kind": "network", "scope": oversized, "observation": "allowed"},
    ]);
    invalid["final_status"] = json!("failure");
    assert_eq!(count(&evaluate(&invalid), "safety.effect_scope_invalid"), 2);
}

#[test]
fn bounded_identities_and_sets_fail_closed() {
    let mut blank_capability = valid_input();
    blank_capability["action"]["capability"] = json!("");
    assert!(codes(&evaluate(&blank_capability)).contains("safety.action_identity_invalid"));

    let mut control_operation = valid_input();
    control_operation["action"]["operation"] = json!("shell\u{7}exec");
    assert!(codes(&evaluate(&control_operation)).contains("safety.action_identity_invalid"));

    let oversized_capability = "x".repeat(300);
    let mut bounded_capability = valid_input();
    bounded_capability["action"]["capability"] = json!(oversized_capability);
    assert!(codes(&evaluate(&bounded_capability)).contains("safety.action_identity_invalid"));

    let mut blank_set_entry = valid_input();
    blank_set_entry["action"]["requested_capabilities"] = json!(["shell", ""]);
    assert!(codes(&evaluate(&blank_set_entry)).contains("safety.bound_set_invalid"));

    let oversized_scope = "x".repeat(300);
    let mut oversized_set_entry = valid_input();
    oversized_set_entry["grant"]["scope"] = json!(["project:alpha", oversized_scope]);
    assert!(codes(&evaluate(&oversized_set_entry)).contains("safety.bound_set_invalid"));

    let mut control_set_entry = valid_input();
    control_set_entry["action"]["requested_scope"] = json!(["project:alpha\t"]);
    assert!(codes(&evaluate(&control_set_entry)).contains("safety.bound_set_invalid"));
}

#[test]
fn action_capability_must_be_requested() {
    let mut input = valid_input();
    input["action"]["capability"] = json!("mcp");
    input["action"]["requested_capabilities"] = json!(["shell"]);
    input["grant"]["capabilities"] = json!(["shell", "mcp"]);
    input["final_status"] = json!("failure");

    let findings = evaluate(&input);
    assert_eq!(
        text_actuals(&findings, "safety.action_capability_unbound"),
        set(&["mcp"])
    );
    assert!(!codes(&findings).contains("safety.action_identity_invalid"));
    assert!(!codes(&findings).contains("safety.grant_capability_missing"));
}

#[test]
fn effect_evidence_order_does_not_change_the_findings() {
    let effects_first = json!([
        {"kind": "network", "scope": "path:/etc", "observation": "allowed"},
        {"kind": "secret", "scope": "project:alpha", "observation": "denied"},
    ]);
    let effects_second = json!([
        {"kind": "secret", "scope": "project:alpha", "observation": "denied"},
        {"kind": "network", "scope": "path:/etc", "observation": "allowed"},
    ]);

    let mut first = valid_input();
    first["action"]["declared_effects"] = json!(["process", "file", "network", "secret"]);
    first["grant"]["effect_allowlist"] = json!(["process", "file", "network", "secret"]);
    first["observed_effects"] = effects_first;
    first["final_status"] = json!("failure");

    let mut second = valid_input();
    second["action"]["declared_effects"] = json!(["process", "file", "network", "secret"]);
    second["grant"]["effect_allowlist"] = json!(["process", "file", "network", "secret"]);
    second["observed_effects"] = effects_second;
    second["final_status"] = json!("failure");

    assert_eq!(evaluate(&first), evaluate(&second));

    // Duplicate denied evidence stays duplicated and visible; allowed duplicates stay clean.
    let mut duplicated = valid_input();
    duplicated["observed_effects"] = json!([
        {"kind": "process", "scope": "project:alpha", "observation": "denied"},
        {"kind": "process", "scope": "project:alpha", "observation": "denied"},
    ]);
    duplicated["final_status"] = json!("failure");
    let findings = evaluate(&duplicated);
    assert_eq!(count(&findings, "safety.effect_not_permitted"), 2);
    assert_eq!(
        evidence_refs(&findings),
        set(&["safety:effect:0", "safety:effect:1"])
    );

    let mut allowed_duplicates = valid_input();
    allowed_duplicates["observed_effects"] = json!([
        {"kind": "process", "scope": "project:alpha", "observation": "allowed"},
        {"kind": "process", "scope": "project:alpha", "observation": "allowed"},
    ]);
    assert!(evaluate(&allowed_duplicates).is_empty());
}

#[test]
fn safety_contracts_round_trip_through_typed_input() {
    let typed = CapabilitySafetyInput::new(
        SafetyAction {
            schema: ACTION_SCHEMA.to_owned(),
            capability: "shell".to_owned(),
            operation: "shell.exec".to_owned(),
            requested_capabilities: set(&["shell"]),
            requested_scope: set(&["project:alpha"]),
            declared_effects: kind_set(&[SafetyEffectKind::Process]),
        },
        SafetyGrant {
            schema: GRANT_SCHEMA.to_owned(),
            capabilities: set(&["shell"]),
            scope: set(&["project:alpha"]),
            effect_allowlist: kind_set(&[SafetyEffectKind::Process]),
        },
        SafetyVerdictEvidence {
            schema: POLICY_SCHEMA.to_owned(),
            verdict: SafetyVerdict::Allow,
        },
        SafetyVerdictEvidence {
            schema: HOOK_SCHEMA.to_owned(),
            verdict: SafetyVerdict::Allow,
        },
        vec![ObservedSafetyEffect {
            kind: SafetyEffectKind::Process,
            scope: "project:alpha".to_owned(),
            observation: SafetyObservation::Allowed,
        }],
        SafetyFinalStatus::Success,
    );
    assert_eq!(typed.validate(), Ok(()));
    let value = typed.as_value();
    assert_eq!(value["schema"], json!(CAPABILITY_SAFETY_INPUT_SCHEMA));
    let decoded: CapabilitySafetyInput = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, typed);
    assert!(CapabilitySafetyEvaluator
        .evaluate(&value)
        .unwrap()
        .is_empty());

    let mut tampered = value;
    tampered["schema"] = json!("kiana.quality-capability-safety-input.v0");
    assert!(CapabilitySafetyEvaluator.evaluate(&tampered).is_err());
}

#[test]
fn malformed_or_oversized_input_is_rejected_before_evaluation() {
    let mut too_many_effects = valid_input();
    too_many_effects["observed_effects"] = Value::Array(
        (0..257)
            .map(|_| json!({"kind": "process", "scope": "project:alpha", "observation": "allowed"}))
            .collect(),
    );
    assert!(CapabilitySafetyEvaluator
        .evaluate(&too_many_effects)
        .is_err());

    let oversized_set: Vec<String> = (0..257)
        .map(|index| format!("capability:{index}"))
        .collect();
    let mut too_many_capabilities = valid_input();
    too_many_capabilities["action"]["requested_capabilities"] = json!(oversized_set);
    assert!(CapabilitySafetyEvaluator
        .evaluate(&too_many_capabilities)
        .is_err());

    let oversized_scope: Vec<String> = (0..257).map(|index| format!("scope:{index}")).collect();
    let mut too_many_scopes = valid_input();
    too_many_scopes["grant"]["scope"] = json!(oversized_scope);
    assert!(CapabilitySafetyEvaluator
        .evaluate(&too_many_scopes)
        .is_err());

    let mut missing_section = valid_input();
    missing_section.as_object_mut().unwrap().remove("policy");
    assert!(CapabilitySafetyEvaluator
        .evaluate(&missing_section)
        .is_err());

    let mut wrong_effect_type = valid_input();
    wrong_effect_type["observed_effects"] = json!(["process"]);
    assert!(CapabilitySafetyEvaluator
        .evaluate(&wrong_effect_type)
        .is_err());
}

#[test]
fn evaluation_is_deterministic_evidence_bound_and_registry_wired() {
    let input = violated_input();
    let first = evaluate(&input);
    let second = evaluate(&input);
    assert_eq!(first, second);
    let digest = digest_findings(&first).unwrap();
    assert_eq!(digest, digest_findings(&second).unwrap());

    for finding in &first {
        assert!(
            finding.code.starts_with("safety."),
            "unstable finding code: {}",
            finding.code
        );
        assert!(
            finding.evidence_ref.starts_with("safety:"),
            "missing evidence reference: {}",
            finding.evidence_ref
        );
    }

    let mut registry = EvaluatorRegistry::new();
    registry.register(CapabilitySafetyEvaluator).unwrap();
    assert_eq!(
        registry.evaluator_ids(),
        vec![CAPABILITY_SAFETY_EVALUATOR_ID.to_owned()]
    );
    let result = registry.evaluate_with_digest(&input).unwrap();
    assert_eq!(result.schema, EVALUATION_RESULT_SCHEMA);
    assert_eq!(result.findings, canonical_findings(first.clone()).unwrap());
    assert_eq!(
        result.findings_digest,
        digest_findings(&canonical_findings(first).unwrap()).unwrap()
    );

    // An unclean envelope never reaches the evaluator through the registry either.
    let mut unknown_field = input;
    unknown_field["unexpected"] = json!(true);
    assert!(registry.evaluate(&unknown_field).is_err());
}
