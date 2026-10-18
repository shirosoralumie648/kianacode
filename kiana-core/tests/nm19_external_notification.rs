//! NM-19 deny-first fixtures for the external notification admission gate.
//!
//! Every refusal names its stable code and is asserted before the acceptance fixtures. The gate
//! stays a pure contract: no transport, no signer, no direct effect.

use kiana_core::{
    check_external_notification_signature, observe_external_notification_receipt,
    ExternalNotificationAdmissionContext, ExternalNotificationDisposition,
    ExternalNotificationPolicyRegistry, ExternalNotificationReceiptObservation,
    ExternalNotificationSignatureVerification, EXTERNAL_NOTIFICATION_REGISTRY_SCHEMA,
};
use kiana_domain::{
    ExternalNotificationEnvelope, ExternalNotificationPolicy, ExternalNotificationPolicySnapshot,
    ExternalNotificationReceipt, ExternalNotificationReceiptStatus,
    ExternalNotificationReplayStore, ExternalNotificationReplayWindow,
    ExternalNotificationTransport, EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA,
    EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA, EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA,
};
use serde_json::Value;

fn hex(nibble: char) -> String {
    format!("sha256:{}", nibble.to_string().repeat(64))
}

fn enabled_policy() -> ExternalNotificationPolicy {
    ExternalNotificationPolicy::new(
        true,
        vec!["https://notify.example".to_owned()],
        Some("key-nm19".to_owned()),
        7,
        3,
    )
    .unwrap()
}

fn registry(enabled: bool) -> ExternalNotificationPolicyRegistry {
    let snapshot = if enabled {
        ExternalNotificationPolicySnapshot::issue(enabled_policy(), 2, 100).unwrap()
    } else {
        ExternalNotificationPolicySnapshot::disabled(7, 3, 100).unwrap()
    };
    ExternalNotificationPolicyRegistry::install(snapshot).unwrap()
}

fn context(
    registry: &ExternalNotificationPolicyRegistry,
    now_unix_ms: u64,
) -> ExternalNotificationAdmissionContext {
    ExternalNotificationAdmissionContext::new(
        registry.policy().policy_digest.clone(),
        7,
        3,
        now_unix_ms,
    )
}

fn replay() -> ExternalNotificationReplayStore {
    ExternalNotificationReplayStore::new(ExternalNotificationReplayWindow::new(16, 10_000).unwrap())
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
        payload_digest: hex('a'),
        signature_digest: hex('b'),
        signing_key_id: Some("key-nm19".to_owned()),
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
        payload_digest: hex('a'),
        nonce: "nonce-nm19".to_owned(),
        authority_epoch: 7,
        observed_at_unix_ms: 200,
        receipt_digest: String::new(),
    };
    value.receipt_digest = value.digest();
    value
}

// ---------------------------------------------------------------------------------------------
// Deny first.
// ---------------------------------------------------------------------------------------------

#[test]
fn a_caller_cannot_fabricate_an_enabled_policy() {
    // The server holds the explicit disabled default. The caller offers a self-consistent,
    // digest-valid enabled policy and an envelope that only matches that fabricated policy.
    let server = registry(false);
    let fake = enabled_policy();
    let fake_context =
        ExternalNotificationAdmissionContext::new(fake.policy_digest.clone(), 7, 3, 200);
    let result = server.admit(&envelope(), &fake_context, &mut replay());
    assert_eq!(
        result.unwrap_err(),
        "external_notification_policy_not_pinned"
    );

    // Any other digest echo (including a tampered one) is refused the same way.
    let mut tampered = context(&server, 200);
    tampered.policy_digest = hex('f');
    assert_eq!(
        server
            .admit(&envelope(), &tampered, &mut replay())
            .unwrap_err(),
        "external_notification_policy_not_pinned"
    );
}

