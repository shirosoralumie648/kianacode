use serde_json::Value;
use std::fs;
use std::path::Path;

fn source(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)).unwrap()
}

#[test]
fn deny_fixture_requires_zero_effect_and_stable_follow_up() {
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui32-deny-first.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(fixture["schema"], "kiana.ui-deny-first.v1");
    for item in fixture["denied"].as_array().unwrap() {
        assert_eq!(item["effect_count"], 0);
        assert!(item["error"]
            .as_str()
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '_'));
        assert!(matches!(
            item["follow_up"].as_str().unwrap(),
            "requery_original" | "safe_retry" | "do_not_retry" | "reconcile"
        ));
    }
}

#[test]
fn entrypoints_keep_deny_and_unknown_on_the_shared_spine() {
    let web = source("src/web.rs");
    let cli = source("src/cli.rs");
    let workbench = source("src/workbench_chat.rs");
    let harness = source("src/harness_run.rs");
    for marker in [
        "DaemonHost",
        "ControlPlane",
        "EventLog",
        "result_unknown",
        "session_unknown",
    ] {
        assert!(
            web.contains(marker)
                || cli.contains(marker)
                || workbench.contains(marker)
                || harness.contains(marker),
            marker
        );
    }
    assert!(web.contains("claim_action_submission"));
    assert!(web.contains("require_session_owner"));
    assert!(web.contains("authorize_mutation"));
    assert!(cli.contains("parity") || cli.contains("cancel"));
    assert!(workbench.contains("ChatAction::Cancel") || workbench.contains("cancel"));
}

#[test]
fn fixture_lists_injection_and_cancel_unknown_without_happy_path_substitution() {
    let fixture: Value = serde_json::from_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui32-deny-first.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let denied = fixture["denied"].as_array().unwrap();
    assert!(denied.iter().any(|item| item["id"] == "indirect_injection"));
    assert!(denied.iter().any(|item| item["id"] == "cancel_unknown"));
    assert!(fixture["success"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item.get("requires").is_some()));
}
