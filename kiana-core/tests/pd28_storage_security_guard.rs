//! PD-28 source guard for the project-artifact persistence boundary.

#[test]
fn artifact_writes_validate_secret_free_content_before_opening_paths() {
    let source = include_str!("../src/artifacts.rs");
    for marker in [
        "fn validate_artifact_contents",
        "kiana_domain::validate_secret_free",
        "kiana_domain::redact_text",
        "validate_artifact_contents(contents, error)?",
        "prepare_project_artifact_linux",
        "create_artifact_temp_sibling",
    ] {
        assert!(source.contains(marker), "PD-28 marker missing: {marker}");
    }
    let validation = source
        .find("fn validate_artifact_contents")
        .expect("validation helper");
    let linux_write = source
        .find("prepare_project_artifact_linux(root, relative, contents, error)?.commit()")
        .expect("linux write");
    let fallback_write = source
        .rfind("let path = confined_artifact_path(root, relative, error)?;")
        .expect("fallback write");
    assert!(validation < linux_write);
    assert!(validation < fallback_write);
}

#[test]
fn artifact_source_keeps_opaque_secret_references_inside_structured_validation() {
    let source = include_str!("../src/artifacts.rs");
    assert!(source.contains("serde_json::from_slice::<Value>(contents)"));
    assert!(source.contains("validate_secret_free(&value)"));
    assert!(source.contains("redact_text(text) != text"));
}
