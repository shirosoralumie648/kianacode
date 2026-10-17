//! H19 入口接线守卫与 CLI 端到端 fixtures。
//!
//! 三入口（CLI / TTY / Web）必须复用同一条 KianaClient→DaemonHost→ControlPlane 主路径：
//! 本文件先钉「拒绝」路径（过期 turn 不得落成 accepted 事实、参数缺失不得静默），再钉
//! 「成功」路径（当前 turn 的 steer/inject 得到 queued 回执并写入 accepted 事件）。

use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

fn source(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(relative)).unwrap()
}

fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
    source
        .split(start)
        .nth(1)
        .and_then(|rest| rest.split(end).next())
        .unwrap_or_else(|| panic!("missing marker slice: {start} .. {end}"))
}

fn braced_body<'a>(source: &'a str, name: &str) -> &'a str {
    let start = source
        .find(&format!("struct {name}"))
        .expect("struct present");
    let open = start + source[start..].find('{').expect("opening brace present");
    let close = open + source[open..].find('}').expect("closing brace present");
    &source[open + 1..close]
}

#[test]
fn entrypoints_route_steer_and_inject_through_the_shared_spine() {
    let harness = source("src/harness_run.rs");
    let cli = source("src/cli.rs");
    let web = source("src/web.rs");
    let workbench = source("src/workbench_chat.rs");

    // 共享 helper 是唯一出口：三入口都从这里回到协议客户端。
    assert!(harness.contains("pub async fn steer_envelope"));
    assert!(harness.contains("pub async fn inject_envelope"));
    assert!(harness.contains("client.steer_run("));
    assert!(harness.contains("client.inject_run("));

    assert!(cli.contains("\"--steer\""));
    assert!(cli.contains("\"--inject\""));
    assert!(cli.contains("harness_run::steer_envelope("));
    assert!(cli.contains("harness_run::inject_envelope("));
    assert!(cli.contains("steer_turn_required"));
    assert!(cli.contains("turn_id_requires_steer_or_inject"));
    assert!(cli.contains("inject_options_require_inject"));

    assert!(web.contains("\"/api/steer\""));
    assert!(web.contains("\"/api/inject\""));
    assert!(web.contains("WebRouteClass::Steer"));
    assert!(web.contains("WebRouteClass::Inject"));
    assert!(web.contains("steer_envelope_on_host"));
    assert!(web.contains("inject_envelope_on_host"));

    // TTY：运行中继续输入的普通文本走 next-turn 注入，而不是要求先取消或另起 run。
    let queue = slice_between(&workbench, "fn queue_running_input", "\nfn submit_turn");
    assert!(queue.contains("inject_envelope_on_host"));
    assert!(queue.contains("\"next-turn\""));
    assert!(queue.contains("\"tty\""));
    assert!(queue.contains("input_tx.send(result)"));
    assert!(!queue.contains("continue_envelope_on_host"));
    assert!(!queue.contains("run_envelope_on_host"));
}

#[test]
fn steer_inject_wiring_carries_no_permission_mutation_and_rejects_before_claiming() {
    let web = source("src/web.rs");
    let workbench = source("src/workbench_chat.rs");

    // 两个 Web 请求体都拒绝未知字段：浏览器不能夹带 sandbox / 模型档位之类的字段。
    assert!(web.contains("#[serde(deny_unknown_fields)]\nstruct SteerBody"));
    assert!(web.contains("#[serde(deny_unknown_fields)]\nstruct InjectBody"));
    for name in ["SteerBody", "InjectBody"] {
        let body = braced_body(&web, name);
        for forbidden in ["sandbox", "model", "permission"] {
            assert!(
                !body.contains(forbidden),
                "{name} must not accept permission-mutating field: {forbidden}"
            );
        }
    }

    // 先校验形状再占用幂等槽：畸形请求不能消费重放键，也不能绕过 owner 检查。
    let steer = slice_between(&web, "async fn steer_turn", "async fn inject_turn");
    assert!(steer.contains("require_session_owner"));
    let shape = steer.find("web_input_text_required").unwrap();
    let claim = steer.find("claim_action_submission").unwrap();
    assert!(
        shape < claim,
        "steer must reject malformed input before claiming the replay key"
    );
    let inject = slice_between(&web, "async fn inject_turn", "async fn resume_turn");
    assert!(inject.contains("\"web\","));
    let shape = inject.find("web_input_text_required").unwrap();
    let claim = inject.find("claim_action_submission").unwrap();
    assert!(
        shape < claim,
        "inject must reject malformed input before claiming the replay key"
    );

    // 运行中排队的回执不得把本地 running 状态改成终态。
    let receipt = slice_between(
        &workbench,
        "pub fn apply_input_response",
        "fn finish_non_completed_response",
    );
    assert!(!receipt.contains("running = false"));
    assert!(!receipt.contains("stream_turn = None"));
}

fn harness_script(outputs: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("kiana-h19-script-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("script.json");
    fs::write(&path, outputs).unwrap();
    path
}

fn unique_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("kiana-h19-{label}-{pid}-{stamp}"));
    fs::create_dir_all(&dir).unwrap();
    fs::canonicalize(&dir).unwrap()
}

