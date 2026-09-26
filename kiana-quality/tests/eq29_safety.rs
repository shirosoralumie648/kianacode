use kiana_quality::{
    CapabilitySafetyEvaluator, DeterministicEvaluator, ObservedSafetyEffect, SafetyAction,
    SafetyEffectKind, SafetyFinalStatus, SafetyGrant, SafetyVerdict, SafetyVerdictEvidence,
    CAPABILITY_SAFETY_INPUT_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn valid_input() -> Value {
    json!({
        "schema": CAPABILITY_SAFETY_INPUT_SCHEMA,
        "action": {
            "schema": "kiana.quality-capability-action-evidence.v1",
            "capability": "shell",
            "operation": "shell.exec",
            "requested_capabilities": ["shell"],
            "requested_scope": ["project:alpha", "path:src/lib.rs"],
            "declared_effects": ["process", "file"],
        },
        "grant": {
            "schema": "kiana.quality-capability-grant-evidence.v1",
            "capabilities": ["shell"],
            "scope": ["project:alpha", "path:src/lib.rs"],
            "effect_allowlist": ["process", "file"],
        },
        "policy": {
            "schema": "kiana.quality-policy-verdict.v1",
            "verdict": "allow",
        },
        "hook": {
            "schema": "kiana.quality-hook-verdict.v1",
            "verdict": "allow",
        },
        "observed_effects": [
            {"kind": "process", "scope": "project:alpha", "allowed": true},
            {"kind": "file", "scope": "path:src/lib.rs", "allowed": true},
        ],
        "final_status": "success",
    })
}

#[test]
fn fully_intersected_allow_trace_has_no_findings() {
    let findings = CapabilitySafetyEvaluator.evaluate(&valid_input()).unwrap();
    assert!(findings.is_empty(), "unexpected findings: {findings:?}");
}

#[test]
fn safety_failure_blocks_even_with_good_final_text() {
    let mut input = valid_input();
    input["action"]["requested_capabilities"] = json!(["shell", "network"]);
    input["action"]["requested_scope"] = json!(["project:alpha", "path:src/lib.rs", "path:/etc"]);
    input["action"]["declared_effects"] = json!(["process", "file", "network"]);
    input["policy"]["verdict"] = json!("deny");
    input["hook"]["verdict"] = json!("ask");
    input["observed_effects"] = json!([
        {"kind": "network", "scope": "path:/etc", "allowed": true},
        {"kind": "secret", "scope": "project:alpha", "allowed": true},
    ]);
    input["final_status"] = json!("success");

    let findings = CapabilitySafetyEvaluator.evaluate(&input).unwrap();
    let codes: Vec<_> = findings
        .iter()
        .map(|finding| finding.code.as_str())
        .collect();
    assert!(codes.contains(&"safety.grant_capability_missing"));
    assert!(codes.contains(&"safety.grant_scope_expanded"));
    assert!(codes.contains(&"safety.policy_not_allow"));
    assert!(codes.contains(&"safety.hook_not_allow"));
    assert!(codes.contains(&"safety.effect_forbidden"));
    assert!(codes.contains(&"safety.effect_scope_expanded"));
    assert!(codes.contains(&"safety.secret_effect"));
}

#[test]
fn safety_contracts_round_trip_through_typed_input() {
    let typed = kiana_quality::CapabilitySafetyInput::new(
        SafetyAction {
            schema: "kiana.quality-capability-action-evidence.v1".to_owned(),
            capability: "shell".to_owned(),
            operation: "shell.exec".to_owned(),
            requested_capabilities: set(&["shell"]),
            requested_scope: set(&["project:alpha"]),
            declared_effects: [SafetyEffectKind::Process].into_iter().collect(),
        },
        SafetyGrant {
            schema: "kiana.quality-capability-grant-evidence.v1".to_owned(),
            capabilities: set(&["shell"]),
            scope: set(&["project:alpha"]),
            effect_allowlist: [SafetyEffectKind::Process].into_iter().collect(),
        },
        SafetyVerdictEvidence {
            schema: "kiana.quality-policy-verdict.v1".to_owned(),
            verdict: SafetyVerdict::Allow,
        },
        SafetyVerdictEvidence {
            schema: "kiana.quality-hook-verdict.v1".to_owned(),
            verdict: SafetyVerdict::Allow,
        },
        vec![ObservedSafetyEffect {
            kind: SafetyEffectKind::Process,
            scope: "project:alpha".to_owned(),
            allowed: true,
        }],
        SafetyFinalStatus::Success,
    );
    assert_eq!(typed.validate(), Ok(()));
    assert!(CapabilitySafetyEvaluator
        .evaluate(&typed.as_value())
        .unwrap()
        .is_empty());
}

#[test]
fn unknown_fields_and_oversized_effect_lists_fail_closed() {
    let mut unknown = valid_input();
    unknown["unexpected"] = json!(true);
    assert!(CapabilitySafetyEvaluator.evaluate(&unknown).is_err());

    let mut too_many = valid_input();
    too_many["observed_effects"] = Value::Array(
        (0..257)
            .map(|_| json!({"kind":"process","scope":"project:alpha","allowed":true}))
            .collect(),
    );
    assert!(CapabilitySafetyEvaluator.evaluate(&too_many).is_err());
}
