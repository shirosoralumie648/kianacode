use kiana_domain::{
    ConnectorConformanceAdapter, ConnectorConformanceCase, ConnectorConformanceReport,
    ConnectorConformanceScenario, ConnectorConformanceStatus, CONNECTOR_CONFORMANCE_SCHEMA,
};
use serde_json::json;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn case(
    adapter: ConnectorConformanceAdapter,
    scenario: ConnectorConformanceScenario,
    status: ConnectorConformanceStatus,
) -> ConnectorConformanceCase {
    let verified = status == ConnectorConformanceStatus::Verified;
    let mut value = ConnectorConformanceCase {
        adapter,
        scenario,
        status,
        effect_count: if scenario == ConnectorConformanceScenario::Replay {
            1
        } else {
            0
        },
        unknown_preserved: scenario != ConnectorConformanceScenario::Unknown || verified,
        dedupe_verified: scenario != ConnectorConformanceScenario::Replay || verified,
        scope_fenced: scenario != ConnectorConformanceScenario::Toctou || verified,
        evidence_digest: verified.then_some(D.to_owned()),
        reason: (!verified).then_some("fixture not implemented".to_owned()),
        case_digest: String::new(),
    };
    value.case_digest = kiana_domain::json_digest(&json!({
        "adapter": value.adapter,
        "scenario": value.scenario,
        "status": value.status,
        "effect_count": value.effect_count,
        "unknown_preserved": value.unknown_preserved,
        "dedupe_verified": value.dedupe_verified,
        "scope_fenced": value.scope_fenced,
        "evidence_digest": value.evidence_digest,
        "reason": value.reason,
    }));
    value
}

#[test]
fn shared_adapters_cover_deny_replay_unknown_toctou_and_success() {
    let cases = vec![
        case(
            ConnectorConformanceAdapter::LocalFixture,
            ConnectorConformanceScenario::Deny,
            ConnectorConformanceStatus::Verified,
        ),
        case(
            ConnectorConformanceAdapter::StdioMcpFake,
            ConnectorConformanceScenario::Replay,
            ConnectorConformanceStatus::Verified,
        ),
        case(
            ConnectorConformanceAdapter::HttpFake,
            ConnectorConformanceScenario::Unknown,
            ConnectorConformanceStatus::Verified,
        ),
        case(
            ConnectorConformanceAdapter::HttpFake,
            ConnectorConformanceScenario::Toctou,
            ConnectorConformanceStatus::Verified,
        ),
        case(
            ConnectorConformanceAdapter::LocalFixture,
            ConnectorConformanceScenario::Success,
            ConnectorConformanceStatus::Verified,
        ),
    ];
    let report = ConnectorConformanceReport::evaluate(cases).unwrap();
    assert_eq!(report.schema, CONNECTOR_CONFORMANCE_SCHEMA);
    assert_eq!(report.status, ConnectorConformanceStatus::Verified);
    report.validate().unwrap();
}

#[test]
fn unknown_effect_and_duplicate_or_forged_report_fail_closed() {
    let mut unknown = case(
        ConnectorConformanceAdapter::LocalFixture,
        ConnectorConformanceScenario::Unknown,
        ConnectorConformanceStatus::Verified,
    );
    unknown.unknown_preserved = false;
    unknown.case_digest = kiana_domain::json_digest(&json!({
        "adapter": unknown.adapter,
        "scenario": unknown.scenario,
        "status": unknown.status,
        "effect_count": unknown.effect_count,
        "unknown_preserved": unknown.unknown_preserved,
        "dedupe_verified": unknown.dedupe_verified,
        "scope_fenced": unknown.scope_fenced,
        "evidence_digest": unknown.evidence_digest,
        "reason": unknown.reason,
    }));
    assert_eq!(
        unknown.validate().unwrap_err(),
        "connector_conformance_unknown_not_preserved"
    );

    let one = case(
        ConnectorConformanceAdapter::LocalFixture,
        ConnectorConformanceScenario::Deny,
        ConnectorConformanceStatus::Verified,
    );
    assert_eq!(
        ConnectorConformanceReport::evaluate(vec![one.clone(), one]).unwrap_err(),
        "connector_conformance_case_duplicate"
    );

    let partial = ConnectorConformanceReport::evaluate(vec![case(
        ConnectorConformanceAdapter::HttpFake,
        ConnectorConformanceScenario::Schema,
        ConnectorConformanceStatus::NotImplemented,
    )])
    .unwrap();
    assert_eq!(partial.status, ConnectorConformanceStatus::NotImplemented);
    let mut forged = partial.clone();
    forged.verified_count = 1;
    assert_eq!(
        forged.validate().unwrap_err(),
        "connector_conformance_counts_mismatch"
    );
}
