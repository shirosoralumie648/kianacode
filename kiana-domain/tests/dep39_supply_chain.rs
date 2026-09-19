use kiana_domain::{
    SupplyChainArtifactEvidence, SupplyChainArtifactKind, SupplyChainEvidence,
    SupplyChainGateReport, SupplyChainGateStatus,
};

const DIGEST_A: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const DIGEST_B: &str = "sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const DIGEST_C: &str = "sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DIGEST_D: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";

fn artifact(
    id: &str,
    kind: SupplyChainArtifactKind,
    complete: bool,
) -> SupplyChainArtifactEvidence {
    SupplyChainArtifactEvidence::new(
        id,
        kind,
        "linux-x86_64",
        DIGEST_A,
        DIGEST_B,
        DIGEST_C,
        DIGEST_D,
        complete,
        complete,
        complete,
        complete,
        complete,
    )
    .unwrap()
}

fn evidence(
    artifacts: Vec<SupplyChainArtifactEvidence>,
    unknown_licenses: u32,
    secret_scan_passed: bool,
) -> SupplyChainEvidence {
    SupplyChainEvidence::new(
        DIGEST_A,
        DIGEST_B,
        DIGEST_C,
        DIGEST_D,
        DIGEST_A,
        DIGEST_B,
        DIGEST_C,
        true,
        true,
        unknown_licenses,
        secret_scan_passed,
        "release-reviewer",
        artifacts,
    )
    .unwrap()
}

#[test]
fn gate_requires_signed_sbom_bound_desktop_and_binary_artifacts() {
    let facts = evidence(
        vec![
            artifact("binary-linux", SupplyChainArtifactKind::Binary, true),
            artifact(
                "desktop-linux",
                SupplyChainArtifactKind::DesktopPackage,
                true,
            ),
        ],
        0,
        true,
    );
    let report = SupplyChainGateReport::evaluate(&facts).unwrap();
    assert_eq!(report.status, SupplyChainGateStatus::Ready);
    assert!(report.publish_allowed);
    assert_eq!(report.artifact_count, 2);
}

#[test]
fn gate_blocks_unknown_license_secret_scan_and_incomplete_artifact() {
    let unknown_license = SupplyChainGateReport::evaluate(&evidence(
        vec![
            artifact("binary-linux", SupplyChainArtifactKind::Binary, true),
            artifact(
                "desktop-linux",
                SupplyChainArtifactKind::DesktopPackage,
                true,
            ),
        ],
        1,
        true,
    ))
    .unwrap();
    assert_eq!(unknown_license.reason, "supply_chain_license_unknown");
    assert!(!unknown_license.publish_allowed);

    let secret_failure = SupplyChainGateReport::evaluate(&evidence(
        vec![
            artifact("binary-linux", SupplyChainArtifactKind::Binary, true),
            artifact(
                "desktop-linux",
                SupplyChainArtifactKind::DesktopPackage,
                true,
            ),
        ],
        0,
        false,
    ))
    .unwrap();
    assert_eq!(secret_failure.reason, "supply_chain_secret_scan_failed");

    let artifact_failure = SupplyChainGateReport::evaluate(&evidence(
        vec![
            artifact("binary-linux", SupplyChainArtifactKind::Binary, false),
            artifact(
                "desktop-linux",
                SupplyChainArtifactKind::DesktopPackage,
                true,
            ),
        ],
        0,
        true,
    ))
    .unwrap();
    assert_eq!(
        artifact_failure.reason,
        "supply_chain_artifact_evidence_incomplete"
    );
}

#[test]
fn gate_blocks_missing_desktop_package() {
    let report = SupplyChainGateReport::evaluate(&evidence(
        vec![artifact(
            "binary-linux",
            SupplyChainArtifactKind::Binary,
            true,
        )],
        0,
        true,
    ))
    .unwrap();
    assert_eq!(report.status, SupplyChainGateStatus::Blocked);
    assert_eq!(report.reason, "supply_chain_desktop_package_missing");
}
