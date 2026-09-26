use kiana_domain::*;

fn hash(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn block() -> QualityEvidenceBlock {
    QualityEvidenceBlock {
        source_snapshot: "9e8f3eb5".to_owned(),
        worktree_status: "shared WIP preserved; EQ-51 files are scoped".to_owned(),
        command_argv: vec![
            "bash".to_owned(),
            "scripts/tests/eq51-evidence-archive-static.sh".to_owned(),
            "git diff --check".to_owned(),
        ],
        cwd_environment: "repository root; GitHub Actions Linux runner".to_owned(),
        fixture_cassette: "CI-only report trace-diff evidence reproduction fixtures".to_owned(),
        exit_code: "pending".to_owned(),
        status_change: "EQ-51 archive contract added; remote evidence remains pending".to_owned(),
        proof_level_change: "feature_status=partial; proof_level=source".to_owned(),
        limitations: vec![
            "No local Cargo tests or evaluator execution".to_owned(),
            "No durable, live or physical evidence".to_owned(),
        ],
        reviewer: "Codex EQ-51 source review".to_owned(),
    }
}

fn archive() -> QualityEvidenceArchive {
    let mut value = QualityEvidenceArchive {
        schema: QUALITY_EVIDENCE_ARCHIVE_SCHEMA.to_owned(),
        feature_status: QualityArchiveFeatureStatus::Partial,
        proof_level: QualityArchiveProofLevel::Source,
        block: block(),
        artifacts: vec![
            QualityEvidenceArchiveArtifact {
                kind: QualityArchiveArtifactKind::Report,
                schema: QUALITY_REPORT_SCHEMA.to_owned(),
                reference: "report".to_owned(),
                path: "artifacts/eq51/report.json".to_owned(),
                digest: hash('a'),
            },
            QualityEvidenceArchiveArtifact {
                kind: QualityArchiveArtifactKind::TraceDiff,
                schema: QUALITY_EVIDENCE_ARCHIVE_TRACE_DIFF_SCHEMA.to_owned(),
                reference: "trace-diff".to_owned(),
                path: "artifacts/eq51/trace-diff.json".to_owned(),
                digest: hash('b'),
            },
            QualityEvidenceArchiveArtifact {
                kind: QualityArchiveArtifactKind::Evidence,
                schema: QUALITY_EVIDENCE_MANIFEST_SCHEMA.to_owned(),
                reference: "evidence".to_owned(),
                path: "artifacts/eq51/evidence-manifest.json".to_owned(),
                digest: hash('c'),
            },
            QualityEvidenceArchiveArtifact {
                kind: QualityArchiveArtifactKind::Reproduction,
                schema: QUALITY_REPRODUCTION_SCHEMA.to_owned(),
                reference: "reproduction".to_owned(),
                path: "artifacts/eq51/reproduction.txt".to_owned(),
                digest: hash('d'),
            },
        ],
        archive_digest: String::new(),
    };
    value.archive_digest = value.canonical_digest();
    value
}

#[test]
fn archive_has_complete_source_evidence_block_and_four_artifacts() {
    let value = archive();
    value.validate().expect("valid source archive");
    let rendered = value.render_json().expect("render archive");
    assert!(rendered.contains(QUALITY_EVIDENCE_ARCHIVE_SCHEMA));
    assert!(rendered.contains("trace_diff"));
    assert!(rendered.contains("reproduction"));
}

#[test]
fn archive_rejects_missing_fields_paths_and_secrets() {
    let mut missing = archive();
    missing.block.reviewer.clear();
    missing.archive_digest = missing.canonical_digest();
    assert_eq!(missing.validate().unwrap_err(), "quality_evidence_reviewer");

    let mut traversal = archive();
    traversal.artifacts[0].path = "artifacts/eq51/../report.json".to_owned();
    traversal.archive_digest = traversal.canonical_digest();
    assert_eq!(
        traversal.validate().unwrap_err(),
        "quality_evidence_archive_path_invalid"
    );

    let mut secret = archive();
    secret.block.reviewer = "api_key=raw-value".to_owned();
    secret.archive_digest = secret.canonical_digest();
    assert_eq!(
        secret.validate().unwrap_err(),
        "quality_evidence_reviewer_not_redacted"
    );
}

#[test]
fn archive_rejects_duplicate_or_unavailable_proof() {
    let mut duplicate = archive();
    duplicate.artifacts[1].kind = QualityArchiveArtifactKind::Report;
    duplicate.archive_digest = duplicate.canonical_digest();
    assert_eq!(
        duplicate.validate().unwrap_err(),
        "quality_evidence_artifact_kind_duplicate"
    );

    let mut durable = archive();
    durable.proof_level = QualityArchiveProofLevel::Durable;
    durable.archive_digest = durable.canonical_digest();
    assert_eq!(
        durable.validate().unwrap_err(),
        "quality_evidence_archive_proof_level_unavailable"
    );

    let mut implemented = archive();
    implemented.feature_status = QualityArchiveFeatureStatus::Implemented;
    implemented.archive_digest = implemented.canonical_digest();
    assert_eq!(
        implemented.validate().unwrap_err(),
        "quality_evidence_archive_header_invalid"
    );
}
