use kiana_domain::{
    json_digest, schema_contract, ProjectId, ProjectTrustSnapshot, StorageBackend,
    StorageFileIdentity, StorageOwnerScope, StoragePreflightReport, StoragePreflightRequest,
    StoragePreflightStatus, StorageRoot, StorageSecurityCapabilities,
    STORAGE_PREFLIGHT_REPORT_SCHEMA, STORAGE_PREFLIGHT_SCHEMA,
};
use serde_json::json;
use uuid::Uuid;

fn project() -> ProjectId {
    ProjectId::from_uuid(Uuid::from_u128(1))
}

fn root() -> StorageRoot {
    StorageRoot::new(
        "/tmp/kiana",
        StorageBackend::LocalFilesystem,
        StorageOwnerScope::new("owner", "instance", Some(project()), 4).unwrap(),
    )
    .unwrap()
}

fn project_root_digest() -> String {
    json_digest(&json!({"canonical_root": "/tmp/project"}))
}

fn trust(_root: &StorageRoot, trusted: bool) -> ProjectTrustSnapshot {
    let mut snapshot = ProjectTrustSnapshot {
        schema: kiana_domain::PROJECT_TRUST_SNAPSHOT_SCHEMA.to_owned(),
        version: kiana_domain::TRUST_SNAPSHOT_VERSION,
        project_id: project(),
        canonical_root_digest: project_root_digest(),
        trust_revision: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
            .to_owned(),
        trusted,
        source: "fixture".to_owned(),
        revision: 1,
        trust_digest: String::new(),
    };
    snapshot.trust_digest = snapshot.digest();
    snapshot.validate().unwrap();
    snapshot
}

fn identity(root: &StorageRoot, path: &str) -> StorageFileIdentity {
    StorageFileIdentity::new(
        root.root_digest.clone(),
        path,
        false,
        false,
        false,
        false,
        Some(0o600),
    )
    .unwrap()
}

fn request(
    root: StorageRoot,
    project_trust: ProjectTrustSnapshot,
    requested_paths: Vec<String>,
    file_identities: Vec<StorageFileIdentity>,
) -> StoragePreflightRequest {
    StoragePreflightRequest::new(
        root,
        project_trust,
        project_root_digest(),
        StorageSecurityCapabilities::current(),
        requested_paths,
        file_identities,
        10,
        100,
        1,
        100,
    )
    .unwrap()
}

#[test]
fn storage_preflight_schema_is_registered_and_trusted_root_is_ready() {
    for schema in [STORAGE_PREFLIGHT_SCHEMA, STORAGE_PREFLIGHT_REPORT_SCHEMA] {
        assert_eq!(schema_contract(schema).unwrap().owner_crate, "kiana-domain");
    }
    let root = root();
    let request = request(
        root.clone(),
        trust(&root, true),
        vec!["facts/events.jsonl".to_owned()],
        vec![identity(&root, "facts/events.jsonl")],
    );
    let report = StoragePreflightReport::evaluate(&request).unwrap();
    assert_eq!(report.status, StoragePreflightStatus::Ready);
    assert!(report.reasons.is_empty());
    report.validate().unwrap();
    let mut tampered = request.clone();
    tampered.request_digest =
        "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned();
    assert_eq!(
        StoragePreflightReport::evaluate(&tampered).unwrap_err(),
        "storage_preflight_request_digest_mismatch"
    );
    let mut unknown = serde_json::to_value(request).unwrap();
    unknown["unexpected"] = json!(true);
    assert!(serde_json::from_value::<StoragePreflightRequest>(unknown).is_err());
}

