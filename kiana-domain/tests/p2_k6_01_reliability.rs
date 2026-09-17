use kiana_domain::FailureClass;

#[test]
fn every_failure_class_has_an_incident_and_recovery() {
    let classes = [
        FailureClass::Crash,
        FailureClass::Timeout,
        FailureClass::Cancel,
        FailureClass::DiskFull,
        FailureClass::McpFailure,
        FailureClass::ProviderUnknown,
    ];

    assert_eq!(classes.len(), 6);
    for class in classes {
        let known = class.recovery(false);
        assert!(!known.steps.is_empty());
        assert!(!known.requires_reconciliation);
        assert!(!known.automatic_retry_allowed);

        let unknown = class.recovery(true);
        assert!(!unknown.steps.is_empty());
        assert!(unknown.requires_reconciliation);
        assert!(!unknown.automatic_retry_allowed);
    }
}
