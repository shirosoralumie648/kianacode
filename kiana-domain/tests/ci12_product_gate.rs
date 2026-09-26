use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn case(scenario: Ci12Scenario, disposition: Ci12Disposition) -> Ci12Case {
    let denied = matches!(disposition, Ci12Disposition::Denied);
    let unknown = matches!(disposition, Ci12Disposition::Unknown);
    let fake = scenario == Ci12Scenario::FakeProviderSuccess;
    let live = scenario == Ci12Scenario::LiveOptIn;
    let mut value = Ci12Case {
        schema: CI12_PRODUCT_GATE_SCHEMA.to_owned(),
        scenario,
        disposition,
        handler_calls: u32::from(!denied),
        provider_calls: u32::from(fake || live),
        effect_count: u32::from(fake || unknown),
        secret_free: true,
        same_spine: true,
        receipt_bound: fake || unknown,
        operator_approval_ref: live.then(|| "approval:ci12".to_owned()),
        live_provider_evidence: live.then(|| hash('e')),
        limitations: if denied || live {
            vec!["CI-only proof".to_owned()]
        } else {
            vec!["result unknown requires reconciliation".to_owned()]
        },
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn gate() -> Ci12ProductGate {
    let scenarios = [
        Ci12Scenario::MissingAuthentication,
        Ci12Scenario::UntrustedProject,
        Ci12Scenario::CrossProjectScope,
        Ci12Scenario::ExpiredApproval,
        Ci12Scenario::SecretLeak,
        Ci12Scenario::ToctouDrift,
        Ci12Scenario::Replay,
        Ci12Scenario::ResultUnknown,
        Ci12Scenario::FakeProviderSuccess,
        Ci12Scenario::LiveOptIn,
    ];
    Ci12ProductGate::new(
        12,
        scenarios
            .into_iter()
            .map(|scenario| {
                let disposition = match scenario {
                    Ci12Scenario::ResultUnknown => Ci12Disposition::Unknown,
                    Ci12Scenario::FakeProviderSuccess => Ci12Disposition::Completed,
                    Ci12Scenario::LiveOptIn => Ci12Disposition::Unverified,
                    _ => Ci12Disposition::Denied,
                };
                case(scenario, disposition)
            })
            .collect(),
    )
}

#[test]
fn ci12_matrix_covers_deny_first_fake_success_and_live_opt_in_ceiling() {
    let gate = gate();
    gate.validate().expect("CI-12 gate");
    assert!(gate
        .cases
        .iter()
        .filter(|case| case.disposition == Ci12Disposition::Denied)
        .all(|case| case.effect_count == 0 && case.provider_calls == 0));
}

#[test]
fn forged_deny_effect_unknown_and_live_claims_fail_closed() {
    let mut gate = gate();
    gate.cases[0].effect_count = 1;
    gate.cases[0].digest = gate.cases[0].canonical_digest();
    gate.digest = gate.canonical_digest();
    assert_eq!(
        gate.validate().unwrap_err(),
        "ci12_deny_case_effect_invalid"
    );

    let mut unknown = gate();
    let item = unknown
        .cases
        .iter_mut()
        .find(|case| case.scenario == Ci12Scenario::ResultUnknown)
        .unwrap();
    item.receipt_bound = false;
    item.digest = item.canonical_digest();
    unknown.digest = unknown.canonical_digest();
    assert_eq!(
        unknown.validate().unwrap_err(),
        "ci12_unknown_case_reconcile_invalid"
    );

    let mut live = gate();
    let item = live
        .cases
        .iter_mut()
        .find(|case| case.scenario == Ci12Scenario::LiveOptIn)
        .unwrap();
    item.operator_approval_ref = None;
    item.disposition = Ci12Disposition::Completed;
    item.digest = item.canonical_digest();
    live.digest = live.canonical_digest();
    assert_eq!(
        live.validate().unwrap_err(),
        "ci12_live_opt_in_proof_invalid"
    );
}

#[test]
fn duplicate_scenario_and_secret_text_are_rejected() {
    let mut duplicate = gate();
    duplicate.cases.push(duplicate.cases[0].clone());
    duplicate.cases.pop();
    duplicate.cases[1].scenario = duplicate.cases[0].scenario;
    duplicate.cases[1].digest = duplicate.cases[1].canonical_digest();
    duplicate.digest = duplicate.canonical_digest();
    assert_eq!(duplicate.validate().unwrap_err(), "ci12_scenario_duplicate");

    let mut secret = gate();
    secret.cases[0].secret_free = false;
    secret.cases[0].digest = secret.cases[0].canonical_digest();
    secret.digest = secret.canonical_digest();
    assert_eq!(
        secret.validate().unwrap_err(),
        "ci12_case_header_or_spine_invalid"
    );
}
