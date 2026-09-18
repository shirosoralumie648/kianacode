use kiana_domain::{
    BillingContractHeader, BillingErrorCode, BillingState, BillingUnknownReason,
    ProviderReceiptRef, RateCardId, ReservationId, UsageId,
};

#[test]
fn billing_ids_schema_and_unknown_reason_are_stable() {
    let usage = UsageId::new();
    let reservation = ReservationId::new();
    assert_ne!(usage.as_uuid(), reservation.as_uuid());
    assert_ne!(RateCardId::new().as_uuid(), usage.as_uuid());

    let known = BillingContractHeader::new(BillingState::Settled, None);
    known.validate().unwrap();
    let unknown = BillingContractHeader::new(
        BillingState::Unknown,
        Some(BillingUnknownReason::ProviderUnreported),
    );
    unknown.validate().unwrap();
    assert!(BillingUnknownReason::ProviderUnreported.requires_reconciliation());
    assert_eq!(BillingUnknownReason::Absent.as_str(), "absent");
    assert_eq!(
        BillingErrorCode::parse("not-a-code"),
        BillingErrorCode::Unsupported
    );
}

#[test]
fn billing_state_transitions_and_header_schema_fail_closed() {
    assert_eq!(
        BillingState::Reserved
            .transition(BillingState::Observed)
            .unwrap(),
        BillingState::Observed
    );
    assert_eq!(
        BillingState::Observed
            .transition(BillingState::Unknown)
            .unwrap(),
        BillingState::Unknown
    );
    assert_eq!(
        BillingState::Reserved
            .transition(BillingState::Settled)
            .unwrap_err(),
        "billing_state_transition_invalid"
    );

    let mut bad = BillingContractHeader::new(BillingState::Unknown, None);
    assert_eq!(
        bad.validate().unwrap_err(),
        "billing_contract_header_invalid"
    );
    bad = BillingContractHeader::new(BillingState::Settled, None);
    bad.version.major = 2;
    assert_eq!(
        bad.validate().unwrap_err(),
        "billing_contract_header_invalid"
    );
    assert!(serde_json::from_value::<BillingContractHeader>(serde_json::json!({
        "schema": "kiana.billing-contract.v1",
        "version": {"major": 1, "minor": 0},
        "state": "settled",
        "contract_digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000",
        "unexpected": true
    }))
    .is_err());
}

#[test]
fn provider_receipt_ref_is_opaque_and_secret_free() {
    assert_eq!(
        ProviderReceiptRef::new("provider-receipt:abc")
            .unwrap()
            .as_str(),
        "provider-receipt:abc"
    );
    assert!(ProviderReceiptRef::new("provider-receipt:secret-value").is_err());
    assert!(ProviderReceiptRef::new("\n").is_err());
}