#[test]
fn a_caller_cannot_publish_a_policy_revision_out_of_order() {
    let mut server = registry(false);
    let first = server.revision();
    let enabled = ExternalNotificationPolicySnapshot::issue(enabled_policy(), 2, 100).unwrap();
    server.publish(enabled.clone()).unwrap();
    assert!(server.revision() > first);
    assert!(server.policy().enabled);

    // Rollback (revision 1 after 2) is refused.
    let rollback = ExternalNotificationPolicySnapshot::disabled(7, 3, 100).unwrap();
    assert_eq!(
        server.publish(rollback).unwrap_err(),
        "external_notification_policy_revision_rollback"
    );
    // A new revision carrying the previous policy digest is a replay, not an upgrade.
    let replay_snapshot =
        ExternalNotificationPolicySnapshot::issue(enabled_policy(), 3, 200).unwrap();
    assert_eq!(
        server.publish(replay_snapshot).unwrap_err(),
        "external_notification_policy_replay"
    );
    assert!(server.policy().enabled);
}

#[test]
fn a_disabled_server_returns_not_supported_and_never_direct_effect() {
    let server = registry(false);
    let result = server
        .admit(&envelope(), &context(&server, 200), &mut replay())
        .unwrap();
    assert_eq!(result.schema, EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA);
    assert_eq!(
        result.disposition,
        ExternalNotificationDisposition::NotSupported
    );
    assert_eq!(result.reason, "external_notification_disabled");
    assert!(result.control_plane_required);
    assert!(!result.direct_effect);
    assert_eq!(result.policy_revision, 1);
}

#[test]
fn nonce_replay_delivery_replay_and_expiry_fail_closed() {
    let server = registry(true);
    let mut store = replay();

    // First admission is the only one that may succeed; a replay of the same identifiers is
    // refused in both dimensions.
    server
        .admit(&envelope(), &context(&server, 200), &mut store)
        .unwrap();
    assert_eq!(
        server
            .admit(&envelope(), &context(&server, 200), &mut store)
            .unwrap_err(),
        "external_notification_nonce_replayed"
    );

    let mut same_delivery = envelope();
    same_delivery.nonce = "nonce-other".to_owned();
    same_delivery.envelope_digest = same_delivery.digest();
    assert_eq!(
        server
            .admit(&same_delivery, &context(&server, 200), &mut store)
            .unwrap_err(),
        "external_notification_delivery_replayed"
    );

    // Expired, not-yet-valid and epoch-drifted envelopes are refused before the replay store.
    let mut store = replay();
    let mut expired = envelope();
    expired.expires_at_unix_ms = 150;
    expired.envelope_digest = expired.digest();
    assert_eq!(
        server
            .admit(&expired, &context(&server, 200), &mut store)
            .unwrap_err(),
        "external_notification_expired"
    );
    let mut early = context(&server, 200);
    early.now_unix_ms = 50;
    assert_eq!(
        server.admit(&envelope(), &early, &mut store).unwrap_err(),
        "external_notification_expired"
    );
    let mut drifted = context(&server, 200);
    drifted.authority_epoch = 8;
    assert_eq!(
        server.admit(&envelope(), &drifted, &mut store).unwrap_err(),
        "external_notification_epoch_mismatch"
    );
    drifted.authority_epoch = 0;
    assert_eq!(
        server.admit(&envelope(), &drifted, &mut store).unwrap_err(),
        "external_notification_expected_epoch_invalid"
    );
    assert_eq!(store.admitted_count(), 0);
}

#[test]
fn origin_allowlist_and_signature_binding_fail_closed() {
    let server = registry(true);
    let mut store = replay();

    let mut foreign_origin = envelope();
    foreign_origin.destination_origin = "https://other.example".to_owned();
    foreign_origin.envelope_digest = foreign_origin.digest();
    assert_eq!(
        server
            .admit(&foreign_origin, &context(&server, 200), &mut store)
            .unwrap_err(),
        "external_notification_origin_not_allowlisted"
    );

    let mut foreign_key = envelope();
    foreign_key.signing_key_id = Some("key-attacker".to_owned());
    foreign_key.envelope_digest = foreign_key.digest();
    assert_eq!(
        server
            .admit(&foreign_key, &context(&server, 200), &mut store)
            .unwrap_err(),
        "external_notification_signature_binding_mismatch"
    );

    let mut unkeyed = envelope();
    unkeyed.signing_key_id = None;
    unkeyed.envelope_digest = unkeyed.digest();
    assert_eq!(
        server
            .admit(&unkeyed, &context(&server, 200), &mut store)
            .unwrap_err(),
        "external_notification_signature_key_missing"
    );
    assert_eq!(store.admitted_count(), 0);
}

