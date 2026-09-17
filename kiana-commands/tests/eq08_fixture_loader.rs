use kiana_commands::eval_fixtures::{
    load_fixture_manifest, EVAL_FIXTURE_MANIFEST_SCHEMA, EVAL_RUNTIME_FIXTURE_SCHEMA,
};
use serde_json::json;
use std::fs;
use std::path::PathBuf;

fn fixture_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("kiana-eq08-{label}-{}", std::process::id()))
}

fn write_manifest(root: &PathBuf, cases: serde_json::Value) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&json!({
            "schema": EVAL_FIXTURE_MANIFEST_SCHEMA,
            "suite_id": "eq08",
            "cases": cases
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn fixture_loader_is_deterministic_and_enforces_declared_schema() {
    let root = fixture_root("valid");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.jsonl"), b"a\n").unwrap();
    fs::write(root.join("b.jsonl"), b"b\n").unwrap();
    write_manifest(
        &root,
        json!([
            {"case_id":"z-case","fixture_ref":"b.jsonl","fixture_schema":EVAL_RUNTIME_FIXTURE_SCHEMA,"max_bytes":64},
            {"case_id":"a-case","fixture_ref":"a.jsonl","fixture_schema":EVAL_RUNTIME_FIXTURE_SCHEMA,"max_bytes":64}
        ]),
    );
    let loaded = load_fixture_manifest(&root, &root.join("manifest.json")).unwrap();
    assert_eq!(loaded.fixtures[0].case_id, "a-case");
    assert_eq!(loaded.fixtures[1].case_id, "z-case");
    assert!(loaded.fixtures[0].sha256.starts_with("sha256:"));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fixture_path_escape_unknown_schema_duplicate_and_size_fail_closed() {
    let root = fixture_root("invalid");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("ok.jsonl"), b"ok\n").unwrap();
    write_manifest(
        &root,
        json!([{"case_id":"escape","fixture_ref":"../outside.jsonl","fixture_schema":EVAL_RUNTIME_FIXTURE_SCHEMA,"max_bytes":64}]),
    );
    assert!(load_fixture_manifest(&root, &root.join("manifest.json")).is_err());

    write_manifest(
        &root,
        json!([{"case_id":"unknown","fixture_ref":"ok.jsonl","fixture_schema":"kiana.future.v9","max_bytes":64}]),
    );
    assert!(load_fixture_manifest(&root, &root.join("manifest.json")).is_err());

    write_manifest(
        &root,
        json!([
            {"case_id":"duplicate","fixture_ref":"ok.jsonl","fixture_schema":EVAL_RUNTIME_FIXTURE_SCHEMA,"max_bytes":64},
            {"case_id":"duplicate","fixture_ref":"ok.jsonl","fixture_schema":EVAL_RUNTIME_FIXTURE_SCHEMA,"max_bytes":64}
        ]),
    );
    assert!(load_fixture_manifest(&root, &root.join("manifest.json")).is_err());

    write_manifest(
        &root,
        json!([{"case_id":"too-small","fixture_ref":"ok.jsonl","fixture_schema":EVAL_RUNTIME_FIXTURE_SCHEMA,"max_bytes":1}]),
    );
    assert!(load_fixture_manifest(&root, &root.join("manifest.json")).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn fixture_manifest_unknown_fields_are_not_dropped() {
    let root = fixture_root("unknown");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("ok.jsonl"), b"ok\n").unwrap();
    fs::write(
        root.join("manifest.json"),
        serde_json::to_vec(&json!({
            "schema": EVAL_FIXTURE_MANIFEST_SCHEMA,
            "suite_id": "eq08",
            "cases": [],
            "unexpected": true
        }))
        .unwrap(),
    )
    .unwrap();
    assert!(load_fixture_manifest(&root, &root.join("manifest.json")).is_err());
    fs::remove_dir_all(root).unwrap();
}
