use kiana_capability_broker::consume_credential_lease;
use kiana_domain::{json_digest, CredentialLease, SecretRef};

#[test]
fn broker_consumes_only_the_exact_effect_binding() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    let reference = SecretRef::new(
        "env",
        "CI07_BROKER_SECRET",
        "provider.request",
        "fake-provider",
        1,
    )
    .expect("reference");
    let mut lease = CredentialLease::issue(
        reference,
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest.clone(),
        1_000,
        100,
    )
    .expect("lease");
    consume_credential_lease(
        &mut lease,
        1_001,
        "fake-provider",
        "provider.request",
        "fake-provider",
        &endpoint_digest,
    )
    .expect("first effect");
    assert!(matches!(
        consume_credential_lease(
            &mut lease,
            1_002,
            "fake-provider",
            "provider.request",
            "fake-provider",
            &endpoint_digest,
        ),
        Err(kiana_ports::PortError::Failed(code)) if code == "credential_lease_replayed"
    ));
}

#[test]
fn broker_denies_endpoint_drift_before_consumption() {
    let endpoint_digest = json_digest(&serde_json::json!("https://provider.invalid/v1"));
    let reference = SecretRef::new(
        "env",
        "CI07_BROKER_SECRET",
        "provider.request",
        "fake-provider",
        1,
    )
    .expect("reference");
    let mut lease = CredentialLease::issue(
        reference,
        "fake-provider",
        "provider.request",
        "fake-provider",
        endpoint_digest,
        1_000,
        100,
    )
    .expect("lease");
    let result = consume_credential_lease(
        &mut lease,
        1_001,
        "fake-provider",
        "provider.request",
        "fake-provider",
        &json_digest(&serde_json::json!("https://other.invalid/v1")),
    );
    assert!(matches!(
        result,
        Err(kiana_ports::PortError::Failed(code)) if code == "credential_lease_endpoint_mismatch"
    ));
}
