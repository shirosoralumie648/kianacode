use futures_util::StreamExt;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

fn unique_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "kiana-cli-web-{label}-{}-{stamp}",
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

fn apply_patch_cassette() -> &'static str {
    r#"[{"text":"writing","tool_calls":[{"id":"c1","name":"apply_patch","arguments":{"patch":"*** Begin Patch\n*** Add File: GOLDEN_PATH.txt\n+hello\n*** End Patch\n"}}]},{"text":"created GOLDEN_PATH.txt"}]"#
}

fn harness_script() -> PathBuf {
    let path = unique_dir("script").join("script.json");
    fs::write(&path, apply_patch_cassette()).unwrap();
    path
}

fn kiana_base(cwd: &Path, home: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_kiana"));
    command
        .current_dir(cwd)
        .env("KIANA_HOME", home)
        .env("HOME", home)
        .env_remove("KIANA_PROVIDER")
        .env_remove("KIANA_FAKE_PROVIDER_SCRIPT")
        .env_remove("ANTHROPIC_API_KEY")
        .env_remove("KIANA_OPENAI_API_KEY")
        .env_remove("OPENAI_API_KEY");
    command
}

#[test]
fn web_help_does_not_need_a_model() {
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_base(&cwd, &home)
        .args(["web", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("kiana web"), "{text}");
    assert!(text.contains("DaemonHost"), "{text}");
    assert!(text.to_ascii_lowercase().contains("loopback"), "{text}");
}

#[test]
fn top_level_help_lists_web() {
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_base(&cwd, &home).args(["--help"]).output().unwrap();
    assert!(output.status.success(), "{:?}", output);
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("kiana web"), "{text}");
    assert!(text.contains("Loopback Web workbench"), "{text}");
}

