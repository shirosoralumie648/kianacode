use kiana_domain::{
    check_schema_compatibility, schema_contract, DeploymentCompatibilityMatrix,
    DeploymentCompatibilityStatus, DeploymentVersionAxes, SchemaLayer, SchemaVersion,
    DEPLOYMENT_COMPATIBILITY_SCHEMA, DEPLOYMENT_COMPATIBILITY_VERSION, MAX_SUPPORTED_STORE_FORMAT,
};

const D: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const D2: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

/// The store layout this binary actually reads. Fixtures start from the supported bound so an
/// in-bound comparison is not accidentally exercising the unsupported-format refusal.
const CURRENT_STORE: u32 = MAX_SUPPORTED_STORE_FORMAT;

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
fn deployment_compatibility_schema_is_registered_and_unknown_major_fails_closed() {
    let contract =
        schema_contract(DEPLOYMENT_COMPATIBILITY_SCHEMA).expect("schema must be registered");
    assert_eq!(contract.version, DEPLOYMENT_COMPATIBILITY_VERSION);
    assert_eq!(contract.layer, SchemaLayer::Domain);
    assert_eq!(contract.owner_crate, "kiana-domain");
    assert!(
        !contract.allow_unknown_fields,
        "the matrix structs deny unknown fields; the registry must say so"
    );
    assert_eq!(contract.version, SchemaVersion::new(1, 0));
    assert!(
        check_schema_compatibility(DEPLOYMENT_COMPATIBILITY_SCHEMA, &SchemaVersion::new(2, 0))
            .is_err(),
        "an unknown major must fail closed at the registry boundary too"
    );
    assert!(
        check_schema_compatibility(DEPLOYMENT_COMPATIBILITY_SCHEMA, &SchemaVersion::new(1, 7))
            .is_ok(),
        "a backward-compatible minor bump stays readable"
    );
}

#[test]
fn unsupported_store_format_is_named_and_blocked() {
    // A candidate layout above the bound this binary reads is an unknown format, not a
    // forward-compatible upgrade: it must be refused by name, never silently accepted.
    let current = axes(CURRENT_STORE);
    let mut future = axes(CURRENT_STORE);
    future.store_format = MAX_SUPPORTED_STORE_FORMAT + 1;
    future.axes_digest = future.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, future).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["store_format_unsupported"]);

    // A self-declared current snapshot above the bound is equally unreadable. Equal-to-equal must
    // not launder it into `Compatible`, which is what the previous downgrade-only check did.
    let mut already_future = axes(CURRENT_STORE);
    already_future.store_format = MAX_SUPPORTED_STORE_FORMAT + 1;
    already_future.axes_digest = already_future.digest();
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(already_future.clone(), already_future).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["store_format_unsupported"]);

    // Crossing from an unsupported layout down to the supported bound keeps both facts and a
    // deterministic order.
    let mut future_current = axes(CURRENT_STORE);
    future_current.store_format = MAX_SUPPORTED_STORE_FORMAT + 1;
    future_current.axes_digest = future_current.digest();
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(future_current, axes(CURRENT_STORE)).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(
        matrix.reasons,
        vec!["store_format_unsupported", "store_format_downgrade"]
    );

    // The bound is the layout the workspace writes, so the reason cannot be relaxed by a
    // caller-supplied constant.
    assert_eq!(
        MAX_SUPPORTED_STORE_FORMAT,
        kiana_domain::JOURNAL_WRITER_VERSION
    );
}

#[test]
fn unknown_major_downgrade_and_digest_drift_are_blocked() {
    let current = axes(CURRENT_STORE);

    let mut unknown_major = axes(CURRENT_STORE);
    unknown_major.protocol_version = SchemaVersion::new(9, 0);
    unknown_major.axes_digest = unknown_major.digest();
    let major_matrix =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), unknown_major).unwrap();
    assert_eq!(major_matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(major_matrix.reasons, vec!["protocol_major_mismatch"]);

    let downgrade =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), axes(CURRENT_STORE - 1)).unwrap();
    assert_eq!(downgrade.reasons, vec!["store_format_downgrade"]);

    let mut drift = axes(CURRENT_STORE);
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
    let current = axes(CURRENT_STORE);
    let mut candidate = axes(CURRENT_STORE);
    candidate.config_revision = "config-r2".to_owned();
    candidate.axes_digest = candidate.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, candidate).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["config_revision_drift"]);
}