#[test]
fn storage_preflight_denies_trust_escape_remote_fs_and_capacity() {
    let root = root();
    let untrusted = StoragePreflightReport::evaluate(&request(
        root.clone(),
        trust(&root, false),
        vec!["facts/events.jsonl".to_owned()],
        vec![identity(&root, "facts/events.jsonl")],
    ))
    .unwrap();
    assert_eq!(untrusted.status, StoragePreflightStatus::Blocked);
    assert!(untrusted.reasons.contains(&"project_untrusted".to_owned()));

    let escaped = StoragePreflightReport::evaluate(&request(
        root.clone(),
        trust(&root, true),
        vec!["../escape".to_owned()],
        Vec::new(),
    ))
    .unwrap();
    assert!(escaped.reasons.contains(&"path_escape".to_owned()));
    let mut foreign_project = request(
        root.clone(),
        trust(&root, true),
        vec!["facts/events.jsonl".to_owned()],
        vec![identity(&root, "facts/events.jsonl")],
    );
    foreign_project.project_root_digest = json_digest(&json!({"canonical_root": "/tmp/foreign"}));
    foreign_project.request_digest = foreign_project.digest();
    let report = StoragePreflightReport::evaluate(&foreign_project).unwrap();
    assert!(report.reasons.contains(&"project_root_mismatch".to_owned()));
    assert!(StoragePreflightRequest::new(
        root.clone(),
        trust(&root, true),
        project_root_digest(),
        StorageSecurityCapabilities::current(),
        vec!["facts/\0events.jsonl".to_owned()],
        Vec::new(),
        10,
        100,
        1,
        100,
    )
    .is_err());

    let mut remote_root = root.clone();
    remote_root.backend = StorageBackend::NetworkFilesystem;
    remote_root.root_digest = remote_root.digest();
    let remote = StoragePreflightReport::evaluate(&request(
        remote_root.clone(),
        trust(&remote_root, true),
        vec!["facts/events.jsonl".to_owned()],
        vec![identity(&remote_root, "facts/events.jsonl")],
    ))
    .unwrap();
    assert!(remote
        .reasons
        .contains(&"remote_filesystem_unsupported".to_owned()));

    let mut capacity = request(
        root.clone(),
        trust(&root, true),
        vec!["facts/events.jsonl".to_owned()],
        vec![identity(&root, "facts/events.jsonl")],
    );
    capacity.required_bytes = 101;
    capacity.available_bytes = 100;
    capacity.request_digest = capacity.digest();
    let report = StoragePreflightReport::evaluate(&capacity).unwrap();
    assert!(report
        .reasons
        .contains(&"capacity_bytes_insufficient".to_owned()));
    capacity.required_inodes = 101;
    capacity.available_inodes = 100;
    capacity.request_digest = capacity.digest();
    let report = StoragePreflightReport::evaluate(&capacity).unwrap();
    assert!(report
        .reasons
        .contains(&"capacity_inodes_insufficient".to_owned()));

    let mut foreign_identity = identity(&root, "facts/events.jsonl");
    foreign_identity.root_digest = json_digest(&json!({"foreign_root": true}));
    foreign_identity.identity_digest = foreign_identity.digest();
    let report = StoragePreflightReport::evaluate(&request(
        root.clone(),
        trust(&root, true),
        vec!["facts/events.jsonl".to_owned()],
        vec![foreign_identity],
    ))
    .unwrap();
    assert!(report
        .reasons
        .contains(&"file_identity_root_mismatch".to_owned()));
}

#[test]
fn storage_preflight_denies_symlink_hardlink_permission_and_missing_capabilities() {
    let root = root();
    let mut symlink = StorageFileIdentity {
        schema: kiana_domain::STORAGE_FILE_IDENTITY_SCHEMA.to_owned(),
        version: kiana_domain::STORAGE_SECURITY_SCHEMA_VERSION,
        root_digest: root.root_digest.clone(),
        relative_path: "facts/events.jsonl".to_owned(),
        exists: true,
        regular_file: true,
        symlink: true,
        hardlink: true,
        mode: Some(0o644),
        identity_digest: String::new(),
    };
    symlink.identity_digest = symlink.digest();
    let mut capabilities = StorageSecurityCapabilities::current();
    capabilities.symlink_guard = false;
    capabilities.hardlink_guard = false;
    capabilities.permission_guard = false;
    capabilities.atomic_replace = false;
    capabilities.limitations = vec![
        "symlink_guard_requires_adapter_check".to_owned(),
        "hardlink_identity_requires_adapter_check".to_owned(),
        "permission_mode_unavailable".to_owned(),
        "atomic_replace_semantics_unproven".to_owned(),
    ];
    capabilities.capability_digest = capabilities.digest();
    let mut request = StoragePreflightRequest::new(
        root.clone(),
        trust(&root, true),
        project_root_digest(),
        capabilities,
        vec!["facts/events.jsonl".to_owned()],
        vec![symlink],
        10,
        100,
        1,
        100,
    )
    .unwrap();
    request.request_digest = request.digest();
    let report = StoragePreflightReport::evaluate(&request).unwrap();
    for reason in [
        "symlink_target",
        "hardlink_target",
        "permissions_too_broad",
        "symlink_guard_unavailable",
        "hardlink_guard_unavailable",
        "permission_guard_unavailable",
        "atomic_replace_unproven",
    ] {
        assert!(
            report.reasons.contains(&reason.to_owned()),
            "missing {reason}"
        );
    }
}
