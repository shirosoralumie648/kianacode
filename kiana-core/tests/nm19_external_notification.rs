use kiana_core::{
    admit_external_notification, observe_external_notification_receipt,
    ExternalNotificationDisposition, ExternalNotificationReceiptObservation,
};
use kiana_domain::{
    ExternalNotificationEnvelope, ExternalNotificationPolicy, ExternalNotificationReceipt,
    ExternalNotificationReceiptStatus, ExternalNotificationTransport,
    EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA, EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA,
    EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA,
};
use serde_json::Value;

fn policy(enabled: bool) -> ExternalNotificationPolicy {
    ExternalNotificationPolicy::new(
        enabled,
        vec!["https://notify.example".to_owned()],
        enabled.then(|| "key-nm19".to_owned()),
        7,
        3,
    )
    .unwrap()
}

fn envelope() -> ExternalNotificationEnvelope {
    let mut value = ExternalNotificationEnvelope {
        schema: EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA.to_owned(),
        transport: ExternalNotificationTransport::Webhook,
        notification_id: "notification:nm19".to_owned(),
        delivery_id: "delivery:nm19".to_owned(),
        recipient_id: "principal:nm19".to_owned(),
        destination_origin: "https://notify.example".to_owned(),
        nonce: "nonce-nm19".to_owned(),
        created_at_unix_ms: 100,
        expires_at_unix_ms: 1_000,
        authority_epoch: 7,
        data_epoch: 3,
        payload_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        signature_digest: "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            .to_owned(),
        source_event_ids: vec!["event-nm19".to_owned()],
        envelope_digest: String::new(),
    };
    value.envelope_digest = value.digest();
    value
}

fn receipt(status: ExternalNotificationReceiptStatus) -> ExternalNotificationReceipt {
    let mut value = ExternalNotificationReceipt {
        schema: EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA.to_owned(),
        transport: ExternalNotificationTransport::Webhook,
        notification_id: "notification:nm19".to_owned(),
        delivery_id: "delivery:nm19".to_owned(),
        status,
        provider_receipt_ref: Some("provider-receipt:nm19".to_owned()),
        payload_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        nonce: "nonce-nm19".to_owned(),
        authority_epoch: 7,
        observed_at_unix_ms: 200,
        receipt_digest: String::new(),
    };
    value.receipt_digest = value.digest();
    value
}

#[test]
fn external_notification_is_default_off_and_never_direct_effect() {
    let result =
        admit_external_notification(&policy(false), &envelope(), 200, Some("nonce-nm19"), 7, 3)
            .unwrap();
    assert_eq!(result.schema, EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA);
    assert_eq!(
        result.disposition,
        ExternalNotificationDisposition::NotSupported
    );
    assert!(result.control_plane_required);
    assert!(!result.direct_effect);
}

#[test]
fn enabled_policy_still_returns_explicit_connector_handoff_only() {
    let result =
        admit_external_notification(&policy(true), &envelope(), 200, Some("nonce-nm19"), 7, 3)
            .unwrap();
    assert_eq!(
        result.disposition,
        ExternalNotificationDisposition::ReadyForExplicitConnector
    );
    assert!(result.control_plane_required);
    assert!(!result.direct_effect);
}

#[test]
fn origin_nonce_epoch_and_expiry_drift_fail_closed() {
    let mut wrong_nonce = envelope();
    assert!(
        admit_external_notification(&policy(true), &wrong_nonce, 200, Some("other"), 7, 3).is_err()
    );
    wrong_nonce.destination_origin = "https://other.example".to_owned();
    wrong_nonce.envelope_digest = wrong_nonce.digest();
    assert!(admit_external_notification(
        &policy(true),
        &wrong_nonce,
        200,
        Some("nonce-nm19"),
        7,
        3
    )
    .is_err());
    let mut expired = envelope();
    expired.expires_at_unix_ms = 150;
    expired.envelope_digest = expired.digest();
    assert!(
        admit_external_notification(&policy(true), &expired, 200, Some("nonce-nm19"), 7, 3)
            .is_err()
    );
}

#[test]
fn receipts_are_observed_only_and_unknown_requires_reconcile() {
    assert_eq!(
        observe_external_notification_receipt(
            &envelope(),
            &receipt(ExternalNotificationReceiptStatus::Acknowledged)
        )
        .unwrap(),
        ExternalNotificationReceiptObservation::Acknowledged
    );
    assert_eq!(
        observe_external_notification_receipt(
            &envelope(),
            &receipt(ExternalNotificationReceiptStatus::Unknown)
        )
        .unwrap(),
        ExternalNotificationReceiptObservation::ReconcileRequired
    );
    let mut mismatched = receipt(ExternalNotificationReceiptStatus::Acknowledged);
    mismatched.delivery_id = "delivery:other".to_owned();
    mismatched.receipt_digest = mismatched.digest();
    assert!(observe_external_notification_receipt(&envelope(), &mismatched).is_err());
}

#[test]
fn fixture_captures_default_off_external_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/nm19-external-notification.json"))
            .expect("valid NM-19 fixture");
    assert_eq!(
        fixture["schema"],
        "kiana.external-notification-admission.v1"
    );
    assert_eq!(fixture["policy"], "default_off");
    for denial in [
        "disabled_by_default",
        "nonce_replay_or_mismatch",
        "direct_network_effect",
    ] {
        assert!(fixture["deny_first"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == denial));
    }
}