#[test]
fn web_rejects_non_loopback_bind() {
    let cwd = unique_dir("cwd");
    let home = isolated_home();
    let output = kiana_base(&cwd, &home)
        .args(["web", "--no-open", "--bind", "0.0.0.0:3080"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("bind_loopback_only"), "{text}");
}

#[tokio::test]
async fn web_cassette_writes_through_daemon_host() {
    let root = git_fixture();
    let home = isolated_home();
    let script = harness_script();
    let mut child = TokioCommand::from(kiana_base(&root, &home))
        .env("KIANA_HARNESS_SCRIPT", &script)
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().expect("stdout");
    let url = wait_for_url(stdout).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let health: Value = client
        .get(format!("{url}/api/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["harness"], "kiana-harness");
    assert_eq!(health["streaming"], true);
    assert_eq!(health["streaming_transport"], "sse");

    let page = client
        .get(format!("{url}/"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let token = page
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("web token")
        .to_owned();
    let unauthorized = client.get(format!("{url}/api/state")).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let auth = |request: reqwest::RequestBuilder| request.header("x-kiana-web-token", &token);

    let trusted: Value = auth(
        client
            .post(format!("{url}/api/trust"))
            .json(&serde_json::json!({})),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(trusted["trusted"], true, "{trusted}");

    let oversized_body = format!(r#"{{"prompt":"{}"}}"#, "x".repeat(128 * 1024));
    let oversized_body_response = auth(
        client
            .post(format!("{url}/api/run"))
            .header("content-type", "application/json")
            .body(oversized_body),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(
        oversized_body_response.status(),
        reqwest::StatusCode::PAYLOAD_TOO_LARGE
    );

    let oversized = auth(
        client
            .post(format!("{url}/api/run"))
            .json(&serde_json::json!({ "prompt": "x".repeat(64 * 1024 + 1) })),
    )
    .send()
    .await
    .unwrap();
    assert_eq!(oversized.status(), reqwest::StatusCode::BAD_REQUEST);
    let oversized: Value = oversized.json().await.unwrap();
    assert_eq!(oversized["error"], "prompt_too_large");

    let run: Value = auth(
        client
            .post(format!("{url}/api/run"))
            .json(&serde_json::json!({
                "prompt": "create GOLDEN_PATH.txt containing hello"
            })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(run["response"]["status"], "completed", "{run}");
    assert_eq!(
        fs::read_to_string(root.join("GOLDEN_PATH.txt"))
            .unwrap()
            .trim(),
        "hello"
    );
    let _ = child.kill().await;
}

#[tokio::test]
async fn web_sse_streams_ordered_deltas_and_terminal_with_auth() {
    let root = git_fixture();
    let home = isolated_home();
    let script = harness_script();
    let mut child = TokioCommand::from(kiana_base(&root, &home))
        .env("KIANA_HARNESS_SCRIPT", &script)
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let url = wait_for_url(child.stdout.take().expect("stdout")).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let page = client.get(&url).send().await.unwrap().text().await.unwrap();
    let token = page
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("web token")
        .to_owned();
    let auth = |request: reqwest::RequestBuilder| request.header("x-kiana-web-token", &token);

    let state: Value = auth(client.get(format!("{url}/api/state")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_id = state["session_id"].as_str().expect("session id").to_owned();
    let events_base = format!(
        "{url}/api/events?session_id={}",
        urlencoding_encode(&session_id)
    );
    let events_url = format!("{events_base}&token={}", urlencoding_encode(&token));

    let unauthorized = client.get(&events_base).send().await.unwrap();
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let wrong_host = client
        .get(&events_url)
        .header("host", "attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), reqwest::StatusCode::UNAUTHORIZED);

    let wrong_origin = client
        .get(&events_url)
        .header("origin", "https://attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_origin.status(), reqwest::StatusCode::UNAUTHORIZED);

    let (sse_ready_tx, sse_ready_rx) = tokio::sync::oneshot::channel();
    let sse_task = tokio::spawn({
        let client = client.clone();
        let events_url = events_url.clone();
        async move {
            let response = client.get(events_url).send().await.unwrap();
            assert_eq!(response.status(), reqwest::StatusCode::OK);
            assert!(response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.starts_with("text/event-stream")));
            let _ = sse_ready_tx.send(());
            response.text().await.unwrap()
        }
    });
    sse_ready_rx.await.expect("SSE headers");

    let trusted: Value = auth(
        client
            .post(format!("{url}/api/trust"))
            .json(&serde_json::json!({ "session_id": session_id })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(trusted["trusted"], true, "{trusted}");

    let run: Value = auth(
        client
            .post(format!("{url}/api/run"))
            .json(&serde_json::json!({
                "prompt": "create GOLDEN_PATH.txt containing hello",
                "session_id": session_id,
            })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(run["response"]["status"], "completed", "{run}");

    let body = tokio::time::timeout(Duration::from_secs(10), sse_task)
        .await
        .expect("SSE terminal timeout")
        .unwrap();
    let events = parse_sse_events(&body);
    let deltas = events
        .iter()
        .filter(|(name, _)| name == "delta")
        .map(|(_, data)| {
            data["event"]["text"]
                .as_str()
                .expect("delta text")
                .to_owned()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        deltas,
        vec!["writing".to_owned(), "created GOLDEN_PATH.txt".to_owned()],
        "{events:?}"
    );
    let (terminal_name, terminal) = events.last().expect("terminal event");
    assert_eq!(terminal_name, "terminal", "{events:?}");
    assert_eq!(
        terminal["event"]["response"]["status"], "completed",
        "{terminal}"
    );

    let _ = child.kill().await;
}

#[tokio::test]
async fn web_sse_reconnect_emits_stream_gap_without_replaying_delta_items() {
    let root = git_fixture();
    let home = isolated_home();
    let script = unique_dir("reconnect-script").join("script.json");
    fs::write(
        &script,
        r#"[{"text":"first chunk","tool_calls":[{"id":"c1","name":"shell","arguments":{"command":"sleep 2"}}]},{"text":" second chunk"}]"#,
    )
    .unwrap();
    let mut child = TokioCommand::from(kiana_base(&root, &home))
        .env("KIANA_HARNESS_SCRIPT", &script)
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let url = wait_for_url(child.stdout.take().expect("stdout")).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let page = client.get(&url).send().await.unwrap().text().await.unwrap();
    let token = page
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("web token")
        .to_owned();
    let auth = |request: reqwest::RequestBuilder| request.header("x-kiana-web-token", &token);

    let state: Value = auth(client.get(format!("{url}/api/state")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_id = state["session_id"].as_str().expect("session id").to_owned();
    let events_url = format!(
        "{url}/api/events?session_id={}&token={}",
        urlencoding_encode(&session_id),
        urlencoding_encode(&token)
    );
    let trusted: Value = auth(
        client
            .post(format!("{url}/api/trust"))
            .json(&serde_json::json!({ "session_id": session_id })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(trusted["trusted"], true, "{trusted}");

    let first_response = client.get(&events_url).send().await.unwrap();
    assert_eq!(first_response.status(), reqwest::StatusCode::OK);
    let mut first_stream = first_response.bytes_stream();
    let mut first_buffer = Vec::new();

    let run_task = tokio::spawn({
        let client = client.clone();
        let url = url.clone();
        let token = token.clone();
        let session_id = session_id.clone();
        async move {
            client
                .post(format!("{url}/api/run"))
                .header("x-kiana-web-token", token)
                .json(&serde_json::json!({
                    "prompt": "stream then sleep",
                    "session_id": session_id,
                }))
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    });

    let (first_name, first_event) = tokio::time::timeout(
        Duration::from_secs(10),
        next_sse_event(&mut first_stream, &mut first_buffer),
    )
    .await
    .expect("first SSE delta timeout");
    assert_eq!(first_name, "delta", "{first_event}");
    assert_eq!(first_event["event"]["text"], "first chunk", "{first_event}");
    drop(first_stream);

    let second_response = client.get(&events_url).send().await.unwrap();
    assert_eq!(second_response.status(), reqwest::StatusCode::OK);
    let mut second_stream = second_response.bytes_stream();
    let mut second_buffer = Vec::new();
    let mut second_events = Vec::new();
    loop {
        let event = tokio::time::timeout(
            Duration::from_secs(10),
            next_sse_event(&mut second_stream, &mut second_buffer),
        )
        .await
        .expect("reconnected SSE event timeout");
        let terminal = event.0 == "terminal";
        second_events.push(event);
        if terminal {
            break;
        }
    }

    let run = run_task.await.unwrap();
    assert_eq!(run["response"]["status"], "completed", "{run}");
    assert_eq!(
        second_events.first().map(|event| event.0.as_str()),
        Some("stream_gap"),
        "{second_events:?}"
    );
    assert_eq!(
        second_events[0].1["run_id"], session_id,
        "{second_events:?}"
    );
    assert_eq!(
        second_events[0].1["reason"], "subscription_attached_after_run_started",
        "{second_events:?}"
    );
    let replayed_first_chunks = second_events
        .iter()
        .filter(|(name, data)| name == "delta" && data["event"]["text"] == "first chunk")
        .count();
    assert_eq!(
        replayed_first_chunks, 0,
        "reconnect must not replay already-delivered delta items: {second_events:?}"
    );
    let reconnected_deltas = second_events
        .iter()
        .filter(|(name, _)| name == "delta")
        .map(|(_, data)| data["event"]["text"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(
        reconnected_deltas,
        vec![" second chunk".to_owned()],
        "{second_events:?}"
    );
    let (terminal_name, terminal) = second_events.last().expect("terminal event");
    assert_eq!(terminal_name, "terminal", "{second_events:?}");
    assert_eq!(
        terminal["event"]["response"]["status"], "completed",
        "{terminal}"
    );

    let _ = child.kill().await;
}

#[tokio::test]
async fn web_sse_unknown_session_connection_fails_closed_with_explicit_error() {
    let root = git_fixture();
    let home = isolated_home();
    let mut child = TokioCommand::from(kiana_base(&root, &home))
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let url = wait_for_url(child.stdout.take().expect("stdout")).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let page = client.get(&url).send().await.unwrap().text().await.unwrap();
    let token = page
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("web token");

    let response = client
        .get(format!(
            "{url}/api/events?session_id=missing-session&token={}",
            urlencoding_encode(token)
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body["error"], "session_unknown", "{body}");

    let _ = child.kill().await;
}

fn urlencoding_encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

fn parse_sse_events(body: &str) -> Vec<(String, Value)> {
    let mut events = Vec::new();
    let mut name = None;
    let mut data = String::new();
    for line in body.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let (Some(name), false) = (name.take(), data.is_empty()) {
                if let Ok(data) = serde_json::from_str(&data) {
                    events.push((name, data));
                }
            }
            data.clear();
        } else if let Some(value) = line.strip_prefix("event:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
    events
}

async fn next_sse_event<S, B, E>(stream: &mut S, buffer: &mut Vec<u8>) -> (String, Value)
where
    S: futures_util::Stream<Item = Result<B, E>> + Unpin,
    B: AsRef<[u8]>,
    E: std::fmt::Debug,
{
    loop {
        if let Some(event) = take_sse_event(buffer) {
            return event;
        }
        let chunk = stream
            .next()
            .await
            .expect("SSE stream ended before the next event")
            .expect("SSE chunk");
        buffer.extend_from_slice(chunk.as_ref());
    }
}

fn take_sse_event(buffer: &mut Vec<u8>) -> Option<(String, Value)> {
    let boundary = buffer.windows(4).position(|window| window == b"\r\n\r\n");
    let (boundary, width) = match boundary {
        Some(boundary) => (boundary, 4),
        None => (buffer.windows(2).position(|window| window == b"\n\n")?, 2),
    };
    let block = buffer.drain(..boundary + width).collect::<Vec<_>>();
    let block = String::from_utf8_lossy(&block);
    let mut name = None;
    let mut data = String::new();
    for line in block.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with(':') {
            continue;
        }
        if let Some(value) = line.strip_prefix("event:") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }
    match (name, data.is_empty()) {
        (Some(name), false) => serde_json::from_str(&data).ok().map(|data| (name, data)),
        _ => None,
    }
}

#[tokio::test]
async fn web_rejects_wrong_origin_and_host_without_mutating_trust() {
    let root = git_fixture();
    let home = isolated_home();
    let mut child = TokioCommand::from(kiana_base(&root, &home))
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().expect("stdout");
    let url = wait_for_url(stdout).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let page = client.get(&url).send().await.unwrap().text().await.unwrap();
    let token = page
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("web token");
    let bound_port = reqwest::Url::parse(&url)
        .unwrap()
        .port()
        .expect("ephemeral web port");
    let wrong_port = if bound_port == 1 { 2 } else { 1 };

    let wrong_loopback_index = client
        .get(&url)
        .header("host", format!("127.0.0.1:{wrong_port}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_loopback_index.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let wrong_loopback_origin = client
        .post(format!("{url}/api/trust"))
        .header("x-kiana-web-token", token)
        .header("origin", format!("http://127.0.0.1:{wrong_port}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_loopback_origin.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let wrong_loopback_host = client
        .post(format!("{url}/api/trust"))
        .header("x-kiana-web-token", token)
        .header("host", format!("127.0.0.1:{wrong_port}"))
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_loopback_host.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );

    let wrong_origin = client
        .post(format!("{url}/api/trust"))
        .header("x-kiana-web-token", token)
        .header("origin", "https://attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_origin.status(), reqwest::StatusCode::UNAUTHORIZED);

    let wrong_host = client
        .post(format!("{url}/api/trust"))
        .header("x-kiana-web-token", token)
        .header("host", "attacker.example")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), reqwest::StatusCode::UNAUTHORIZED);

    let state: Value = client
        .get(format!("{url}/api/state"))
        .header("x-kiana-web-token", token)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state["trusted"], false, "{state}");

    let trusted: Value = client
        .post(format!("{url}/api/trust"))
        .header("x-kiana-web-token", token)
        .header("origin", &url)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(trusted["trusted"], true, "{trusted}");
    let _ = child.kill().await;
}

#[tokio::test]
async fn web_rejects_foreign_bearers_and_sessions_without_mutation() {
    let root_a = git_fixture();
    let home_a = isolated_home();
    let mut child_a = TokioCommand::from(kiana_base(&root_a, &home_a))
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root_a)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let url_a = wait_for_url(child_a.stdout.take().expect("first web stdout")).await;

    let root_b = git_fixture();
    let home_b = isolated_home();
    let mut child_b = TokioCommand::from(kiana_base(&root_b, &home_b))
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root_b)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let url_b = wait_for_url(child_b.stdout.take().expect("second web stdout")).await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let page_a = client
        .get(&url_a)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let token_a = page_a
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("first web token")
        .to_owned();
    let page_b = client
        .get(&url_b)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let token_b = page_b
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("second web token")
        .to_owned();
    assert_ne!(token_a, token_b);

    let state_a: Value = client
        .get(format!("{url_a}/api/state"))
        .header("x-kiana-web-token", &token_a)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_a = state_a["session_id"]
        .as_str()
        .expect("first session id")
        .to_owned();
    let state_b: Value = client
        .get(format!("{url_b}/api/state"))
        .header("x-kiana-web-token", &token_b)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_b = state_b["session_id"]
        .as_str()
        .expect("second session id")
        .to_owned();
    assert_ne!(session_a, session_b);

    let no_token = client
        .post(format!("{url_a}/api/trust"))
        .send()
        .await
        .unwrap();
    assert_eq!(no_token.status(), reqwest::StatusCode::UNAUTHORIZED);

    let wrong_token = client
        .post(format!("{url_a}/api/trust"))
        .header("x-kiana-web-token", "wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(wrong_token.status(), reqwest::StatusCode::UNAUTHORIZED);

    let foreign_bearer = client
        .post(format!("{url_b}/api/trust"))
        .header("x-kiana-web-token", &token_a)
        .send()
        .await
        .unwrap();
    assert_eq!(foreign_bearer.status(), reqwest::StatusCode::UNAUTHORIZED);

    let foreign_session = client
        .post(format!("{url_b}/api/run"))
        .header("x-kiana-web-token", &token_b)
        .json(&serde_json::json!({
            "prompt": "write SHOULD_NOT_EXIST.txt",
            "session_id": session_a,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(foreign_session.status(), reqwest::StatusCode::BAD_REQUEST);
    let foreign_session: Value = foreign_session.json().await.unwrap();
    assert_eq!(foreign_session["error"], "session_unknown");

    let final_a: Value = client
        .get(format!("{url_a}/api/state"))
        .header("x-kiana-web-token", &token_a)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let final_b: Value = client
        .get(format!("{url_b}/api/state"))
        .header("x-kiana-web-token", &token_b)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(final_a["trusted"], false, "{final_a}");
    assert_eq!(final_b["trusted"], false, "{final_b}");
    assert_eq!(
        final_a["sessions"].as_array().unwrap().len(),
        1,
        "{final_a}"
    );
    assert_eq!(
        final_b["sessions"].as_array().unwrap().len(),
        1,
        "{final_b}"
    );
    assert_eq!(final_a["session_id"], session_a, "{final_a}");
    assert_eq!(final_b["session_id"], session_b, "{final_b}");
    assert!(!root_a.join("SHOULD_NOT_EXIST.txt").exists());
    assert!(!root_b.join("SHOULD_NOT_EXIST.txt").exists());

    let _ = child_a.kill().await;
    let _ = child_b.kill().await;
}

#[tokio::test]
async fn web_projects_each_run_to_its_requested_session() {
    let root = git_fixture();
    let home = isolated_home();
    let script = unique_dir("script").join("script.json");
    fs::write(
        &script,
        r#"[{"text":"first response"},{"text":"second response"}]"#,
    )
    .unwrap();
    let mut child = TokioCommand::from(kiana_base(&root, &home))
        .env("KIANA_HARNESS_SCRIPT", &script)
        .args(["web", "--no-open", "--bind", "127.0.0.1:0"])
        .arg("--workdir")
        .arg(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    let url = wait_for_url(child.stdout.take().expect("web stdout")).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap();
    let page = client.get(&url).send().await.unwrap().text().await.unwrap();
    let token = page
        .split("window.__KIANA_WEB_TOKEN__ = '")
        .nth(1)
        .and_then(|value| value.split('\'').next())
        .expect("web token")
        .to_owned();
    let auth = |request: reqwest::RequestBuilder| request.header("x-kiana-web-token", &token);

    let initial: Value = auth(client.get(format!("{url}/api/state")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_a = initial["session_id"]
        .as_str()
        .expect("initial session id")
        .to_owned();
    let second: Value = auth(client.post(format!("{url}/api/session")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let session_b = second["session_id"]
        .as_str()
        .expect("second session id")
        .to_owned();
    assert_ne!(session_a, session_b);

    let trusted: Value = auth(
        client
            .post(format!("{url}/api/trust"))
            .json(&serde_json::json!({ "session_id": session_a })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(trusted["session_id"], session_a, "{trusted}");

    let sandbox: Value = auth(
        client
            .post(format!("{url}/api/sandbox"))
            .json(&serde_json::json!({
                "sandbox": "workspace-write",
                "session_id": session_a,
            })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(sandbox["session_id"], session_a, "{sandbox}");

    let run_a: Value = auth(
        client
            .post(format!("{url}/api/run"))
            .json(&serde_json::json!({
                "prompt": "first task",
                "session_id": session_a,
            })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(run_a["session_id"], session_a, "{run_a}");
    assert_eq!(run_a["thread"]["id"], session_a, "{run_a}");
    assert_eq!(run_a["response"]["status"], "completed", "{run_a}");

    let state_b: Value = auth(client.get(format!("{url}/api/state?session_id={session_b}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state_b["session_id"], session_b, "{state_b}");
    assert_eq!(state_b["thread"]["turns"].as_array().unwrap().len(), 0);

    let run_b: Value = auth(
        client
            .post(format!("{url}/api/run"))
            .json(&serde_json::json!({
                "prompt": "second task",
                "session_id": session_b,
            })),
    )
    .send()
    .await
    .unwrap()
    .json()
    .await
    .unwrap();
    assert_eq!(run_b["session_id"], session_b, "{run_b}");
    assert_eq!(run_b["thread"]["id"], session_b, "{run_b}");
    assert_eq!(run_b["thread"]["turns"].as_array().unwrap().len(), 1);

    let state_a: Value = auth(client.get(format!("{url}/api/state?session_id={session_a}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(state_a["session_id"], session_a, "{state_a}");
    assert_eq!(state_a["thread"]["id"], session_a, "{state_a}");
    assert_eq!(state_a["thread"]["turns"].as_array().unwrap().len(), 1);

    let _ = child.kill().await;
}

async fn wait_for_url(stdout: impl tokio::io::AsyncRead + Unpin) -> String {
    let mut lines = BufReader::new(stdout).lines();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    let mut buf = String::new();
    while tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_secs(2), lines.next_line()).await {
            Ok(Ok(Some(line))) => {
                buf.push_str(&line);
                buf.push('\n');
                if let Some(rest) = line.trim().strip_prefix("KIANA_WEB_URL=") {
                    return rest.to_owned();
                }
                if let Some(url) = line
                    .split_whitespace()
                    .find(|part| part.starts_with("http://127.0.0.1"))
                {
                    return url.to_owned();
                }
            }
            Ok(Ok(None)) => break,
            _ => {}
        }
    }
    panic!("web server did not print a loopback url: {buf}");
}
