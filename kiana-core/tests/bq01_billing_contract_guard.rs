#[test]
fn billing_contracts_have_stable_ids_schema_states_and_errors() {
    let billing = include_str!("../../kiana-domain/src/billing_contracts.rs");
    let ids = include_str!("../../kiana-domain/src/ids.rs");
    let contracts = include_str!("../../kiana-domain/src/contracts.rs");
    let baseline = include_str!("../../docs/roadmap/billing-quota-cost-baseline.md");
    for marker in [
        "BILLING_SCHEMA_VERSION",
        "BillingUnknownReason",
        "BillingState",
        "BillingErrorCode",
        "BillingContractHeader",
        "ProviderReceiptRef",
        "BILLING_CONTRACT_SCHEMA",
        "transition",
        "parse",
    ] {
        assert!(
            billing.contains(marker),
            "billing contract marker missing: {marker}"
        );
    }
    for marker in [
        "uuid_id!(UsageId)",
        "uuid_id!(ReservationId)",
        "uuid_id!(LedgerEntryId)",
        "uuid_id!(RateCardId)",
        "uuid_id!(CostCorrectionId)",
        "uuid_id!(QuotaReservationId)",
    ] {
        assert!(ids.contains(marker), "billing ID marker missing: {marker}");
    }
    assert!(contracts.contains("pub struct SchemaVersion"));
    assert!(baseline.contains("BQ-02"));
    assert!(!billing.contains("reqwest"));
    assert!(!billing.contains("tokio"));
    assert!(!billing.contains("std::fs"));
}