fn isolated_home() -> PathBuf {
    unique_dir("home")
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

fn trusted_fixture(label: &str) -> (PathBuf, PathBuf) {
    let cwd = unique_dir(label);
    fs::create_dir_all(cwd.join(".git")).unwrap();
    let home = isolated_home();
    let trust = kiana_in(&cwd, &home, &["trust", "."]);
    assert!(trust.status.success(), "trust failed: {}", combined(&trust));
    (cwd, home)
}

fn combined(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn events(home: &Path) -> Vec<Value> {
    let path = home.join("sessions").join("events.jsonl");
    if !path.exists() {
        return Vec::new();
    }
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn accepted_facts(facts: &[Value]) -> Vec<&Value> {
    facts
        .iter()
        .filter(|event| event["kind"] == "run.input.accepted")
        .collect()
}

fn turn_id_for_request(response: &Value) -> String {
    let raw = response["request_id"].as_str().expect("request_id present");
    let request_id = kiana_protocol::RequestId::parse_str(raw).expect("request_id parses");
    kiana_protocol::TurnId::from_uuid(request_id.as_uuid()).to_string()
}

#[test]
fn cli_steer_with_stale_turn_is_rejected_without_accepted_fact() {
    let script = harness_script(r#"[{"text":"mapped"}]"#);
    let (cwd, home) = trusted_fixture("steer-stale");
    let first = kiana_with_script(&cwd, &home, &script, &["run", "--json", "map the harness"]);
    assert!(first.status.success(), "{}", combined(&first));
    let run: Value = serde_json::from_slice(&first.stdout).unwrap();
    let run_id = run["output"]["run_id"].as_str().unwrap().to_owned();
    let accepted_before = accepted_facts(&events(&home)).len();

    let stale = kiana_protocol::TurnId::new().to_string();
    let output = kiana_with_script(
        &cwd,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--steer",
            run_id.as_str(),
            "--turn",
            stale.as_str(),
            "--",
            "steer into an expired turn",
        ],
    );
    assert!(!output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "blocked");
    assert_eq!(response["error"], "stale_turn_steer");
    let facts = events(&home);
    assert_eq!(accepted_facts(&facts).len(), accepted_before);
    assert!(!facts.iter().any(|event| {
        event["data"]["text"]
            .as_str()
            .is_some_and(|text| text.contains("steer into an expired turn"))
    }));
}

#[test]
fn cli_steer_without_turn_flag_fails_before_any_daemon_request() {
    let cwd = unique_dir("steer-no-turn");
    let home = isolated_home();
    let output = kiana_in(
        &cwd,
        &home,
        &[
            "run",
            "--json",
            "--steer",
            "00000000-0000-4000-8000-000000000010",
            "--",
            "steer without turn",
        ],
    );
    assert!(!output.status.success());
    assert!(
        combined(&output).contains("steer_turn_required"),
        "{}",
        combined(&output)
    );
    assert!(accepted_facts(&events(&home)).is_empty());
}

#[test]
fn cli_target_or_source_without_inject_is_rejected() {
    let cwd = unique_dir("inject-option-guard");
    let home = isolated_home();
    let output = kiana_in(
        &cwd,
        &home,
        &[
            "run",
            "--json",
            "--target",
            "next-turn",
            "--",
            "no inject flag",
        ],
    );
    assert!(!output.status.success());
    assert!(
        combined(&output).contains("inject_options_require_inject"),
        "{}",
        combined(&output)
    );
    assert!(accepted_facts(&events(&home)).is_empty());
}

#[test]
fn cli_steer_on_current_turn_is_queued_and_records_accepted_fact() {
    let script = harness_script(r#"[{"text":"mapped"}]"#);
    let (cwd, home) = trusted_fixture("steer-current");
    let first = kiana_with_script(&cwd, &home, &script, &["run", "--json", "map the harness"]);
    assert!(first.status.success(), "{}", combined(&first));
    let run: Value = serde_json::from_slice(&first.stdout).unwrap();
    let run_id = run["output"]["run_id"].as_str().unwrap().to_owned();
    // Core 从 run 请求的 request_id 派生当前 turn；CLI 响应回显同一个 request_id。
    let turn = turn_id_for_request(&run);

    let output = kiana_with_script(
        &cwd,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--steer",
            run_id.as_str(),
            "--turn",
            turn.as_str(),
            "--",
            "summarize the query crate",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["queued"], true);
    assert_eq!(response["output"]["target"], "next-step");
    assert!(response["output"]["input_id"].as_str().is_some());
    let facts = events(&home);
    assert!(facts.iter().any(|event| {
        event["kind"] == "run.input.accepted"
            && event["data"]["source"] == "steer"
            && event["data"]["target"] == "next-step"
            && event["data"]["text"]
                .as_str()
                .is_some_and(|text| text.contains("summarize the query crate"))
    }));
}

#[test]
fn cli_inject_queues_source_labelled_input_for_next_turn() {
    let script = harness_script(r#"[{"text":"mapped"}]"#);
    let (cwd, home) = trusted_fixture("inject-next-turn");
    let first = kiana_with_script(&cwd, &home, &script, &["run", "--json", "map the harness"]);
    assert!(first.status.success(), "{}", combined(&first));
    let run: Value = serde_json::from_slice(&first.stdout).unwrap();
    let run_id = run["output"]["run_id"].as_str().unwrap().to_owned();

    let output = kiana_with_script(
        &cwd,
        &home,
        &script,
        &[
            "run",
            "--json",
            "--inject",
            run_id.as_str(),
            "--target",
            "next-turn",
            "--source",
            "cli-check",
            "--",
            "queue this for the next turn",
        ],
    );
    assert!(output.status.success(), "{}", combined(&output));
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["status"], "completed");
    assert_eq!(response["output"]["queued"], true);
    assert_eq!(response["output"]["target"], "next-turn");
    let facts = events(&home);
    assert!(facts.iter().any(|event| {
        event["kind"] == "run.input.accepted"
            && event["data"]["source"] == "cli-check"
            && event["data"]["target"] == "next-turn"
    }));
}
