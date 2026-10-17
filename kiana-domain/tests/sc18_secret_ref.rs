//! SC-18 deny-first fixtures for the opaque `SecretRef` schema.
//!
//! A `SecretRef` is only a secret-free locator. These fixtures pin that it is versioned,
//! digest-bound, closed to unknown fields, and that a reference is not transferable between
//! provider accounts, purposes or audiences. No assertion here materializes a secret value.

use kiana_domain::{
    json_digest, CredentialLease, SecretRef, CREDENTIAL_LEASE_DEFAULT_TTL_MS, SECRET_REF_SCHEMA,
};

/// Raw value that must never appear in a serialized reference, lease or event envelope.
const SENTINEL: &str = "SC18_SECRET_REF_SENTINEL";

fn reference() -> SecretRef {
    SecretRef::new("env", "SC18_REF_KEY", "provider.request", "fake-provider", 3)
        .expect("valid reference")
}

fn endpoint_digest() -> String {
    json_digest(&serde_json::json!("https://provider.invalid/v1"))
}

fn lease() -> CredentialLease {
    CredentialLease::issue(
        reference(),
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest(),
        1_000,
        CREDENTIAL_LEASE_DEFAULT_TTL_MS,
    )
    .expect("valid lease")
}

#[test]
fn secret_ref_is_opaque_versioned_and_digest_bound() {
    let reference = reference();
    assert_eq!(reference.schema, SECRET_REF_SCHEMA);
    assert_eq!(reference.store, "env");
    assert_eq!(reference.key, "SC18_REF_KEY");
    assert_eq!(reference.purpose, "provider.request");
    assert_eq!(reference.audience, "fake-provider");
    assert_eq!(reference.generation, 3);
    assert_eq!(reference.reference_digest, reference.digest());
    reference.validate().expect("valid reference");

    // The reference digest covers only metadata; it changes when a bound field changes.
    let mut drifted = reference.clone();
    drifted.generation = 4;
    assert_ne!(drifted.digest(), reference.digest());
    assert_eq!(drifted.validate().unwrap_err(), "secret_ref_digest_mismatch");
}

#[test]
fn secret_ref_rejects_malformed_fields() {
    assert_eq!(
        SecretRef::new("", "key", "provider.request", "fake-provider", 1).unwrap_err(),
        "secret_ref_store_invalid"
    );
    assert_eq!(
        SecretRef::new("env", "  ", "provider.request", "fake-provider", 1).unwrap_err(),
        "secret_ref_key_invalid"
    );
    assert_eq!(
        SecretRef::new("env", "key", "", "fake-provider", 1).unwrap_err(),
        "secret_ref_purpose_invalid"
    );
    assert_eq!(
        SecretRef::new("env", "key", "provider.request", "", 1).unwrap_err(),
        "secret_ref_audience_invalid"
    );
    assert_eq!(
        SecretRef::new("env", "key", "provider.request", "fake-provider", 0).unwrap_err(),
        "secret_ref_header_invalid"
    );
    assert_eq!(
        SecretRef::new("env", "bad\nkey", "provider.request", "fake-provider", 1).unwrap_err(),
        "secret_ref_key_invalid"
    );
}

#[test]
fn secret_ref_is_not_transferable_across_account_purpose_or_audience() {
    let lease = lease();
    let digest = lease.endpoint_digest.clone();

    lease
        .validate_for(1_050, "fake-provider", "provider.request", "fake-provider", &digest)
        .expect("bound lease");

    assert_eq!(
        lease
            .validate_for(1_050, "other-provider", "provider.request", "fake-provider", &digest)
            .unwrap_err(),
        "credential_lease_provider_mismatch"
    );
    assert_eq!(
        lease
            .validate_for(1_050, "fake-provider", "audit", "fake-provider", &digest)
            .unwrap_err(),
        "credential_lease_purpose_mismatch"
    );
    assert_eq!(
        lease
            .validate_for(1_050, "fake-provider", "provider.request", "other-audience", &digest)
            .unwrap_err(),
        "credential_lease_audience_mismatch"
    );
    assert_eq!(
        lease
            .validate_for(
                1_050,
                "fake-provider",
                "provider.request",
                "fake-provider",
                &json_digest(&serde_json::json!("https://other.invalid/v1")),
            )
            .unwrap_err(),
        "credential_lease_endpoint_mismatch"
    );

    // A reference whose purpose no longer matches the lease purpose cannot even be issued.
    let mut drifted_reference = reference();
    drifted_reference.purpose = "audit".to_owned();
    drifted_reference.reference_digest = drifted_reference.digest();
    let drifted = CredentialLease::issue(
        drifted_reference,
        "fake-provider",
        "provider.request",
        "fake-provider",
        digest,
        1_000,
        100,
    );
    assert_eq!(drifted.unwrap_err(), "credential_lease_binding_mismatch");
}

#[test]
fn wire_log_event_payload_carries_no_secret_value() {
    let reference = reference();
    let lease = lease();

    let envelope = serde_json::json!({
        "wire": &reference,
        "log": &lease,
        "event": {
            "kind": "credential_lease_issued",
            "secret_ref": &reference,
        },
    });
    let encoded = serde_json::to_string(&envelope).expect("envelope json");
    assert!(!encoded.contains(SENTINEL));
    assert!(!encoded.contains("secret_value"));
    assert!(!encoded.contains("SC18_SECRET_REF_SENTINEL"));

    // Injecting a raw value slot is rejected by the closed schema rather than accepted.
    let mut tampered = serde_json::to_value(&reference).expect("reference value");
    tampered["secret_value"] = serde_json::json!(SENTINEL);
    assert!(serde_json::from_value::<SecretRef>(tampered).is_err());

    let mut tampered_lease = serde_json::to_value(&lease).expect("lease value");
    tampered_lease["secret_value"] = serde_json::json!(SENTINEL);
    assert!(serde_json::from_value::<CredentialLease>(tampered_lease).is_err());
}
