use serde_json::Value;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

fn apply_patch_cassette_for(path: &str) -> String {
    format!(
        r#"[{{"text":"writing","tool_calls":[{{"id":"c1","name":"apply_patch","arguments":{{"patch":"*** Begin Patch\n*** Add File: {path}\n+hello\n*** End Patch\n"}}}}]}},{{"text":"created {path}"}}]"#
    )
}

fn write_builder_packet(root: &Path) {
    fs::create_dir_all(root.join("packet")).unwrap();
    fs::write(
        root.join("packet").join("TASK.json"),
        r#"{
  "schema": "kiana.work-packet.v1",
  "id": "wp-1",
  "from_department": "planning",
  "to_department": "executing",
  "assignee_role": "builder",
  "goal": "create GOLDEN_PATH.txt containing hello",
  "path_allow": ["GOLDEN_PATH.txt"]
}"#,
    )
    .unwrap();
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
        .env_remove("KIANA_HARNESS_SCRIPT")
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(args)
        .output()
        .unwrap()
}

fn kiana_command_with_script(cwd: &Path, home: &Path, script: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiana"));
    command
        .current_dir(cwd)
        .env("KIANA_HOME", home)
        .env("HOME", home)
        .env("KIANA_HARNESS_SCRIPT", script)
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .args(args);
    command
}

fn kiana_with_script(cwd: &Path, home: &Path, script: &Path, args: &[&str]) -> Output {
    kiana_command_with_script(cwd, home, script, args)
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
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_with_script(
        &cwd,
        &home,
        &script,
        &["run", "--json", "map the architecture"],
    );
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
fn run_streams_each_delta_by_default_before_terminal_receipt() {
    let script = harness_script(
        r#"[{"text":"first chunk","tool_calls":[{"id":"c1","name":"shell","arguments":{"command":"sleep 1"}}]},{"text":" second chunk"}]"#,
    );
    let cwd = unique_dir("stream-cwd");
    let home = isolated_home();
    // 不传 --stream：覆盖 §8 决定的默认流式路径。
    let mut child = kiana_command_with_script(&cwd, &home, &script, &["run", "stream it"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let mut stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut chunks = Vec::new();
        let mut buffer = [0_u8; 64];
        loop {
            match stdout.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    let chunk = buffer[..read].to_vec();
                    chunks.push(chunk.clone());
                    if sender.send(chunk).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        chunks
    });

    let first = receiver
        .recv_timeout(Duration::from_secs(10))
        .expect("first streamed chunk");
    assert_eq!(String::from_utf8_lossy(&first), "first chunk");
    assert!(
        child.try_wait().unwrap().is_none(),
        "streamed chunk was buffered until process exit"
    );

    let status = child.wait().unwrap();
    let chunks = reader.join().unwrap();
    let stdout = String::from_utf8_lossy(&chunks.concat()).into_owned();
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .unwrap();

    assert!(status.success(), "stderr: {stderr}");
    assert_eq!(stdout.matches("first chunk").count(), 1, "{stdout}");
    assert!(stdout.contains(" second chunk"), "{stdout}");
    assert!(stdout.contains("session_id: "), "{stdout}");
    assert!(stdout.contains("run_id: "), "{stdout}");
}

#[test]
fn run_json_keeps_machine_output_clean_with_stream_flag() {
    let script = harness_script(r#"[{"text":"machine output"}]"#);
    let output = kiana_with_script(
        &unique_dir("stream-json-cwd"),
        &isolated_home(),
        &script,
        &["run", "--json", "--stream", "hello"],
    );

    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["output"]["text"], "machine output");
}

#[test]
fn run_without_stream_keeps_final_only_human_output() {
    let script = harness_script(
        r#"[{"text":"interim","tool_calls":[{"id":"c1","name":"shell","arguments":{"command":"true"}}]},{"text":"final text"}]"#,
    );
    let output = kiana_with_script(
        &unique_dir("no-stream-cwd"),
        &isolated_home(),
        &script,
        &["run", "--no-stream", "hello"],
    );

    assert!(output.status.success(), "{}", combined(&output));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("final text\n"), "{stdout}");
    assert!(!stdout.contains("interim"), "{stdout}");
    assert!(stdout.contains("session_id: "), "{stdout}");
}

#[test]
fn run_without_model_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &["run", "--json", "hello"],
    );
    assert!(!output.status.success());
    let combined = combined(&output);
    assert!(
        combined.contains("model_unavailable") || combined.contains("kiana_harness"),
        "{combined}"
    );
}

