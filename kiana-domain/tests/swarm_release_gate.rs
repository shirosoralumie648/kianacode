use kiana_domain::{
    SwarmReleaseCase, SwarmReleaseGate, SwarmReleaseScenario, SwarmReleaseStatus,
    SWARM_RELEASE_GATE_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn case(scenario: SwarmReleaseScenario) -> SwarmReleaseCase {
    let mut value = SwarmReleaseCase {
        scenario,
        status: SwarmReleaseStatus::Verified,
        handler_calls: 0,
        effect_count: 0,
        unknown_preserved: true,
        replay_fenced: true,
        same_spine: true,
        secret_free: true,
        evidence_digest: Some(D.to_owned()),
        limitation: "CI fixture only".to_owned(),
        case_digest: String::new(),
    };
    value.case_digest = kiana_domain::json_digest(&json!({
        "scenario": value.scenario,
        "status": value.status,
        "handler_calls": value.handler_calls,
        "effect_count": value.effect_count,
        "unknown_preserved": value.unknown_preserved,
        "replay_fenced": value.replay_fenced,
        "same_spine": value.same_spine,
        "secret_free": value.secret_free,
        "evidence_digest": value.evidence_digest,
        "limitation": value.limitation,
    }));
    value
}

fn gate() -> SwarmReleaseGate {
    let mut value = SwarmReleaseGate {
        schema: SWARM_RELEASE_GATE_SCHEMA.to_owned(),
        cases: vec![
            case(SwarmReleaseScenario::Deny),
            case(SwarmReleaseScenario::Unknown),
            case(SwarmReleaseScenario::Replay),
            case(SwarmReleaseScenario::Crash),
            case(SwarmReleaseScenario::Race),
            case(SwarmReleaseScenario::FakeGolden),
        ],
        status: SwarmReleaseStatus::Verified,
        gate_digest: String::new(),
    };
    value.gate_digest = kiana_domain::json_digest(&json!({
        "schema": value.schema,
        "cases": value.cases,
        "status": value.status,
    }));
    value
}

#[test]
fn full_deny_unknown_replay_crash_race_and_fake_golden_gate_validates() {
    gate().validate().unwrap();
}

#[test]
fn effectful_deny_and_missing_unknown_or_replay_fence_fail_closed() {
    let mut gate = gate();
    gate.cases[0].effect_count = 1;
    gate.cases[0].case_digest = kiana_domain::json_digest(&json!({
        "scenario": gate.cases[0].scenario,
        "status": gate.cases[0].status,
        "handler_calls": gate.cases[0].handler_calls,
        "effect_count": gate.cases[0].effect_count,
        "unknown_preserved": gate.cases[0].unknown_preserved,
        "replay_fenced": gate.cases[0].replay_fenced,
        "same_spine": gate.cases[0].same_spine,
        "secret_free": gate.cases[0].secret_free,
        "evidence_digest": gate.cases[0].evidence_digest,
        "limitation": gate.cases[0].limitation,
    }));
    gate.gate_digest = kiana_domain::json_digest(&json!({
        "schema": gate.schema,
        "cases": gate.cases,
        "status": gate.status,
    }));
    assert_eq!(
        gate.validate().unwrap_err(),
        "swarm_release_deny_effect_invalid"
    );

    let mut unknown = gate();
    unknown.cases[1].unknown_preserved = false;
    unknown.cases[1].case_digest = kiana_domain::json_digest(&json!({
        "scenario": unknown.cases[1].scenario,
        "status": unknown.cases[1].status,
        "handler_calls": unknown.cases[1].handler_calls,
        "effect_count": unknown.cases[1].effect_count,
        "unknown_preserved": unknown.cases[1].unknown_preserved,
        "replay_fenced": unknown.cases[1].replay_fenced,
        "same_spine": unknown.cases[1].same_spine,
        "secret_free": unknown.cases[1].secret_free,
        "evidence_digest": unknown.cases[1].evidence_digest,
        "limitation": unknown.cases[1].limitation,
    }));
    unknown.gate_digest = kiana_domain::json_digest(&json!({
        "schema": unknown.schema,
        "cases": unknown.cases,
        "status": unknown.status,
    }));
    assert_eq!(
        unknown.validate().unwrap_err(),
        "swarm_release_unknown_invalid"
    );
}
