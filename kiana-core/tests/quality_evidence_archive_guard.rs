#[test]
fn eq51_archive_is_read_only_and_uses_existing_quality_facts() {
    let domain = include_str!("../../kiana-domain/src/quality_evidence_archive.rs");
    let core = include_str!("../src/quality_evidence_archive.rs");
    let lib = include_str!("../src/lib.rs");
    for marker in [
        "QUALITY_EVIDENCE_ARCHIVE_SCHEMA",
        "QualityEvidenceArchive",
        "QualityEvidenceBlock",
        "QualityArchiveArtifactKind",
        "QualityReport",
        "QualityEvidenceManifest",
        "QUALITY_REPRODUCTION_SCHEMA",
        "QualityArchiveProofLevel",
        "source_snapshot",
        "worktree_status",
        "command_argv",
        "cwd_environment",
        "fixture_cassette",
        "exit_code",
        "status_change",
        "proof_level_change",
        "limitations",
        "reviewer",
        "validate_quality_evidence_archive",
    ] {
        assert!(
            domain.contains(marker) || core.contains(marker) || lib.contains(marker),
            "EQ-51 marker missing: {marker}"
        );
    }
    for forbidden in [
        "CapabilityBroker::new",
        "ModelClient::new",
        "std::process::Command",
        "tokio::spawn",
        "std::fs::",
        "publish_report",
        "write_artifact",
    ] {
        assert!(
            !domain.contains(forbidden) && !core.contains(forbidden),
            "EQ-51 archive must not execute effects: {forbidden}"
        );
    }
}
