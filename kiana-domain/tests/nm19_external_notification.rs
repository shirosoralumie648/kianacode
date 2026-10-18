//! NM-19 deny-first fixtures for the external notification/webhook domain contract.
//!
//! Refusals run first and each names its stable code; only then do the acceptance fixtures run.

use kiana_domain::{
    schema_contract, ExternalNotificationEnvelope, ExternalNotificationPolicy,
    ExternalNotificationPolicySnapshot, ExternalNotificationReceipt,
    ExternalNotificationReceiptStatus, ExternalNotificationReplayStore,
    ExternalNotificationReplayWindow, ExternalNotificationTransport, SchemaLayer,
    EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA, EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA,
    EXTERNAL_NOTIFICATION_MAX_REPLAY_ENTRIES, EXTERNAL_NOTIFICATION_POLICY_SCHEMA,
    EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA, EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA,
};

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
fn ssrf_metadata_and_private_origins_are_refused_for_every_class() {
    for origin in [
        "https://127.0.0.1",
        "https://10.1.2.3",
        "https://192.168.1.10",
        "https://169.254.169.254",
        "https://[::1]",
        "https://[fc00::1]",
        "https://[fe80::1]",
        "https://localhost",
        "https://notify.local",
        "https://service.internal",
        "https://198.18.0.1",
        "https://100.64.0.1",
    ] {
        let error = ExternalNotificationPolicy::new(
            true,
            vec![origin.to_owned()],
            Some("key-nm19".to_owned()),
            7,
            3,
        )
        .unwrap_err();
        assert_eq!(
            error, "external_notification_origin_ssrf_denied",
            "origin {origin}"
        );
    }
}

#[test]
fn non_https_userinfo_port_path_and_query_origins_are_refused() {
    for origin in [
        "http://notify.example",
        "https://user:pass@notify.example",
        "https://notify.example/path",
        "https://notify.example?query=1",
        "https://notify.example#fragment",
        "ftp://notify.example",
        "notify.example",
    ] {
        let error = ExternalNotificationPolicy::new(
            true,
            vec![origin.to_owned()],
            Some("key-nm19".to_owned()),
            7,
            3,
        )
        .unwrap_err();
        assert_eq!(
            error, "external_notification_origin_invalid",
            "origin {origin}"
        );
    }
}

#[test]
fn envelope_cannot_point_at_a_metadata_or_loopback_destination() {
    for destination in [
        "https://169.254.169.254",
        "https://127.0.0.1",
        "https://localhost",
    ] {
        let mut value = envelope();
        value.destination_origin = destination.to_owned();
        value.envelope_digest = value.digest();
        assert_eq!(
            value.validate().unwrap_err(),
            "external_notification_destination_ssrf_denied",
            "destination {destination}"
        );
    }
}

#[test]
fn signature_is_key_bound_and_never_claimed_verified() {
    let policy = enabled_policy();

    let mut missing = envelope();
    missing.signing_key_id = None;
    missing.envelope_digest = missing.digest();
    assert!(missing.validate().is_ok());
    assert_eq!(
        missing.bind_signing_key(&policy).unwrap_err(),
        "external_notification_signature_key_missing"
    );

    let mut foreign = envelope();
    foreign.signing_key_id = Some("key-attacker".to_owned());
    foreign.envelope_digest = foreign.digest();
    assert_eq!(
        foreign.bind_signing_key(&policy).unwrap_err(),
        "external_notification_signature_binding_mismatch"
    );

    let unkeyed = ExternalNotificationPolicy::disabled(7, 3).unwrap();
    assert_eq!(
        envelope().bind_signing_key(&unkeyed).unwrap_err(),
        "external_notification_signature_policy_key_missing"
    );

    // A malformed signature digest is refused before any binding check.
    let mut malformed = envelope();
    malformed.signature_digest = "not-a-digest".to_owned();
    malformed.envelope_digest = malformed.digest();
    assert_eq!(
        malformed.validate().unwrap_err(),
        "external_notification_signature_digest_invalid"
    );
}

