#[test]
fn delivery_authorization_keeps_manifest_broker_and_recipient_boundaries() {
    let authorization = include_str!("../../kiana-domain/src/delivery_authorization.rs");
    let manifest = include_str!("../../kiana-domain/src/delivery_manifest.rs");
    let closeout = include_str!("../../kiana-domain/src/company_closeout.rs");
    let core = include_str!("../src/delivery_authorization.rs");
    for marker in [
        "DELIVERY_AUTHORIZATION_SCHEMA",
        "DeliveryAuthorization",
        "DeliveryDispatchIntent",
        "DeliveryEffectReceipt",
        "DeliveryRecipientConfirmation",
        "DeliveryReceiptSource",
        "DeliveryEffectOutcome",
        "delivery_unknown_dispatch_fenced",
        "delivery_effect_receipt_binding_invalid",
        "delivery_confirmation_binding_invalid",
        "DeliveryManifest",
        "DispatchDelivery",
        "ConfirmDelivery",
        "recipient",
        "Broker",
        "authorize_delivery",
        "record_delivery_effect",
    ] {
        assert!(
            authorization.contains(marker)
                || manifest.contains(marker)
                || closeout.contains(marker)
                || core.contains(marker),
            "CO-34 marker missing: {marker}"
        );
    }
    for forbidden in [
        "automatic_retry_unknown",
        "retry_unknown_effect",
        "ModelClient::new",
        "Command::new",
    ] {
        assert!(
            !authorization.contains(forbidden),
            "CO-34 bypass marker present: {forbidden}"
        );
    }
}
