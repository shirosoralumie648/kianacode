#[test]
fn core_and_runner_use_domain_state_transition_contracts() {
    let states = include_str!("../../kiana-domain/src/states.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/lifecycle.rs");
    let dispatch = include_str!("../src/dispatch.rs");
    let runner = include_str!("../../kiana-runner/src/state_driver.rs");
    for marker in [
        "can_transition_to",
        "transition_via",
        "is_terminal",
        "ResultUnknown",
        "InvalidStateTransition",
    ] {
        assert!(
            states.contains(marker),
            "state contract marker missing: {marker}"
        );
    }
    assert!(company.contains("company_illegal_state_transition"));
    assert!(core.contains("ExecutionStatus::ResultUnknown"));
    assert!(dispatch.contains("result_unknown"));
    assert!(runner.contains("DriverTransition"));
}