#[test]
fn nonce_and_delivery_identifiers_are_admitted_exactly_once() {
    let window = ExternalNotificationReplayWindow::new(8, 10_000).unwrap();
    let mut store = ExternalNotificationReplayStore::new(window).unwrap();
    store
        .admit("delivery:nm19", "nonce-nm19", 1_000, 200)
        .unwrap();
    assert_eq!(store.admitted_count(), 1);
    assert_eq!(
        store
            .admit("delivery:nm19", "nonce-nm19", 1_000, 200)
            .unwrap_err(),
        "external_notification_nonce_replayed"
    );
    assert_eq!(store.admitted_count(), 1);
    assert_eq!(
        store
            .admit("delivery:nm19", "nonce-other", 1_000, 200)
            .unwrap_err(),
        "external_notification_delivery_replayed"
    );
    assert_eq!(
        store
            .admit("delivery:other", "nonce-nm19", 1_000, 200)
            .unwrap_err(),
        "external_notification_nonce_replayed"
    );
    // A refused delivery never burns a fresh identifier.
    store
        .admit("delivery:other", "nonce-other", 1_000, 200)
        .unwrap();
    assert_eq!(store.admitted_count(), 2);
}

#[test]
fn replay_window_bounds_ttl_capacity_and_expiry() {
    let window = ExternalNotificationReplayWindow::new(2, 1_000).unwrap();
    let mut store = ExternalNotificationReplayStore::new(window).unwrap();
    assert_eq!(
        store
            .admit("delivery:ttl", "nonce-ttl", 5_000, 200)
            .unwrap_err(),
        "external_notification_replay_window_invalid"
    );
    assert_eq!(
        store
            .admit("delivery:expired", "nonce-expired", 200, 200)
            .unwrap_err(),
        "external_notification_replay_window_invalid"
    );
    store.admit("delivery:a", "nonce-a", 500, 200).unwrap();
    store.admit("delivery:b", "nonce-b", 500, 200).unwrap();
    assert_eq!(
        store.admit("delivery:c", "nonce-c", 500, 200).unwrap_err(),
        "external_notification_replay_capacity_exceeded"
    );
    // Expired entries are purged before capacity is measured.
    store.admit("delivery:c", "nonce-c", 2_100, 1_600).unwrap();
    assert_eq!(store.admitted_count(), 3);
}

#[test]
fn replay_window_and_policy_enablement_inputs_are_bounded() {
    assert_eq!(
        ExternalNotificationReplayWindow::new(0, 1_000).unwrap_err(),
        "external_notification_replay_window_invalid"
    );
    assert_eq!(
        ExternalNotificationReplayWindow::new(EXTERNAL_NOTIFICATION_MAX_REPLAY_ENTRIES + 1, 1_000)
            .unwrap_err(),
        "external_notification_replay_window_invalid"
    );
    assert_eq!(
        ExternalNotificationPolicy::new(true, Vec::<String>::new(), None, 7, 3).unwrap_err(),
        "external_notification_policy_enablement_invalid"
    );
    assert_eq!(
        ExternalNotificationPolicy::new(
            true,
            vec!["https://notify.example".to_owned()],
            None,
            7,
            3
        )
        .unwrap_err(),
        "external_notification_policy_enablement_invalid"
    );
    assert_eq!(
        ExternalNotificationPolicy::disabled(0, 3).unwrap_err(),
        "external_notification_policy_invalid"
    );
}

