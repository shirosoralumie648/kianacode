use kiana_domain::{
    json_digest, schema_contract, DeploymentConfigSnapshot, DeploymentConfigSource,
    DeploymentConfigSourceKind, ProjectId, ProjectTrustSnapshot, SecretRef,
    DEPLOYMENT_CONFIG_DIFF_SCHEMA, DEPLOYMENT_CONFIG_SNAPSHOT_SCHEMA,
    DEPLOYMENT_CONFIG_SOURCE_SCHEMA,
};
use serde_json::json;
use uuid::Uuid;

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn project() -> ProjectId {
    ProjectId::from_uuid(Uuid::from_u128(1))
}

fn trust(trusted: bool) -> ProjectTrustSnapshot {
    let mut value = ProjectTrustSnapshot {
        schema: kiana_domain::PROJECT_TRUST_SNAPSHOT_SCHEMA.to_owned(),
        version: kiana_domain::TRUST_SNAPSHOT_VERSION,
        project_id: project(),
        canonical_root_digest: D.to_owned(),
        trust_revision: D.to_owned(),
        trusted,
        source: "fixture".to_owned(),
        revision: 1,
        trust_digest: String::new(),
    };
    value.trust_digest = value.digest();
    value.validate().unwrap();
    value
}

fn source(
    kind: DeploymentConfigSourceKind,
    reference: &str,
    trust_digest: Option<String>,
) -> DeploymentConfigSource {
    DeploymentConfigSource::new(kind, reference, trust_digest, D).unwrap()
}

fn snapshot() -> DeploymentConfigSnapshot {
    let trusted = trust(true);
    DeploymentConfigSnapshot::new(
        &trusted,
        vec!["KIANA_MODE".to_owned()],
        vec![
            source(
                DeploymentConfigSourceKind::CompiledDefault,
                "builtin:defaults",
                None,
            ),
            source(DeploymentConfigSourceKind::UserFile, "user:config", None),
            source(
                DeploymentConfigSourceKind::KianaHomeFile,
                "kiana-home:config",
                None,
            ),
            source(
                DeploymentConfigSourceKind::ProjectFile,
                "project:config",
                Some(trusted.trust_digest.clone()),
            ),
            source(
                DeploymentConfigSourceKind::Environment,
                "env:KIANA_MODE",
                None,
            ),
            source(
                DeploymentConfigSourceKind::CommandLine,
                "cli:--profile",
                None,
            ),
        ],
        json!({"provider": {"model": "fake", "token_ref": "secret-ref-1"}}),
        vec![SecretRef::new("local", "provider/token", "provider", "fake", 1).unwrap()],
    )
    .unwrap()
}

#[test]
fn deployment_config_sources_are_registered_ordered_and_immutable() {
    assert_eq!(
        schema_contract(DEPLOYMENT_CONFIG_SOURCE_SCHEMA)
            .unwrap()
            .owner_crate,
        "kiana-domain"
    );
    assert_eq!(
        schema_contract(DEPLOYMENT_CONFIG_SNAPSHOT_SCHEMA)
            .unwrap()
            .owner_crate,
        "kiana-domain"
    );
    assert_eq!(
        schema_contract(DEPLOYMENT_CONFIG_DIFF_SCHEMA)
            .unwrap()
            .owner_crate,
        "kiana-domain"
    );
    let snapshot = snapshot();
    snapshot.validate().unwrap();
    let precedences = snapshot
        .source_order()
        .map(|source| source.precedence)
        .collect::<Vec<_>>();
    assert_eq!(precedences, vec![10, 20, 25, 30, 40, 50]);
    assert_eq!(
        serde_json::from_str::<DeploymentConfigSnapshot>(
            &serde_json::to_string(&snapshot).unwrap()
        )
        .unwrap(),
        snapshot
    );
}

#[test]
fn deployment_config_denies_untrusted_project_env_escape_and_raw_secret() {
    let untrusted = trust(false);
    let project_source = source(
        DeploymentConfigSourceKind::ProjectFile,
        "project:config",
        Some(untrusted.trust_digest.clone()),
    );
    assert_eq!(
        DeploymentConfigSnapshot::new(
            &untrusted,
            Vec::new(),
            vec![project_source],
            json!({"safe": true}),
            Vec::new(),
        )
        .unwrap_err(),
        "deployment_config_project_trust_mismatch"
    );

    let trusted = trust(true);
    let env = source(
        DeploymentConfigSourceKind::Environment,
        "env:NOT_ALLOWED",
        None,
    );
    assert_eq!(
        DeploymentConfigSnapshot::new(
            &trusted,
            vec!["KIANA_MODE".to_owned()],
            vec![env],
            json!({"safe": true}),
            Vec::new(),
        )
        .unwrap_err(),
        "deployment_config_environment_not_allowlisted"
    );

    let mut raw = snapshot();
    raw.effective_non_secret_config = json!({"password": "raw-secret"});
    raw.config_revision = raw.config_revision_digest();
    assert_eq!(
        raw.validate().unwrap_err(),
        "deployment_config_secret_or_size_invalid"
    );
}

#[test]
fn deployment_config_rejects_tampered_revision_and_duplicate_source() {
    let mut revision = snapshot();
    revision.config_revision = D.to_owned();
    assert_eq!(
        revision.validate().unwrap_err(),
        "deployment_config_revision_mismatch"
    );

    let trusted = trust(true);
    let duplicate = source(DeploymentConfigSourceKind::UserFile, "user:second", None);
    let mut value = snapshot();
    value.sources.push(duplicate);
    value.sources.sort_by_key(|source| source.precedence);
    value.config_revision = value.config_revision_digest();
    value.snapshot_digest = value.digest();
    assert_eq!(
        value.validate().unwrap_err(),
        "deployment_config_source_duplicate"
    );
    let _ = trusted;
}

#[test]
fn deployment_config_diff_derives_changed_paths_and_impact() {
    let old = snapshot();
    let trusted = trust(true);
    let next = DeploymentConfigSnapshot::new(
        &trusted,
        vec!["KIANA_MODE".to_owned()],
        old.sources.clone(),
        json!({"migration": {"schema": 2}, "provider": {"model": "next"}}),
        old.secret_refs.clone(),
    )
    .unwrap();
    let diff = old.diff(&next).unwrap();
    diff.validate().unwrap();
    assert!(diff.changed_paths.contains(&"migration".to_owned()));
    assert!(diff.impact.restart_required);
    assert!(diff.impact.migration_required);
    assert!(!diff.changed_digests.is_empty());

    let no_op = old.diff(&old).unwrap();
    assert!(no_op.changed_paths.is_empty());
    assert!(!no_op.impact.restart_required);
}

#[test]
fn deployment_config_source_and_secret_reference_fences_are_strict() {
    assert!(DeploymentConfigSource::new(
        DeploymentConfigSourceKind::CommandLine,
        "cli:--token=raw",
        None,
        D,
    )
    .is_err());
    assert!(DeploymentConfigSource::new(
        DeploymentConfigSourceKind::Environment,
        "env:lowercase",
        None,
        D,
    )
    .is_err());
    let trusted = trust(true);
    let secret = SecretRef::new("local", "provider/token", "provider", "fake", 1).unwrap();
    assert!(DeploymentConfigSnapshot::new(
        &trusted,
        vec!["KIANA_MODE".to_owned()],
        vec![source(
            DeploymentConfigSourceKind::CompiledDefault,
            "builtin:defaults",
            None,
        )],
        json!({"safe": true}),
        vec![secret.clone(), secret],
    )
    .is_err());
}