#[test]
fn empty_prompt_fails_closed_with_prompt_required() {
    let output = kiana_in(&unique_dir("cwd"), &isolated_home(), &["run", "--json"]);
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
    assert_eq!(response["output"]["files_changed"][0], "GOLDEN_PATH.txt");
    let run_id = response["output"]["run_id"].as_str().unwrap();
    let receipt = kiana_with_script(
        &fixture,
        &home,
        &script,
        &["run", "--json", "--receipt", run_id],
    );
    assert!(receipt.status.success(), "{}", combined(&receipt));
    let receipt: Value = serde_json::from_slice(&receipt.stdout).unwrap();
    assert_eq!(receipt["status"], "completed");
    assert_eq!(receipt["output"]["run_id"], run_id);
    assert_eq!(receipt["output"]["files_changed"][0], "GOLDEN_PATH.txt");
    assert!(
        receipt["output"]["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["operation"] == "apply_patch"),
        "{receipt:?}"
    );
    assert!(home.join("sessions").join("events.jsonl").exists());
}

#[test]
fn planning_pm_cannot_apply_patch_source() {
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
            "run",
            "--json",
            "--role",
            "pm",
            "--sandbox",
            "workspace-write",
            "--",
            "create a file named GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "failed");
    assert_eq!(response["error"], "role_path_denied");
    assert_eq!(response["output"]["role_id"], "pm");
    assert_eq!(response["output"]["department_id"], "planning");
    assert!(!fixture.join("GOLDEN_PATH.txt").exists());
}

#[test]
fn planning_pm_can_apply_patch_plan_artifact() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(&apply_patch_cassette_for("plan/WORK.md"));
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));

    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--role",
            "pm",
            "--sandbox",
            "workspace-write",
            "--",
            "write plan/WORK.md",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["role_id"], "pm");
    assert_eq!(response["output"]["department_id"], "planning");
    assert_eq!(response["output"]["files_changed"][0], "plan/WORK.md");
    assert_eq!(
        fs::read_to_string(fixture.join("plan").join("WORK.md")).unwrap(),
        "hello\n"
    );
}

#[test]
fn unknown_role_fails_closed() {
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
            "run",
            "--json",
            "--role",
            "ceo",
            "--sandbox",
            "workspace-write",
            "--",
            "create GOLDEN_PATH.txt",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("role_unknown"),
        "{}",
        combined(&output)
    );
    assert!(!fixture.join("GOLDEN_PATH.txt").exists());
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

#[test]
fn run_json_prints_session_and_run_ids() {
    let script = harness_script(r#"[{"text":"architecture mapped"}]"#);
    let output = kiana_with_script(
        &unique_dir("cwd"),
        &isolated_home(),
        &script,
        &["run", "--json", "map the architecture"],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert!(response["output"]["run_id"].as_str().is_some());
    assert!(response["output"]["session_id"].as_str().is_some());
}

#[test]
fn continue_unknown_session_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &[
            "run",
            "--json",
            "--continue",
            "00000000-0000-4000-8000-000000000001",
            "--",
            "keep going",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("run_not_found") || combined.contains("session_not_found"),
        "{combined}"
    );
    if let Ok(response) = serde_json::from_slice::<Value>(&output.stdout) {
        assert_ne!(response["status"], "completed");
    }
}

#[test]
fn receipt_unknown_session_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &[
            "run",
            "--json",
            "--receipt",
            "00000000-0000-4000-8000-000000000003",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("receipt_not_found") || combined.contains("session_not_found"),
        "{combined}"
    );
}

