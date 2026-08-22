use serde_json::Value;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn harness_script(outputs: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("kiana-harness-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("script.json");
    fs::write(&path, outputs).unwrap();
    path
}

#[test]
fn run_routes_through_kiana_harness_and_reports_brokered_result() {
    let script = harness_script(
        r#"[{"text":"running ls","tool_calls":[{"id":"c1","name":"shell","arguments":{"command":"ls"}}]},{"text":"architecture mapped"}]"#,
    );
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .env("KIANA_HARNESS_SCRIPT", &script)
        .args(["run", "--json", "map the architecture"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["schema"], "kiana.run-result.v1");
    assert_eq!(response["output"]["harness"], "kiana-harness");
    assert_eq!(
        response["output"]["output"]["schema"],
        "kiana.harness-result.v1"
    );
    assert_eq!(response["output"]["output"]["text"], "architecture mapped");
}

#[test]
fn run_without_model_fails_closed() {
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .env_remove("KIANA_HARNESS_SCRIPT")
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(["run", "--json", "hello"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        combined.contains("model_unavailable") || combined.contains("kiana_harness"),
        "{combined}"
    );
}