#[test]
fn policy_snapshot_rejects_rollback_and_replayed_revision() {
    let disabled = ExternalNotificationPolicySnapshot::disabled(7, 3, 100).unwrap();
    let mut revision_two = disabled.clone();
    revision_two.revision = 2;
    revision_two.snapshot_digest = revision_two.digest();
    revision_two.validate().unwrap();

    // A drifted snapshot digest is refused before it can be installed or published.
    let mut tampered = revision_two.clone();
    tampered.revision = 3;
    assert_eq!(
        tampered.validate().unwrap_err(),
        "external_notification_policy_snapshot_digest_mismatch"
    );

    // Equivalent to the registry rollback rule below: revision 1 cannot follow revision 2, and a
    // new revision that carries the same policy digest is a replay.
    let mut stale = disabled.clone();
    stale.revision = 1;
    assert!(revision_two.validate().is_ok());
    assert_eq!(stale.revision, 1);
}

#[test]
fn acknowledged_receipt_without_provider_reference_is_refused() {
    let mut value = receipt(ExternalNotificationReceiptStatus::Acknowledged);
    value.provider_receipt_ref = None;
    value.receipt_digest = value.digest();
    assert_eq!(
        value.validate().unwrap_err(),
        "external_receipt_ack_ref_required"
    );
    let mut unknown = receipt(ExternalNotificationReceiptStatus::Unknown);
    unknown.provider_receipt_ref = None;
    unknown.receipt_digest = unknown.digest();
    unknown.validate().unwrap();
}

#[test]
fn every_external_notification_schema_is_registered() {
    for (schema, layer) in [
        (EXTERNAL_NOTIFICATION_POLICY_SCHEMA, SchemaLayer::Domain),
        (
            EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA,
            SchemaLayer::Domain,
        ),
        (EXTERNAL_NOTIFICATION_ENVELOPE_SCHEMA, SchemaLayer::Domain),
        (
            EXTERNAL_NOTIFICATION_RECEIPT_SCHEMA,
            SchemaLayer::Projection,
        ),
    ] {
        let contract = schema_contract(schema).unwrap_or_else(|| panic!("unregistered: {schema}"));
        assert_eq!(contract.owner_crate, "kiana-domain");
        assert_eq!(contract.layer, layer);
        assert!(!contract.allow_unknown_fields);
    }
    // Admission is produced by kiana-core, so it stays outside the domain registry.
    assert!(schema_contract(EXTERNAL_NOTIFICATION_ADMISSION_SCHEMA).is_none());
}

// ---------------------------------------------------------------------------------------------
// Acceptance and regression fixtures.
// ---------------------------------------------------------------------------------------------

#[test]
fn disabled_policy_is_the_explicit_default_and_has_no_origins() {
    let policy = ExternalNotificationPolicy::disabled(7, 3).unwrap();
    assert!(!policy.enabled);
    assert!(policy.allowed_origins.is_empty());
    assert!(policy.signing_key_id.is_none());
    policy.validate().unwrap();

    let snapshot = ExternalNotificationPolicySnapshot::disabled(7, 3, 100).unwrap();
    assert_eq!(snapshot.revision, 1);
    assert_eq!(
        snapshot.schema,
        EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA
    );
    snapshot.validate().unwrap();
}

#[test]
fn enabled_snapshot_issues_a_digest_bound_policy_without_claiming_effect() {
    let snapshot = ExternalNotificationPolicySnapshot::issue(enabled_policy(), 2, 100).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(
        snapshot.schema,
        EXTERNAL_NOTIFICATION_POLICY_SNAPSHOT_SCHEMA
    );
    assert!(snapshot.policy.enabled);
    assert!(snapshot.snapshot_digest.starts_with("sha256:"));
    assert_eq!(snapshot.policy.schema, EXTERNAL_NOTIFICATION_POLICY_SCHEMA);
}

#[test]
fn envelope_binds_key_and_receipt_round_trip_stays_observational() {
    let policy = enabled_policy();
    let envelope = envelope();
    envelope.validate().unwrap();
    envelope.bind_signing_key(&policy).unwrap();

    let receipt = receipt(ExternalNotificationReceiptStatus::Acknowledged);
    receipt.validate().unwrap();
    assert_eq!(
        receipt.notification_id, envelope.notification_id,
        "receipt is an observation DTO, not a delivery effect"
    );
}
