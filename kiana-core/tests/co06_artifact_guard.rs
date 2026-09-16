#[test]
fn company_evidence_uses_immutable_artifact_and_read_port_boundaries() {
    let domain = include_str!("../../kiana-domain/src/artifact_contracts.rs");
    let company = include_str!("../../kiana-domain/src/company.rs");
    let artifacts = include_str!("../src/artifacts.rs");
    let ports = include_str!("../../kiana-ports/src/lib.rs");

    for marker in [
        "ArtifactVersion",
        "ArtifactRef",
        "EvidenceRef",
        "Criterion",
        "content_hash",
        "criterion_id",
    ] {
        assert!(
            domain.contains(marker),
            "artifact contract marker missing: {marker}"
        );
    }
    for marker in ["typed_version", "typed_evidence_refs", "typed_criteria"] {
        assert!(
            company.contains(marker),
            "Company proof/reference marker missing: {marker}"
        );
    }
    for marker in [
        "artifact_version_from_content",
        "validate_artifact_reference_content",
        "read_project_artifact",
        "confined_artifact_path",
    ] {
        assert!(
            artifacts.contains(marker),
            "artifact boundary marker missing: {marker}"
        );
    }
    for marker in [
        "trait ArtifactContentPort",
        "read_artifact",
        "artifact_blob_missing",
        "artifact_content_hash_mismatch",
    ] {
        assert!(
            ports.contains(marker),
            "artifact port marker missing: {marker}"
        );
    }
}
