use kiana_daemon::validate_protected_ingress;
use kiana_protocol::RequestMetadata;

#[test]
fn protected_ingress_rejects_bad_origin_host_and_missing_credential() {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.origin = Some("https://evil.example".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "ingress_origin_not_loopback"
    );

    metadata.origin = None;
    metadata.host = Some("evil.example".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "ingress_host_not_loopback"
    );

    metadata.host = Some("127.0.0.1:8787".to_owned());
    metadata.identity_mode = Some("protected_local".to_owned());
    assert_eq!(
        validate_protected_ingress(&metadata)
            .unwrap_err()
            .to_string(),
        "ingress_protected_credentials_required"
    );
}

#[test]
fn protected_loopback_metadata_accepts_opaque_credential_reference() {
    let mut metadata = RequestMetadata::local("session-1", "/repo");
    metadata.instance_id = Some("instance-1".to_owned());
    metadata.origin = Some("http://localhost:8787".to_owned());
    metadata.host = Some("127.0.0.1:8787".to_owned());
    metadata.identity_mode = Some("protected_local".to_owned());
    metadata.credential_ref = Some(
        kiana_domain::SecretRef::new(
            "env",
            "KIANA_LOCAL_CREDENTIAL",
            "daemon.ingress",
            "kiana-daemon",
            1,
        )
        .unwrap(),
    );
    validate_protected_ingress(&metadata).unwrap();
}
