use kiana_domain::{
    DeploymentCompatibilityMatrix, DeploymentCompatibilityStatus, DeploymentVersionAxes,
    SchemaVersion,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn axes(store_format: u32) -> DeploymentVersionAxes {
    DeploymentVersionAxes::new(
        "build-1",
        SchemaVersion::new(1, 2),
        SchemaVersion::new(1, 4),
        store_format,
        SchemaVersion::new(1, 3),
        D,
        D,
        D,
        "config-r1",
        4,
        7,
        9,
    )
    .unwrap()
}

#[test]
fn identical_axes_are_compatible_and_round_trip() {
    let matrix = DeploymentCompatibilityMatrix::evaluate(axes(3), axes(3)).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Compatible);
    assert!(matrix.reasons.is_empty());
    assert_eq!(
        serde_json::from_str::<DeploymentCompatibilityMatrix>(
            &serde_json::to_string(&matrix).unwrap()
        )
        .unwrap(),
        matrix
    );
}

#[test]
fn unknown_major_downgrade_and_digest_drift_are_blocked() {
    let current = axes(3);

    let mut unknown_major = axes(3);
    unknown_major.protocol_version = SchemaVersion::new(9, 0);
    unknown_major.axes_digest = unknown_major.digest();
    let major_matrix =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), unknown_major).unwrap();
    assert_eq!(major_matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(major_matrix.reasons, vec!["protocol_major_mismatch"]);

    let downgrade = DeploymentCompatibilityMatrix::evaluate(current.clone(), axes(2)).unwrap();
    assert_eq!(downgrade.reasons, vec!["store_format_downgrade"]);

    let mut drift = axes(3);
    drift.workflow_definition_digest = D2.to_owned();
    drift.provider_route_digest = D2.to_owned();
    drift.extension_digest = D2.to_owned();
    drift.axes_digest = drift.digest();
    let drift_matrix = DeploymentCompatibilityMatrix::evaluate(current, drift).unwrap();
    assert_eq!(
        drift_matrix.reasons,
        vec![
            "workflow_definition_drift",
            "provider_route_drift",
            "extension_digest_drift",
        ]
    );
}

#[test]
fn config_revision_drift_is_blocked() {
    let current = axes(3);
    let mut candidate = axes(3);
    candidate.config_revision = "config-r2".to_owned();
    candidate.axes_digest = candidate.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, candidate).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["config_revision_drift"]);
}

#[test]
fn projection_minor_lag_is_blocked() {
    let current = axes(3);
    let mut candidate = axes(3);
    candidate.projection_version = SchemaVersion::new(1, 2);
    candidate.axes_digest = candidate.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, candidate).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["projection_version_downgrade"]);
}

#[test]
fn epoch_generation_and_schema_major_rollbacks_are_blocked() {
    let current = axes(3);

    let mut domain_major = axes(3);
    domain_major.domain_schema = SchemaVersion::new(2, 0);
    domain_major.axes_digest = domain_major.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current.clone(), domain_major).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["domain_schema_major_mismatch"]);

    let mut projection_major = axes(3);
    projection_major.projection_version = SchemaVersion::new(2, 0);
    projection_major.axes_digest = projection_major.digest();
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), projection_major).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["projection_major_mismatch"]);

    let mut authority_rollback = axes(3);
    authority_rollback.authority_epoch = 3;
    authority_rollback.axes_digest = authority_rollback.digest();
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), authority_rollback).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["authority_epoch_rollback"]);

    let mut data_rollback = axes(3);
    data_rollback.data_epoch = 6;
    data_rollback.axes_digest = data_rollback.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current.clone(), data_rollback).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["data_epoch_rollback"]);

    let mut generation_rollback = axes(3);
    generation_rollback.generation = 8;
    generation_rollback.axes_digest = generation_rollback.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, generation_rollback).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["generation_rollback"]);
}

#[test]
fn status_and_reasons_cannot_be_forged() {
    let mut matrix = DeploymentCompatibilityMatrix::evaluate(axes(3), axes(3)).unwrap();
    matrix.status = DeploymentCompatibilityStatus::Blocked;
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "deployment_compatibility_status_mismatch"
    );
}
