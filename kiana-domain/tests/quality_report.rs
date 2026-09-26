use kiana_domain::*;

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn manifest() -> QualityEvidenceManifest {
    let mut value = QualityEvidenceManifest {
        schema: QUALITY_EVIDENCE_MANIFEST_SCHEMA.to_owned(),
        source_cursor: 7,
        entries: vec![QualityEvidenceManifestEntry {
            kind: "event".to_owned(),
            reference: "evidence:case-1".to_owned(),
            digest: digest('a'),
            path: "artifacts/eq48/report.json".to_owned(),
        }],
        manifest_digest: String::new(),
    };
    value.manifest_digest = value.canonical_digest();
    value
}

fn reproduction() -> QualityReproductionCommand {
    let mut value = QualityReproductionCommand {
        schema: QUALITY_REPRODUCTION_SCHEMA.to_owned(),
        program: "kiana".to_owned(),
        args: vec![
            "eval".to_owned(),
            "run".to_owned(),
            "--suite-id".to_owned(),
            "suite-1".to_owned(),
        ],
        env_keys: vec!["KIANA_HOME".to_owned()],
        cwd: "artifacts/eq48".to_owned(),
        command_digest: String::new(),
    };
    value.command_digest = value.canonical_digest();
    value
}

fn report() -> QualityReport {
    let cases = vec![QualityReportCase {
        case_id: "case-1".to_owned(),
        status: QualityReportStatus::Passed,
        duration_ms: 12,
        failure_code: None,
        failure_message: None,
        evidence_refs: vec!["evidence:case-1".to_owned()],
    }];
    let mut value = QualityReport {
        schema: QUALITY_REPORT_SCHEMA.to_owned(),
        suite_ref: "suite:provider-independent".to_owned(),
        status: QualityReportStatus::Passed,
        summary: QualityReportSummary {
            total: 1,
            passed: 1,
            failed: 0,
            blocked: 0,
            duration_ms: 12,
        },
        cases,
        evidence: manifest(),
        reproduction: reproduction(),
        limitations: vec!["source evidence only".to_owned()],
        report_digest: String::new(),
    };
    value.report_digest = value.canonical_digest();
    value
}

#[test]
fn report_is_machine_readable_and_redacted() {
    let value = report();
    value.validate().expect("valid report");
    let artifacts = value.render_artifacts().expect("render artifacts");
    assert!(artifacts.json_report.contains(QUALITY_REPORT_SCHEMA));
    assert!(artifacts.junit.contains("<testsuite"));
    assert!(artifacts.human_summary.contains("reproduction:"));
    assert!(artifacts
        .evidence_manifest
        .contains(QUALITY_EVIDENCE_MANIFEST_SCHEMA));
    assert!(artifacts
        .reproduction_command
        .contains("KIANA_HOME=[REDACTED]"));
    for rendered in [
        artifacts.json_report,
        artifacts.junit,
        artifacts.human_summary,
        artifacts.evidence_manifest,
        artifacts.reproduction_command,
    ] {
        assert!(!rendered.contains("/home/"));
        assert!(!rendered.contains("api_key="));
    }
}

#[test]
fn report_rejects_forged_summary_paths_secret_and_missing_evidence() {
    let mut forged = report();
    forged.summary.passed = 0;
    assert_eq!(
        forged.validate().unwrap_err(),
        "quality_report_summary_mismatch"
    );

    let mut path = report();
    path.evidence.entries[0].path = "/home/operator/report.json".to_owned();
    path.evidence.manifest_digest = path.evidence.canonical_digest();
    path.report_digest = path.canonical_digest();
    assert!(path.validate().unwrap_err().contains("not_redacted"));

    let mut secret = report();
    secret.cases[0].failure_code = Some("secret_detected".to_owned());
    secret.cases[0].failure_message = Some("api_key=raw-value".to_owned());
    secret.cases[0].status = QualityReportStatus::Failed;
    secret.summary.failed = 1;
    secret.summary.passed = 0;
    secret.status = QualityReportStatus::Failed;
    secret.report_digest = secret.canonical_digest();
    assert!(secret.validate().unwrap_err().contains("not_redacted"));

    let mut missing = report();
    missing.cases[0].evidence_refs = vec!["evidence:missing".to_owned()];
    missing.report_digest = missing.canonical_digest();
    assert_eq!(
        missing.validate().unwrap_err(),
        "quality_report_evidence_reference_missing"
    );
}

#[test]
fn report_redactor_removes_path_and_secret_values() {
    let redacted = redact_report_text("api_key=secret-value /home/operator/report.json");
    assert!(redacted.contains("[REDACTED]"));
    assert!(redacted.contains("[REDACTED_PATH]"));
    assert!(!redacted.contains("secret-value"));
    assert!(!redacted.contains("/home/operator"));
}
