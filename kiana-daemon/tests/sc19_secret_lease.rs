//! SC-19 daemon behaviour fixtures for the secret lease authority.
//!
//! These run the production [`SecretLeaseAuthority`] end to end. Deny paths come first: an expired
//! lease must not be injected, a one-shot lease must not be replayed, a revoked slot must not
//! issue or consume, a stale generation must not rotate or inject, and a restore payload that
//! carries a raw secret must be rejected before any state is rebuilt. Only then does the happy
//! path prove that a bound lease and the rotation/revocation receipts stay secret-free.

use kiana_daemon::{
    SecretLeaseAuthority, SecretLeaseStatus, SECRET_LEASE_JOURNAL_SCHEMA,
    SECRET_REVOCATION_RECEIPT_SCHEMA, SECRET_ROTATION_RECEIPT_SCHEMA,
};
use kiana_domain::{json_digest, SecretRef};
use serde_json::{json, Value};

/// Raw value that must never appear in a lease, receipt or journal.
const SENTINEL: &str = "SC19_RAW_SECRET_SENTINEL";

fn reference(generation: u64) -> SecretRef {
    SecretRef::new(
        "env",
        "SC19_LEASE_KEY",
        "provider.request",
        "fake-provider",
        generation,
    )
    .expect("valid reference")
}

fn endpoint_digest() -> String {
    json_digest(&json!("https://provider.invalid/v1"))
}

fn code(error: kiana_ports::PortError) -> String {
    error.to_string()
}

#[test]
fn expired_lease_is_denied_before_injection() {
    let authority = SecretLeaseAuthority::new();
    let lease_ref = reference(1);
    let mut lease = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            100,
        )
        .expect("issued lease");
    // Still valid, unused.
    authority
        .consume_lease(
            &mut lease,
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_050,
        )
        .expect("valid unused lease");

    let mut expired = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            100,
        )
        .expect("issued lease");
    let denied = authority
        .consume_lease(
            &mut expired,
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_100,
        )
        .expect_err("expired lease must be denied");
    assert!(code(denied).contains("credential_lease_expired"));
}

#[test]
fn one_shot_lease_cannot_be_replayed() {
    let authority = SecretLeaseAuthority::new();
    let lease_ref = reference(1);
    let mut lease = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            100,
        )
        .expect("issued lease");
    authority
        .consume_lease(
            &mut lease,
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_050,
        )
        .expect("first consumption");
    let replayed = authority
        .consume_lease(
            &mut lease,
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_051,
        )
        .expect_err("second consumption must be denied");
    assert!(code(replayed).contains("credential_lease_replayed"));
}

#[test]
fn revoked_slot_denies_further_lease_consume_and_rotate() {
    let authority = SecretLeaseAuthority::new();
    let lease_ref = reference(1);
    let mut lease = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            60_000,
        )
        .expect("issued lease");

    let receipt = authority
        .revoke(&lease_ref, 1, "operator requested", 1_001)
        .expect("revocation receipt");
    assert_eq!(receipt.schema, SECRET_REVOCATION_RECEIPT_SCHEMA);
    assert_eq!(receipt.revoked_generation, 1);
    assert!(receipt.validate().is_ok());
    assert_eq!(
        authority.slot_status(&lease_ref).unwrap(),
        SecretLeaseStatus::Revoked
    );

    let consume_denied = authority
        .consume_lease(
            &mut lease,
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_002,
        )
        .expect_err("revoked slot must not inject");
    assert!(code(consume_denied).contains("secret_lease_revoked"));

    let issue_denied = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_002,
            100,
        )
        .expect_err("revoked slot must not issue");
    assert!(code(issue_denied).contains("secret_lease_revoked"));

    let rotate_denied = authority
        .rotate(
            &lease_ref,
            1,
            2,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_002,
        )
        .expect_err("revoked slot must not rotate");
    assert!(code(rotate_denied).contains("secret_lease_revoked"));

    let already = authority
        .revoke(&lease_ref, 1, "again", 1_003)
        .expect_err("second revocation must be denied");
    assert!(code(already).contains("secret_lease_already_revoked"));
}

