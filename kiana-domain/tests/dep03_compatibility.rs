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
fn status_and_reasons_cannot_be_forged() {
    let mut matrix = DeploymentCompatibilityMatrix::evaluate(axes(3), axes(3)).unwrap();
    matrix.status = DeploymentCompatibilityStatus::Blocked;
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "deployment_compatibility_status_mismatch"
    );
}