#[test]
fn signature_check_reports_not_supported_instead_of_faking_verification() {
    let policy = enabled_policy();
    let check = check_external_notification_signature(&policy, &envelope()).unwrap();
    assert_eq!(
        check.verification,
        ExternalNotificationSignatureVerification::NotSupported
    );
    assert_eq!(
        check.reason,
        "external_notification_signature_verification_not_supported"
    );
    assert_eq!(check.signing_key_id, "key-nm19");
    assert!(check.check_digest.starts_with("sha256:"));
}

#[test]
fn receipt_binding_drift_is_refused() {
    let mut mismatched = receipt(ExternalNotificationReceiptStatus::Acknowledged);
    mismatched.delivery_id = "delivery:other".to_owned();
    mismatched.receipt_digest = mismatched.digest();
    assert_eq!(
        observe_external_notification_receipt(&envelope(), &mismatched).unwrap_err(),
        "external_notification_receipt_binding_mismatch"
    );
    let mut nonce_drift = receipt(ExternalNotificationReceiptStatus::Unknown);
    nonce_drift.nonce = "nonce-other".to_owned();
    nonce_drift.receipt_digest = nonce_drift.digest();
    assert_eq!(
        observe_external_notification_receipt(&envelope(), &nonce_drift).unwrap_err(),
        "external_notification_receipt_binding_mismatch"
    );
}

// ---------------------------------------------------------------------------------------------
// Acceptance and regression fixtures.
// ---------------------------------------------------------------------------------------------

#[test]
fn enabled_policy_still_returns_explicit_connector_handoff_only() {
    let server = registry(true);
    let result = server
        .admit(&envelope(), &context(&server, 200), &mut replay())
        .unwrap();
    assert_eq!(
        result.disposition,
        ExternalNotificationDisposition::ReadyForExplicitConnector
    );
    assert_eq!(
        result.reason,
        "explicit_connector_and_control_plane_required"
    );
    assert!(result.control_plane_required);
    assert!(!result.direct_effect);
    assert_eq!(result.policy_revision, 2);
    assert_eq!(result.signing_key_id.as_deref(), Some("key-nm19"));
    assert_eq!(
        result.signature_verification,
        ExternalNotificationSignatureVerification::NotSupported
    );
    assert_eq!(result.schema, EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA);
    assert_eq!(
        EXTERNAL_NOTIFICATION_REGISTRY_SCHEMA,
        "kiana.external-notification-policy-registry.v1"
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
            &receipt(ExternalNotificationReceiptStatus::Rejected)
        )
        .unwrap(),
        ExternalNotificationReceiptObservation::Rejected
    );
    assert_eq!(
        observe_external_notification_receipt(
            &envelope(),
            &receipt(ExternalNotificationReceiptStatus::Unknown)
        )
        .unwrap(),
        ExternalNotificationReceiptObservation::ReconcileRequired
    );
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
    let deny = fixture["deny_first"].as_array().unwrap();
    assert!(
        deny.len() >= 11,
        "the deny matrix must keep every refusal class: {}",
        deny.len()
    );
    for denial in [
        "disabled_by_default",
        "caller_fabricated_enabled_policy",
        "policy_revision_rollback_or_replay",
        "wrong_https_origin",
        "ssrf_or_metadata_origin",
        "origin_not_allowlisted",
        "nonce_replay_or_mismatch",
        "delivery_replay",
        "replay_window_capacity_or_ttl",
        "authority_epoch_drift",
        "data_epoch_drift",
        "expired_envelope",
        "signature_or_payload_digest_drift",
        "signature_key_binding_mismatch",
        "signature_verification_not_supported",
        "unknown_provider_receipt_is_reconcile",
        "receipt_binding_mismatch",
        "direct_network_effect",
    ] {
        assert!(
            deny.iter().any(|value| value == denial),
            "missing NM-19 denial: {denial}"
        );
    }
    for accepted in [
        "explicit_connector_handoff_only",
        "acknowledged_receipt_observation",
        "rejected_receipt_observation",
        "server_owned_policy_snapshot",
    ] {
        assert!(
            fixture["accepted"]
                .as_array()
                .unwrap()
                .iter()
                .any(|value| value == accepted),
            "missing NM-19 acceptance: {accepted}"
        );
    }
    assert_eq!(fixture["direct_effect"], false);
    assert_eq!(fixture["proof_ceiling"], "source");
}
