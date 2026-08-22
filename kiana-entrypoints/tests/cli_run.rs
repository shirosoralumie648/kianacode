use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn harness_script(outputs: &str) -> PathBuf {
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

fn apply_patch_cassette() -> &'static str {
    r#"[{"text":"writing","tool_calls":[{"id":"c1","name":"apply_patch","arguments":{"patch":"*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"}}]},{"text":"created GOLDEN_PATH.txt"}]"#
}

fn unique_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "kiana-cli-run-{label}-{}-{stamp}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

fn git_fixture() -> PathBuf {
    let root = unique_dir("fixture");
    fs::create_dir_all(root.join(".git")).unwrap();
    root
}

fn isolated_home() -> PathBuf {
    let home = unique_dir("home");
    fs::create_dir_all(&home).unwrap();
    home
}

fn kiana_in(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kiana"))
        .current_dir(cwd)
        .env("KIANA_HOME", home)
        .env("HOME", home)
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(args)
        .output()
        .unwrap()
}

fn kiana_with_script(cwd: &Path, home: &Path, script: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_kiana"))
        .current_dir(cwd)
        .env("KIANA_HOME", home)
        .env("HOME", home)
        .env("KIANA_HARNESS_SCRIPT", script)
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(args)
        .output()
        .unwrap()
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
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
    let combined = combined(&output);
    assert!(
        combined.contains("model_unavailable") || combined.contains("kiana_harness"),
        "{combined}"
    );
}

#[test]
fn empty_prompt_fails_closed_with_prompt_required() {
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(["run", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        combined(&output).contains("prompt_required"),
        "{}",
        combined(&output)
    );
}

#[test]
fn trusted_workspace_write_cassette_creates_golden_path_file() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(apply_patch_cassette());

    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    assert!(
        combined(&trust).contains("project_trust: trusted"),
        "{}",
        combined(&trust)
    );

    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--sandbox",
            "workspace-write",
            "--",
            "create a file named GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));

    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["schema"], "kiana.run-result.v1");
    assert_eq!(response["output"]["harness"], "kiana-harness");
    assert_eq!(response["output"]["sandbox"], "workspace-write");
    assert_eq!(response["output"]["role_id"], "builder");
    assert_eq!(response["output"]["department_id"], "executing");
    let dumped = serde_json::to_string(&response).unwrap();
    assert!(!dumped.contains("Read"));
    assert!(!dumped.contains("Edit"));
    assert!(!dumped.contains("Grep"));
    assert_eq!(
        fs::read_to_string(fixture.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn untrusted_workspace_write_does_not_create_file() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(apply_patch_cassette());
    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--sandbox",
            "workspace-write",
            "--",
            "create a file named GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("workspace_write_requires_trusted_non_safe_profile"),
        "{}",
        combined(&output)
    );
    assert!(!fixture.join("GOLDEN_PATH.txt").exists());
}
