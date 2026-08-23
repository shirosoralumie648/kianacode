use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn apply_patch_cassette() -> &'static str {
    r#"[{"text":"writing","tool_calls":[{"id":"c1","name":"apply_patch","arguments":{"patch":"*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"}}]},{"text":"created GOLDEN_PATH.txt"}]"#
}

fn unique_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "kiana-cli-workbench-{label}-{}-{stamp}",
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
    unique_dir("home")
}

fn harness_script(outputs: &str) -> PathBuf {
    let dir = unique_dir("script");
    let path = dir.join("script.json");
    fs::write(&path, outputs).unwrap();
    path
}

fn kiana_in(cwd: &Path, home: &Path, args: &[&str]) -> Output {
    kiana_cmd(cwd, home, None, None, args)
}

fn kiana_with_script(cwd: &Path, home: &Path, script: &Path, args: &[&str]) -> Output {
    kiana_cmd(cwd, home, Some(script), None, args)
}

fn kiana_cmd(
    cwd: &Path,
    home: &Path,
    script: Option<&Path>,
    picker: Option<&str>,
    args: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiana"));
    command
        .current_dir(cwd)
        .env("KIANA_HOME", home)
        .env("HOME", home)
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .args(args);
    match script {
        Some(script) => {
            command.env("KIANA_HARNESS_SCRIPT", script);
        }
        None => {
            command.env_remove("KIANA_HARNESS_SCRIPT");
        }
    }
    match picker {
        Some(picker) => {
            command.env("KIANA_FOLDER_PICKER_CMD", picker);
        }
        None => {
            command.env_remove("KIANA_FOLDER_PICKER_CMD");
        }
    }
    command.output().unwrap()
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn workbench_help_does_not_need_a_model() {
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_in(&cwd, &home, &["workbench", "--help"]);
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("kiana workbench"), "{text}");
    assert!(text.contains("--workdir"), "{text}");
    assert!(text.contains("DaemonHost"), "{text}");
}

#[test]
fn top_level_help_lists_workdir_and_picker() {
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_in(&cwd, &home, &["--help"]);
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("--workdir DIR"), "{text}");
    assert!(text.contains("--pick-folder"), "{text}");
    assert!(text.contains("folder workbench"), "{text}");
}

#[test]
fn untrusted_workbench_write_fails_closed() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(apply_patch_cassette());
    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "workbench",
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

#[test]
fn trusted_workbench_cassette_creates_golden_path_file() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(apply_patch_cassette());
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));

    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "--workdir",
            fixture.to_str().unwrap(),
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
    assert_eq!(response["output"]["harness"], "kiana-harness");
    assert_eq!(response["output"]["role_id"], "builder");
    assert_eq!(
        fs::read_to_string(fixture.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
    assert_eq!(response["output"]["files_changed"][0], "GOLDEN_PATH.txt");
}

#[test]
fn pick_folder_without_display_fails_closed() {
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_in(&cwd, &home, &["--pick-folder"]);
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("pick_folder_unavailable"),
        "{}",
        combined(&output)
    );
}

#[test]
fn pick_folder_command_then_one_shot_write() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(apply_patch_cassette());
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "{}", combined(&trust));

    let picker = format!("printf '%s' '{}'", fixture.display());
    let output = kiana_cmd(
        &home,
        &home,
        Some(&script),
        Some(&picker),
        &[
            "--pick-folder",
            "--json",
            "--sandbox",
            "workspace-write",
            "--",
            "create a file named GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    assert_eq!(
        fs::read_to_string(fixture.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}
