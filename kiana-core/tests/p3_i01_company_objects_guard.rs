#[test]
fn company_objects_are_domain_contracts_with_explicit_invariants() {
    let company = include_str!("../../kiana-domain/src/company.rs");
    let core = include_str!("../src/company.rs");
    for marker in [
        "pub struct Objective",
        "pub struct Initiative",
        "pub struct Project",
        "pub struct Milestone",
        "pub struct Acceptance",
        "pub struct Delivery",
        "pub struct Outcome",
        "pub struct ChangeRequest",
        "pub struct Risk",
        "pub struct Incident",
        "states!(ObjectiveStatus",
        "states!(ProjectStatus",
        "states!(AcceptanceStatus",
        "states!(DeliveryStatus",
        "states!(OutcomeStatus",
        "states!(IncidentStatus",
        "impl Acceptance",
        "impl CompanyReview",
        "impl MetricObservation",
        "fn validate(&self)",
        "serde(deny_unknown_fields)",
    ] {
        assert!(
            company.contains(marker),
            "company contract marker missing: {marker}"
        );
    }
    for marker in [
        "load_company",
        "handle_company_command",
        "company_packet_not_found",
        "company_revision_conflict",
    ] {
        assert!(
            core.contains(marker),
            "core company boundary marker missing: {marker}"
        );
    }
}
