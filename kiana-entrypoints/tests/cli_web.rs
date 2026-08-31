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
    assert_eq!(health["streaming"], false);

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