#[test]
fn tui_help_is_parked_off_the_v0_2_product_path() {
    let output = kiana_in(&unique_dir("cwd"), &isolated_home(), &["--help"]);
    assert!(output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(text.contains("kiana tui"), "{text}");
    assert!(text.contains("Parked in v0.2"), "{text}");
    assert!(text.contains("not DaemonHost"), "{text}");
}

#[test]
fn tui_without_tty_is_not_the_product_path() {
    let output = kiana_in(&unique_dir("cwd"), &isolated_home(), &["tui"]);
    assert!(!output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(
        text.contains("interactive") || text.contains("terminal"),
        "{text}"
    );
    assert!(!text.contains("harness: kiana-harness"), "{text}");
    assert!(!text.contains("kiana-harness"), "{text}");
}

#[test]
fn packet_spawn_creates_golden_path_without_prompt() {
    let fixture = git_fixture();
    let home = isolated_home();
    write_builder_packet(&fixture);
    let script = harness_script(apply_patch_cassette());
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--sandbox",
            "workspace-write",
            "--packet",
            "packet/TASK.json",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["role_id"], "builder");
    assert_eq!(response["output"]["department_id"], "executing");
    assert_eq!(response["output"]["work_packet_id"], "wp-1");
    assert_eq!(response["output"]["input"], "work_packet");
    assert_eq!(response["output"]["files_changed"][0], "GOLDEN_PATH.txt");
    assert_eq!(
        fs::read_to_string(fixture.join("GOLDEN_PATH.txt")).unwrap(),
        "hello\n"
    );
}

#[test]
fn packet_unknown_path_fails_closed() {
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
            "run",
            "--json",
            "--sandbox",
            "workspace-write",
            "--packet",
            "packet/MISSING.json",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("packet_not_found"),
        "{}",
        combined(&output)
    );
    assert!(!fixture.join("GOLDEN_PATH.txt").exists());
}

#[test]
fn packet_with_prompt_fails_closed() {
    let fixture = git_fixture();
    let home = isolated_home();
    write_builder_packet(&fixture);
    let output = kiana_in(
        &fixture,
        &home,
        &[
            "run",
            "--json",
            "--packet",
            "packet/TASK.json",
            "--",
            "also a prompt",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("packet_prompt_conflict"),
        "{}",
        combined(&output)
    );
}

#[test]
fn packet_role_pm_fails_closed() {
    let fixture = git_fixture();
    let home = isolated_home();
    write_builder_packet(&fixture);
    let script = harness_script(apply_patch_cassette());
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    let output = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--role",
            "pm",
            "--sandbox",
            "workspace-write",
            "--packet",
            "packet/TASK.json",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("packet_role_must_be_builder"),
        "{}",
        combined(&output)
    );
    assert!(!fixture.join("GOLDEN_PATH.txt").exists());
}

#[test]
fn packet_parent_path_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &["run", "--json", "--packet", "../secret.json"],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("packet_path_denied"),
        "{}",
        combined(&output)
    );
}

#[test]
fn cancel_unknown_session_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &[
            "run",
            "--json",
            "--cancel",
            "00000000-0000-4000-8000-000000000002",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let combined = combined(&output);
    assert!(
        combined.contains("run_not_found") || combined.contains("session_not_found"),
        "{combined}"
    );
    if let Ok(response) = serde_json::from_slice::<Value>(&output.stdout) {
        assert_ne!(response["status"], "completed");
    }
}

#[test]
fn symposium_anti_meeting_writes_decision_and_packet() {
    let fixture = git_fixture();
    let home = isolated_home();
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    let output = kiana_in(
        &fixture,
        &home,
        &[
            "run",
            "--json",
            "--symposium",
            "--anti-meeting",
            "--sandbox",
            "workspace-write",
            "--",
            "create GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["schema"], "kiana.symposium-result.v1");
    assert_eq!(response["output"]["builder_present"], false);
    assert_eq!(response["output"]["skipped_meeting"], true);
    assert_eq!(
        response["output"]["decision"]["schema"],
        "kiana.decision-record.v1"
    );
    assert_eq!(response["output"]["packet"]["assignee_role"], "builder");
    let decision = fs::read_to_string(fixture.join("plan").join("DECISION.json")).unwrap();
    let packet = fs::read_to_string(fixture.join("packet").join("TASK.json")).unwrap();
    assert!(decision.contains("kiana.decision-record.v1"), "{decision}");
    assert!(packet.contains("kiana.work-packet.v1"), "{packet}");
    assert!(
        packet.contains("create GOLDEN_PATH.txt containing hello"),
        "{packet}"
    );
}

#[test]
fn symposium_role_builder_fails_closed() {
    let fixture = git_fixture();
    let home = isolated_home();
    let output = kiana_in(
        &fixture,
        &home,
        &[
            "run",
            "--json",
            "--symposium",
            "--anti-meeting",
            "--role",
            "builder",
            "--sandbox",
            "workspace-write",
            "--",
            "create GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("symposium_chair_must_be_pm"),
        "{}",
        combined(&output)
    );
    assert!(!fixture.join("plan").join("DECISION.json").exists());
}

#[test]
fn symposium_without_goal_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &["run", "--json", "--symposium", "--anti-meeting"],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("symposium_goal_required"),
        "{}",
        combined(&output)
    );
}

#[test]
fn anti_meeting_without_symposium_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &[
            "run",
            "--json",
            "--anti-meeting",
            "--",
            "create GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("anti_meeting_requires_symposium"),
        "{}",
        combined(&output)
    );
}