#[test]
fn projection_minor_lag_is_blocked() {
    let current = axes(CURRENT_STORE);
    let mut candidate = axes(CURRENT_STORE);
    candidate.projection_version = SchemaVersion::new(1, 2);
    candidate.axes_digest = candidate.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, candidate).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["projection_version_downgrade"]);
}

#[test]
fn epoch_generation_and_schema_major_rollbacks_are_blocked() {
    let current = axes(CURRENT_STORE);

    let mut domain_major = axes(CURRENT_STORE);
    domain_major.domain_schema = SchemaVersion::new(2, 0);
    domain_major.axes_digest = domain_major.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current.clone(), domain_major).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["domain_schema_major_mismatch"]);

    let mut projection_major = axes(CURRENT_STORE);
    projection_major.projection_version = SchemaVersion::new(2, 0);
    projection_major.axes_digest = projection_major.digest();
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), projection_major).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["projection_major_mismatch"]);

    let mut authority_rollback = axes(CURRENT_STORE);
    authority_rollback.authority_epoch = 3;
    authority_rollback.axes_digest = authority_rollback.digest();
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(current.clone(), authority_rollback).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["authority_epoch_rollback"]);

    let mut data_rollback = axes(CURRENT_STORE);
    data_rollback.data_epoch = 6;
    data_rollback.axes_digest = data_rollback.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current.clone(), data_rollback).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["data_epoch_rollback"]);

    let mut generation_rollback = axes(CURRENT_STORE);
    generation_rollback.generation = 8;
    generation_rollback.axes_digest = generation_rollback.digest();
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, generation_rollback).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Blocked);
    assert_eq!(matrix.reasons, vec!["generation_rollback"]);
}

#[test]
fn status_and_reasons_cannot_be_forged() {
    let mut matrix =
        DeploymentCompatibilityMatrix::evaluate(axes(CURRENT_STORE), axes(CURRENT_STORE)).unwrap();
    matrix.status = DeploymentCompatibilityStatus::Blocked;
    matrix.matrix_digest = matrix.digest();
    assert_eq!(
        matrix.validate().unwrap_err(),
        "deployment_compatibility_status_mismatch"
    );
}

#[test]
fn identical_axes_are_compatible_and_round_trip() {
    let matrix =
        DeploymentCompatibilityMatrix::evaluate(axes(CURRENT_STORE), axes(CURRENT_STORE)).unwrap();
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

/// Card-explicit requirement: `app_build` is digest-bound but non-blocking. A different build
/// with the same data-facing axes is `Compatible` with empty reasons, so replacing the process
/// stays a later admission decision rather than an implicit data-compatibility claim.
#[test]
fn differing_app_build_stays_compatible_with_empty_reasons() {
    let current = axes(CURRENT_STORE);

    // Digest-bound: changing the build without recomputing the axes digest is rejected.
    let mut forged = axes(CURRENT_STORE);
    forged.app_build = "build-2".to_owned();
    assert_eq!(
        forged.validate().unwrap_err(),
        "deployment_compatibility_axes_digest_mismatch"
    );

    // Non-blocking: the same change with a recomputed digest stays compatible.
    let mut candidate = axes(CURRENT_STORE);
    candidate.app_build = "build-2".to_owned();
    candidate.axes_digest = candidate.digest();
    assert_ne!(
        candidate.axes_digest, current.axes_digest,
        "app_build must participate in the axes digest"
    );
    let matrix = DeploymentCompatibilityMatrix::evaluate(current, candidate).unwrap();
    assert_eq!(matrix.status, DeploymentCompatibilityStatus::Compatible);
    assert!(matrix.reasons.is_empty());
}
