use std::fs;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{extract::State, http::StatusCode, response::IntoResponse, routing::post, Json, Router};
use serde_json::json;

#[test]
fn session_list_uses_shared_session_command_output() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-list-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::write(
        sessions_dir.join("session-one.json"),
        r#"{
  "session_id": "session-one",
  "title": "Useful session",
  "tag": "repl",
  "parent_session_id": null,
  "created_at": 1,
  "updated_at": 2,
  "messages": [
    {"role": "user", "content": "hello"},
    {"role": "assistant", "content": [{"type": "text", "text": "hi"}]}
  ]
}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .arg("session")
        .arg("list")
        .env("KIANA_SDK_SESSIONS_DIR", &sessions_dir)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&root);

    assert!(
        output.status.success(),
        "kiana session list failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("1 SDK session(s):"), "{stdout}");
    assert!(stdout.contains("session-one"), "{stdout}");
    assert!(stdout.contains("messages=2"), "{stdout}");
    assert!(stdout.contains("usage: kiana session show"), "{stdout}");
}

#[test]
fn session_management_subcommands_use_shared_session_command_output() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-management-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::write(
        sessions_dir.join("session-one.json"),
        r#"{
  "session_id": "session-one",
  "title": "Old title",
  "tag": null,
  "parent_session_id": null,
  "created_at": 1,
  "updated_at": 2,
  "messages": [
    {"role": "user", "content": "hello"}
  ]
}"#,
    )
    .unwrap();

    let show = run_kiana_session(&sessions_dir, &["show", "session-one"]);
    assert!(show.status.success(), "{}", show.stderr);
    assert!(
        show.stdout.trim_start().starts_with("{\n  \"created_at\"")
            || show.stdout.trim_start().starts_with("{\n  \"messages\"")
            || show.stdout.contains("\"session_id\": \"session-one\""),
        "{}",
        show.stdout
    );
    assert!(!show.stdout.contains("\"session\": {"), "{}", show.stdout);

    let rename = run_kiana_session(&sessions_dir, &["rename", "session-one", "New", "title"]);
    assert!(rename.status.success(), "{}", rename.stderr);
    assert!(
        rename.stdout.contains("Session renamed"),
        "{}",
        rename.stdout
    );
    assert!(
        rename.stdout.contains("title: New title"),
        "{}",
        rename.stdout
    );

    let tag = run_kiana_session(&sessions_dir, &["tag", "session-one", "important"]);
    assert!(tag.status.success(), "{}", tag.stderr);
    assert!(tag.stdout.contains("Session tagged"), "{}", tag.stdout);
    assert!(tag.stdout.contains("tag: important"), "{}", tag.stdout);

    let fork = run_kiana_session(&sessions_dir, &["fork", "session-one"]);
    assert!(fork.status.success(), "{}", fork.stderr);
    assert!(fork.stdout.contains("Session forked"), "{}", fork.stdout);
    assert!(
        fork.stdout.contains("source: session-one"),
        "{}",
        fork.stdout
    );
    let forked_id = fork
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("forked: "))
        .expect("forked id in output");
    let forked_session: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(sessions_dir.join(format!("{forked_id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(forked_session["parent_session_id"], "session-one");
    let forked_events = read_event_lines(&sessions_dir, forked_id);
    assert_eq!(forked_events.len(), 1);
    assert_eq!(forked_events[0]["type"], "user_message");
    assert_eq!(forked_events[0]["message"]["content"], "hello");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn session_export_routes_to_shared_export_command() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-export-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::write(
        sessions_dir.join("session-one.json"),
        r#"{
  "session_id": "session-one",
  "title": "Export title",
  "tag": null,
  "parent_session_id": null,
  "created_at": 1,
  "updated_at": 2,
  "messages": [
    {"role": "user", "content": "hello export"},
    {"role": "assistant", "content": [{"type": "text", "text": "export reply"}]}
  ]
}"#,
    )
    .unwrap();

    let export = run_kiana_session(&sessions_dir, &["export", "session-one", "--text"]);

    let _ = fs::remove_dir_all(&root);

    assert!(export.status.success(), "{}", export.stderr);
    assert!(
        export.stdout.contains("# Kiana Conversation Export"),
        "{}",
        export.stdout
    );
    assert!(
        export.stdout.contains("session_id: session-one"),
        "{}",
        export.stdout
    );
    assert!(export.stdout.contains("hello export"), "{}", export.stdout);
    assert!(export.stdout.contains("export reply"), "{}", export.stdout);
}

#[test]
fn session_import_restores_legacy_json_and_runtime_events() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-import-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir_all(&root).unwrap();
    let sessions_dir = root.join("sdk-sessions");
    let import_path = root.join("exported-session.json");
    fs::write(
        &import_path,
        serde_json::to_string_pretty(&json!({
            "session_id": "imported-session",
            "title": "Imported title",
            "tag": null,
            "parent_session_id": null,
            "created_at": 1,
            "updated_at": 2,
            "messages": [
                {"role": "user", "content": "hello import", "created_at": 1},
                {"role": "assistant", "content": [{"type": "text", "text": "import reply"}], "created_at": 2}
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let import_path = import_path.to_string_lossy().to_string();
    let import = run_kiana_session(&sessions_dir, &["import", &import_path]);
    assert!(import.status.success(), "{}", import.stderr);
    assert!(
        import.stdout.contains("Session imported"),
        "{}",
        import.stdout
    );
    assert!(
        import.stdout.contains("id: imported-session"),
        "{}",
        import.stdout
    );
    let session: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(sessions_dir.join("imported-session.json")).unwrap(),
    )
    .unwrap();
    let events = read_event_lines(&sessions_dir, "imported-session");

    let _ = fs::remove_dir_all(&root);

    assert_eq!(session["session_id"], "imported-session");
    assert_eq!(session["messages"].as_array().unwrap().len(), 2);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["type"], "user_message");
    assert_eq!(events[0]["sequence"], 0);
    assert_eq!(events[0]["message"]["content"], "hello import");
    assert_eq!(events[1]["type"], "assistant_message");
    assert_eq!(events[1]["sequence"], 1);
    assert_eq!(events[1]["parent_turn_id"], "turn-0");
    assert_eq!(events[1]["message"]["content"][0]["text"], "import reply");
}

#[test]
fn session_compact_routes_to_shared_compact_command() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-compact-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    let long_text = "detail ".repeat(160);
    fs::write(
        sessions_dir.join("session-one.json"),
        serde_json::to_string_pretty(&json!({
            "session_id": "session-one",
            "title": "Compact title",
            "tag": null,
            "parent_session_id": null,
            "created_at": 1,
            "updated_at": 2,
            "messages": [
                {"role": "user", "content": format!("first {long_text}")},
                {"role": "assistant", "content": [{"type": "text", "text": format!("second {long_text}")}]},
                {"role": "user", "content": format!("third {long_text}")},
                {"role": "assistant", "content": [{"type": "text", "text": format!("fourth {long_text}")}]},
                {"role": "user", "content": "recent tail"}
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let compact = run_kiana_session(
        &sessions_dir,
        &[
            "compact",
            "session-one",
            "--dry-run",
            "--threshold-tokens",
            "1",
            "--target-tokens",
            "80",
        ],
    );

    let _ = fs::remove_dir_all(&root);

    assert!(compact.status.success(), "{}", compact.stderr);
    assert!(
        compact.stdout.contains("Session compact dry run"),
        "{}",
        compact.stdout
    );
    assert!(
        compact.stdout.contains("id: session-one"),
        "{}",
        compact.stdout
    );
}

#[test]
fn session_reply_record_only_uses_shared_session_command_output() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-reply-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::write(
        sessions_dir.join("session-one.json"),
        r#"{
  "session_id": "session-one",
  "title": "Reply title",
  "tag": null,
  "parent_session_id": null,
  "created_at": 1,
  "updated_at": 2,
  "messages": [
    {"role": "user", "content": "first"}
  ]
}"#,
    )
    .unwrap();

    let reply = run_kiana_session(
        &sessions_dir,
        &["reply", "session-one", "--record-only", "follow", "up"],
    );
    let session = fs::read_to_string(sessions_dir.join("session-one.json")).unwrap();
    let events = read_event_lines(&sessions_dir, "session-one");

    let _ = fs::remove_dir_all(&root);

    assert!(reply.status.success(), "{}", reply.stderr);
    assert!(
        reply.stdout.contains("Session reply recorded"),
        "{}",
        reply.stdout
    );
    assert!(reply.stdout.contains("id: session-one"), "{}", reply.stdout);
    assert!(reply.stdout.contains("messages: 2"), "{}", reply.stdout);
    assert!(
        !reply.stdout.contains("sdk_prompt_recorded"),
        "{}",
        reply.stdout
    );
    assert!(session.contains("\"content\": \"follow up\""), "{session}");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["type"], "user_message");
    assert_eq!(events[0]["message"]["content"], "first");
    assert_eq!(events[1]["type"], "user_message");
    assert_eq!(events[1]["sequence"], 1);
    assert_eq!(events[1]["parent_turn_id"], "turn-0");
    assert_eq!(events[1]["message"]["content"], "follow up");
}

#[tokio::test]
async fn session_reply_model_execution_prints_assistant_text() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-session-reply-model-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::write(
        sessions_dir.join("session-one.json"),
        r#"{
  "session_id": "session-one",
  "title": "Reply model title",
  "tag": null,
  "parent_session_id": null,
  "created_at": 1,
  "updated_at": 2,
  "messages": [
    {"role": "user", "content": "first"}
  ]
}"#,
    )
    .unwrap();
    let (base_url, state, server) = start_session_reply_mock_model_server().await;

    let output_sessions_dir = sessions_dir.clone();
    let output = tokio::task::spawn_blocking(move || {
        Command::new(env!("CARGO_BIN_EXE_kiana"))
            .arg("--base-url")
            .arg(base_url)
            .arg("--model")
            .arg("mock-session-reply-model")
            .arg("session")
            .arg("reply")
            .arg("session-one")
            .arg("follow")
            .arg("up")
            .env("ANTHROPIC_API_KEY", "test-key")
            .env("KIANA_SDK_SESSIONS_DIR", &output_sessions_dir)
            .output()
            .unwrap()
    })
    .await
    .unwrap();

    server.abort();
    let session = fs::read_to_string(sessions_dir.join("session-one.json")).unwrap();
    let requests = state.lock().unwrap().requests.clone();
    let _ = fs::remove_dir_all(&root);

    assert!(
        output.status.success(),
        "kiana session reply failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "session reply complete"
    );
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["model"], "mock-session-reply-model");
    assert_eq!(
        requests[0]["messages"].as_array().unwrap()[1]["content"],
        "follow up"
    );
    assert!(session.contains("\"content\": \"follow up\""), "{session}");
    assert!(
        session.contains("\"text\": \"session reply complete\""),
        "{session}"
    );
}

struct KianaOutput {
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

#[derive(Default)]
struct SessionReplyMockState {
    requests: Vec<serde_json::Value>,
}

async fn start_session_reply_mock_model_server() -> (
    String,
    Arc<Mutex<SessionReplyMockState>>,
    tokio::task::JoinHandle<()>,
) {
    async fn handle_request(
        State(state): State<Arc<Mutex<SessionReplyMockState>>>,
        Json(body): Json<serde_json::Value>,
    ) -> impl IntoResponse {
        state.lock().unwrap().requests.push(body);
        (
            StatusCode::OK,
            Json(json!({
                "id": "msg_session_reply",
                "model": "mock-session-reply-model",
                "role": "assistant",
                "content": [{
                    "type": "text",
                    "text": "session reply complete"
                }],
                "stop_reason": "end_turn",
                "usage": {
                    "input_tokens": 1,
                    "output_tokens": 1
                }
            })),
        )
            .into_response()
    }

    let state = Arc::new(Mutex::new(SessionReplyMockState::default()));
    let app = Router::new()
        .route("/v1/messages", post(handle_request))
        .with_state(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (format!("http://{}", addr), state, server)
}

fn run_kiana_session(sessions_dir: &std::path::Path, args: &[&str]) -> KianaOutput {
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .arg("session")
        .args(args)
        .env("KIANA_SDK_SESSIONS_DIR", sessions_dir)
        .output()
        .unwrap();

    KianaOutput {
        status: output.status,
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

fn read_event_lines(sessions_dir: &std::path::Path, session_id: &str) -> Vec<serde_json::Value> {
    fs::read_to_string(sessions_dir.join(session_id).join("events.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}