#[test]
fn symposium_with_packet_fails_closed() {
    let fixture = git_fixture();
    let home = isolated_home();
    write_builder_packet(&fixture);
    let output = kiana_in(
        &fixture,
        &home,
        &[
            "run",
            "--json",
            "--symposium",
            "--anti-meeting",
            "--packet",
            "packet/TASK.json",
            "--",
            "create GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("use only one of --symposium"),
        "{}",
        combined(&output)
    );
}

#[test]
fn review_after_builder_writes_gate_packet() {
    let fixture = git_fixture();
    let home = isolated_home();
    let script = harness_script(apply_patch_cassette());
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    let built = kiana_with_script(
        &fixture,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--sandbox",
            "workspace-write",
            "--",
            "create GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(built.status.success(), "{}", combined(&built));
    let built_json: Value = serde_json::from_slice(&built.stdout).unwrap();
    assert_eq!(built_json["status"], "completed");
    let author = built_json["output"]["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(built_json["output"]["role_id"], "builder");

    let reviewed = kiana_in(&fixture, &home, &["run", "--json", "--review", &author]);
    assert!(reviewed.status.success(), "{}", combined(&reviewed));
    let response: Value = serde_json::from_slice(&reviewed.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["schema"], "kiana.review-result.v1");
    assert_eq!(response["output"]["role_id"], "reviewer");
    assert_eq!(response["output"]["department_id"], "monitoring");
    assert_eq!(response["output"]["author_session_id"], author);
    assert_ne!(response["output"]["session_id"], author);
    assert_eq!(response["output"]["verdict"], "pass");
    assert_eq!(response["output"]["files_reviewed"][0], "GOLDEN_PATH.txt");
    let packet = fs::read_to_string(fixture.join("gate").join("REVIEW.json")).unwrap();
    assert!(packet.contains("kiana.review-packet.v1"), "{packet}");
    assert!(packet.contains(&author), "{packet}");
}

#[test]
fn review_role_builder_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &[
            "run",
            "--json",
            "--role",
            "builder",
            "--review",
            "00000000-0000-4000-8000-000000000009",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("review_role_must_be_reviewer"),
        "{}",
        combined(&output)
    );
}

#[test]
fn review_with_prompt_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &[
            "run",
            "--json",
            "--review",
            "00000000-0000-4000-8000-000000000009",
            "--",
            "also a prompt",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("review_prompt_conflict"),
        "{}",
        combined(&output)
    );
}

#[test]
fn review_without_author_fails_closed() {
    let output = kiana_in(
        &unique_dir("cwd"),
        &isolated_home(),
        &["run", "--json", "--review"],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("review_author_required"),
        "{}",
        combined(&output)
    );
}

#[test]
fn review_unknown_author_fails_closed() {
    let fixture = git_fixture();
    let home = isolated_home();
    let trust = kiana_in(&fixture, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    let output = kiana_in(
        &fixture,
        &home,
        &[
            "run",
            "--json",
            "--review",
            "00000000-0000-4000-8000-000000000009",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    assert!(
        combined(&output).contains("review_author_not_found"),
        "{}",
        combined(&output)
    );
}

#[test]
fn planning_reviewer_cannot_apply_patch_source() {
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
            "run",
            "--json",
            "--role",
            "reviewer",
            "--sandbox",
            "workspace-write",
            "--",
            "create a file named GOLDEN_PATH.txt containing hello",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let text = combined(&output);
    assert!(
        text.contains("role_tool_denied") || text.contains("role_sandbox_read_only"),
        "{text}"
    );
    assert!(!fixture.join("GOLDEN_PATH.txt").exists());
}
