#[test]
fn audit_export_uses_the_bounded_export_profile_before_rendering_any_format() {
    let export = include_str!("../src/audit_export.rs");

    for marker in [
        "RedactionSignal::Export",
        "encode_bounded_value(&profile, &raw)",
        "audit_export_record_changed",
        "AuditExportError::RedactionFailed",
        "for record in &redacted",
        "oa03_audit_export_rejects_secret_sentinel_without_returning_content",
    ] {
        assert!(
            export.contains(marker),
            "missing OA-03 export boundary: {marker}"
        );
    }
}
