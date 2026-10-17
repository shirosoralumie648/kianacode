//! EXT-28 versioned wire fixtures for the extension command contract. Local tests are not run
//! in this session (CI-only); the manual lane is `.github/workflows/ext28-extension-commands.yml`.

use kiana_domain::ExtensionConfigSnapshot;
use kiana_protocol::{
    ExtensionCommand, ExtensionCommandRequest, RequestBody, RequestEnvelope, RequestMetadata,
};

const TRUST_REVISION: &str =
    "sha256:1111111111111111111111111111111111111111111111111111111111111111";
const DEPENDENCY_DIGEST: &str =
    "sha256:2222222222222222222222222222222222222222222222222222222222222222";

#[test]
fn typed_extension_command_round_trips_through_generic_envelope() {
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Disable);
    request.extension_id = Some("example.skill".to_owned());
    request.expected_registry_version = Some(7);
    request.actor_id = Some("operator".to_owned());
    request.reason = Some("pause pending review".to_owned());
    request.idempotency_key = Some("disable-example-7".to_owned());

    let envelope = RequestEnvelope::extension_command(
        RequestMetadata::local("session", "/workspace"),
        request.clone(),
    )
    .expect("valid extension command");
    let RequestBody::Command(command) = &envelope.body else {
        panic!("typed extension command must use shared command route");
    };
    assert_eq!(command.name, "extension.disable");
    assert_eq!(command.arguments["action"], "disable");
    let encoded = serde_json::to_vec(&envelope).unwrap();
    let decoded: RequestEnvelope = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, envelope);
}

#[test]
fn enable_carries_the_server_issued_lifecycle_binding_on_the_wire() {
    let snapshot = ExtensionConfigSnapshot::new("example.skill", 4, Vec::new())
        .expect("fixture config snapshot");
    let snapshot_digest = snapshot.snapshot_digest.clone();
    let mut request = ExtensionCommandRequest::new(ExtensionCommand::Enable);
    request.extension_id = Some("example.skill".to_owned());
    request.expected_registry_version = Some(4);
    request.actor_id = Some("operator".to_owned());
    request.reason = Some("enable after review".to_owned());
    request.idempotency_key = Some("enable-example-4".to_owned());
    request.trust_revision = Some(TRUST_REVISION.to_owned());
    request.dependency_snapshot_digest = Some(DEPENDENCY_DIGEST.to_owned());
    request.approval_id = Some("approval.ext28".to_owned());
    request.config_snapshot_digest = Some(snapshot_digest.clone());
    request.config_snapshot = Some(snapshot);
    request.validate().expect("bound enable command");

    let envelope = RequestEnvelope::extension_command(
        RequestMetadata::local("session", "/workspace"),
        request,
    )
    .expect("valid enable command");
    let RequestBody::Command(command) = &envelope.body else {
        panic!("typed extension command must use shared command route");
    };
    assert_eq!(command.name, "extension.enable");
    for key in [
        "trust_revision",
        "dependency_snapshot_digest",
        "approval_id",
        "config_snapshot_digest",
        "config_snapshot",
    ] {
        assert!(
            !command.arguments[key].is_null(),
            "missing enable binding {key}"
        );
    }
    assert_eq!(command.arguments["config_snapshot_digest"], snapshot_digest);
    let decoded: RequestEnvelope =
        serde_json::from_slice(&serde_json::to_vec(&envelope).unwrap()).unwrap();
    assert_eq!(decoded, envelope);
}