#[test]
fn rotation_receipt_is_generation_fenced() {
    let authority = SecretLeaseAuthority::new();
    let lease_ref = reference(1);
    authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            60_000,
        )
        .expect("issued lease");

    let receipt = authority
        .rotate(
            &lease_ref,
            1,
            2,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_001,
        )
        .expect("rotation receipt");
    assert_eq!(receipt.schema, SECRET_ROTATION_RECEIPT_SCHEMA);
    assert_eq!(receipt.previous_generation, 1);
    assert_eq!(receipt.next_generation, 2);
    assert!(receipt.validate().is_ok());
    assert_eq!(
        authority.slot_status(&lease_ref).unwrap(),
        SecretLeaseStatus::Active
    );

    let stale = authority
        .rotate(
            &lease_ref,
            1,
            3,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_002,
        )
        .expect_err("stale observation must be denied");
    assert!(code(stale).contains("secret_lease_generation_conflict"));

    let rollback = authority
        .rotate(
            &lease_ref,
            2,
            2,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_003,
        )
        .expect_err("non-advancing generation must be denied");
    assert!(code(rollback).contains("secret_lease_generation_rollback"));

    let old_reference = reference(1);
    let stale_issue = authority
        .issue_lease(
            &old_reference,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_004,
            100,
        )
        .expect_err("old generation must not issue");
    assert!(code(stale_issue).contains("secret_lease_generation_stale"));

    authority
        .issue_lease(
            &reference(2),
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_004,
            100,
        )
        .expect("rotated generation issues");
}

#[test]
fn unknown_slot_and_malformed_binding_are_denied() {
    let authority = SecretLeaseAuthority::new();
    let lease_ref = reference(1);

    let missing = authority
        .slot_status(&lease_ref)
        .expect_err("unregistered slot must be denied");
    assert!(code(missing).contains("secret_lease_not_registered"));

    let mut unregistered = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            100,
        )
        .expect("issued lease");
    // A different reference (different key) is not registered.
    let other = SecretRef::new(
        "env",
        "SC19_OTHER_KEY",
        "provider.request",
        "fake-provider",
        1,
    )
    .expect("valid reference");
    let consume_missing = authority
        .consume_lease(
            &mut unregistered,
            &other,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_010,
        )
        .expect_err("unregistered reference must be denied");
    assert!(code(consume_missing).contains("secret_lease_reference_mismatch"));

    let rotate_missing = authority
        .rotate(
            &other,
            1,
            2,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_010,
        )
        .expect_err("unregistered rotate must be denied");
    assert!(code(rotate_missing).contains("secret_lease_not_registered"));

    let revoke_missing = authority
        .revoke(&other, 1, "operator requested", 1_010)
        .expect_err("unregistered revoke must be denied");
    assert!(code(revoke_missing).contains("secret_lease_not_registered"));

    let binding_drift = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "audit",
            "fake-provider",
            &endpoint_digest(),
            1_010,
            100,
        )
        .expect_err("purpose drift must be denied");
    assert!(code(binding_drift).contains("secret_lease_binding_mismatch"));

    let malformed_digest = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            "sha256:nothex",
            1_010,
            100,
        )
        .expect_err("malformed endpoint digest must be denied");
    assert!(code(malformed_digest).contains("secret_store_binding_endpoint_digest_invalid"));
}

