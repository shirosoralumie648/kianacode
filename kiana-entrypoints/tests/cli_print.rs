use serde_json::Value;
use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn harness_script(outputs: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("kiana-print-harness-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("script.json");
    fs::write(&path, outputs).unwrap();
    path
}

#[test]
fn print_mode_routes_through_kiana_harness() {
    let script = harness_script(r#"[{"text":"print harness ok"}]"#);
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .env("KIANA_HARNESS_SCRIPT", &script)
        .args([
            "--no-session-persistence",
            "-p",
            "--output-format",
            "json",
            "map the architecture",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["type"], "sdk_prompt_completed");
    assert_eq!(response["harness"], "kiana-harness");
    assert_eq!(response["execution"], "model");
    assert_eq!(response["assistant_text"], "print harness ok");
    assert_eq!(response["persistence"], "disabled");
}

#[test]
fn print_mode_without_model_fails_closed() {
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .env_remove("KIANA_HARNESS_SCRIPT")
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(["--no-session-persistence", "-p", "hello"])
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
