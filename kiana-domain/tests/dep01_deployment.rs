use kiana_domain::{
    DeploymentPhase, DeploymentProfile, DeploymentRevision, EnvironmentProfile, InstanceId,
    ProjectId, SecretRefId, StorageBackend, StorageRootId,
};
use serde_json::json;
use uuid::Uuid;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn root_id(value: u128) -> StorageRootId {
    StorageRootId::from_uuid(Uuid::from_u128(value))
}

fn project_id(value: u128) -> ProjectId {
    ProjectId::from_uuid(Uuid::from_u128(value))
}

fn secret_ref(value: u128) -> SecretRefId {
    SecretRefId::from_uuid(Uuid::from_u128(value))
}

fn instance_id(value: u128) -> InstanceId {
    InstanceId::from_uuid(Uuid::from_u128(value))
}

fn environment(profile: DeploymentProfile) -> EnvironmentProfile {
    EnvironmentProfile::new(
        "env-dep01",
        profile,
        root_id(1),
        "/var/lib/kiana",
        Some(project_id(2)),
        Some(project_id(2)),
        Some("/workspace/project".to_owned()),
        Some(D.to_owned()),
        true,
        "config-r1",
        vec![secret_ref(3)],
        vec!["shell.read".to_owned(), "mcp.read".to_owned()],
        "linux-x86_64",
        StorageBackend::LocalFilesystem,
        "default",
    )
    .unwrap()
}

#[test]
fn profiles_round_trip_with_stable_digest() {
    for profile in [
        DeploymentProfile::EmbeddedLocal,
        DeploymentProfile::ManagedLocal,
        DeploymentProfile::Container,
        DeploymentProfile::Orchestrated,
    ] {
        let first = environment(profile);
        let second = environment(profile);
        assert_eq!(first.profile_digest, second.profile_digest);
        assert_eq!(
            serde_json::from_str::<EnvironmentProfile>(&serde_json::to_string(&first).unwrap())
                .unwrap(),
            first
        );
    }
}

#[test]
fn empty_escape_cross_project_and_untrusted_inputs_fail_closed() {
    let empty = EnvironmentProfile::new(
        "env",
        DeploymentProfile::EmbeddedLocal,
        root_id(1),
        "",
        None,
        None,
        None,
        None,
        true,
        "config",
        Vec::new(),
        Vec::new(),
        "linux",
        StorageBackend::LocalFilesystem,
        "default",
    )
    .unwrap_err();
    assert_eq!(empty, "deployment_storage_root_invalid");

    let escape = EnvironmentProfile::new(
        "env",
        DeploymentProfile::EmbeddedLocal,
        root_id(1),
        "/var/lib/../escape",
        None,
        None,
        None,
        None,
        true,
        "config",
        Vec::new(),
        Vec::new(),
        "linux",
        StorageBackend::LocalFilesystem,
        "default",
    )
    .unwrap_err();
    assert_eq!(escape, "deployment_storage_root_invalid");

    let cross_project = EnvironmentProfile::new(
        "env",
        DeploymentProfile::EmbeddedLocal,
        root_id(1),
        "/var/lib/kiana",
        Some(project_id(8)),
        Some(project_id(9)),
        Some("/workspace/project".to_owned()),
        Some(D.to_owned()),
        true,
        "config",
        Vec::new(),
        Vec::new(),
        "linux",
        StorageBackend::LocalFilesystem,
        "default",
    )
    .unwrap_err();
    assert_eq!(cross_project, "deployment_project_root_mismatch");

    let untrusted = EnvironmentProfile::new(
        "env",
        DeploymentProfile::Container,
        root_id(1),
        "/var/lib/kiana",
        Some(project_id(8)),
        Some(project_id(8)),
        Some("/workspace/project".to_owned()),
        Some(D.to_owned()),
        false,
        "config",
        Vec::new(),
        Vec::new(),
        "linux",
        StorageBackend::LocalFilesystem,
        "default",
    )
    .unwrap_err();
    assert_eq!(untrusted, "deployment_project_untrusted");
}

#[test]
fn unknown_profile_is_not_accepted_and_revision_is_digest_bound() {
    let value = serde_json::to_value(environment(DeploymentProfile::Container)).unwrap();
    let mut unknown = value;
    unknown["profile"] = json!("unknown");
    assert!(serde_json::from_value::<EnvironmentProfile>(unknown).is_err());

    let revision = DeploymentRevision::new(
        "revision-1",
        "release-1",
        DeploymentProfile::Container,
        instance_id(4),
        root_id(1),
        "build-1",
        2,
        3,
        DeploymentPhase::Serving,
        "healthy",
        100,
        None,
        None,
    )
    .unwrap();
    revision.validate().unwrap();
    assert_eq!(
        serde_json::from_str::<DeploymentRevision>(&serde_json::to_string(&revision).unwrap())
            .unwrap(),
        revision
    );
}