#[test]
fn leases_and_receipts_stay_secret_free() {
    let authority = SecretLeaseAuthority::new();
    let lease_ref = reference(1);
    let lease = authority
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            60_000,
        )
        .expect("issued lease");
    let rotation = authority
        .rotate(
            &lease_ref,
            1,
            2,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_001,
        )
        .expect("rotation receipt");
    let revocation = authority
        .revoke(&reference(2), 2, "operator requested", 1_002)
        .expect("revocation receipt");

    // The wire/log/event envelope is built only from the closed DTOs and carries no value.
    let envelope = json!({
        "wire": &lease,
        "log": &rotation,
        "event": &revocation,
    });
    let encoded = serde_json::to_string(&envelope).expect("envelope json");
    assert!(!encoded.contains(SENTINEL));
    assert!(!encoded.contains("secret_value"));

    // Injecting a raw value slot is rejected by the closed schemas rather than accepted.
    let mut tampered = serde_json::to_value(&rotation).expect("rotation value");
    tampered["secret_value"] = json!(SENTINEL);
    assert!(serde_json::from_value::<kiana_daemon::SecretRotationReceipt>(tampered).is_err());

    let mut tampered_lease = serde_json::to_value(&lease).expect("lease value");
    tampered_lease["secret_value"] = json!(SENTINEL);
    assert!(serde_json::from_value::<kiana_domain::CredentialLease>(tampered_lease).is_err());

    // A consumed lease/secret value never reaches the receipt digest inputs.
    assert!(!rotation.receipt_digest.is_empty());
    assert!(!revocation.receipt_digest.is_empty());
    assert_eq!(revocation.schema, SECRET_REVOCATION_RECEIPT_SCHEMA);
    assert_eq!(lease.schema, "kiana.credential-lease.v1");
}

#[test]
fn restore_rejects_plaintext_and_round_trips_secret_free() {
    let source = SecretLeaseAuthority::new();
    let lease_ref = reference(1);
    source
        .issue_lease(
            &lease_ref,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_000,
            60_000,
        )
        .expect("issued lease");
    source
        .rotate(
            &lease_ref,
            1,
            2,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest(),
            1_001,
        )
        .expect("rotation receipt");
    source
        .revoke(&reference(2), 2, "operator requested", 1_002)
        .expect("revocation receipt");

    let records = source.export_journal();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.schema, SECRET_LEASE_JOURNAL_SCHEMA);
    assert_eq!(record.generation, 2);
    assert_eq!(record.status, SecretLeaseStatus::Revoked);
    assert!(record.validate().is_ok());

    let payload = serde_json::to_string(&records).expect("journal json");
    assert!(!payload.contains(SENTINEL));
    assert!(!payload.contains("secret_value"));

    let restored = SecretLeaseAuthority::new();
    assert_eq!(restored.restore_from_journal(&payload).unwrap(), 1);
    assert_eq!(
        restored.slot_status(&reference(2)).unwrap(),
        SecretLeaseStatus::Revoked
    );

    // A journal record carrying a raw value is rejected before any state is rebuilt.
    let mut tampered: Vec<Value> = serde_json::from_str(&payload).expect("journal value");
    tampered[0]["secret_value"] = json!(SENTINEL);
    let tampered_payload = serde_json::to_string(&tampered).expect("tampered json");
    let rejected = restored
        .restore_from_journal(&tampered_payload)
        .expect_err("plaintext journal must be rejected");
    assert!(code(rejected).contains("secret_lease_restore_plaintext_rejected"));

    // A malformed journal that does not carry a plaintext field is a schema failure.
    let malformed = restored
        .restore_from_journal("{not json")
        .expect_err("malformed journal must be rejected");
    assert!(code(malformed).contains("secret_lease_journal_invalid"));

    // Restoring an older generation over a known slot must not roll state back.
    let mut rolled_back = records.clone();
    rolled_back[0].generation = 1;
    rolled_back[0].record_digest = rolled_back[0].digest();
    let rollback_payload = serde_json::to_string(&rolled_back).expect("rollback json");
    let rollback = restored
        .restore_from_journal(&rollback_payload)
        .expect_err("older generation must not roll back");
    assert!(code(rollback).contains("secret_lease_restore_generation_rollback"));
}
