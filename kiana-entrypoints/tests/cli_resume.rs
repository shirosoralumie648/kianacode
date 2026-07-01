use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

#[test]
fn resume_without_prompt_uses_shared_session_show_output() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-resume-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::write(
        sessions_dir.join("session-one.json"),
        r#"{
  "session_id": "session-one",
  "title": "Resume title",
  "tag": null,
  "parent_session_id": null,
  "created_at": 1,
  "updated_at": 2,
  "messages": [
    {"role": "user", "content": "hello resume"}
  ]
}"#,
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .arg("--resume")
        .arg("session-one")
        .env("KIANA_SDK_SESSIONS_DIR", &sessions_dir)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&root);

    assert!(
        output.status.success(),
        "kiana --resume failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("\"session_id\": \"session-one\""),
        "{stdout}"
    );
    assert!(stdout.contains("\"title\": \"Resume title\""), "{stdout}");
    assert!(stdout.contains("\"content\": \"hello resume\""), "{stdout}");
    assert!(!stdout.contains("\"session\": {"), "{stdout}");
}

#[test]
fn resume_without_prompt_reads_jsonl_only_session_tree() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-resume-jsonl-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    let event_dir = sessions_dir.join("jsonl-session");
    fs::create_dir_all(&event_dir).unwrap();
    let events = [
        kiana_types::sdk_message_to_runtime_event(
            "jsonl-session",
            "turn-0",
            None,
            0,
            "10",
            json!({
                "role": "user",
                "content": "resume from event tree",
                "created_at": 10
            }),
        ),
        kiana_types::sdk_message_to_runtime_event(
            "jsonl-session",
            "turn-1",
            Some("turn-0".to_string()),
            1,
            "11",
            json!({
                "role": "assistant",
                "content": [{"type": "text", "text": "event tree answer"}],
                "created_at": 11
            }),
        ),
    ];
    fs::write(
        event_dir.join("events.jsonl"),
        events
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
            .join("\n")
            + "\n",
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .arg("--resume")
        .arg("jsonl-session")
        .env("KIANA_SDK_SESSIONS_DIR", &sessions_dir)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&root);

    assert!(
        output.status.success(),
        "kiana --resume failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("\"session_id\": \"jsonl-session\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"content\": \"resume from event tree\""),
        "{stdout}"
    );
    assert!(stdout.contains("event tree answer"), "{stdout}");
}

#[test]
fn continue_without_prompt_uses_latest_session_from_current_cwd() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "kiana-entrypoints-cli-continue-cwd-{}-{unique}",
        std::process::id()
    ));
    let sessions_dir = root.join("sdk-sessions");
    let current_project = root.join("current-project");
    let other_project = root.join("other-project");
    fs::create_dir_all(&sessions_dir).unwrap();
    fs::create_dir_all(&current_project).unwrap();
    fs::create_dir_all(&other_project).unwrap();
    fs::write(
        sessions_dir.join("current-session.json"),
        serde_json::to_string_pretty(&json!({
            "session_id": "current-session",
            "title": "Current project",
            "tag": null,
            "parent_session_id": null,
            "created_at": 1,
            "updated_at": 10,
            "cwd": current_project.to_string_lossy().to_string(),
            "messages": [
                {"role": "user", "content": "current project prompt"},
                {"role": "assistant", "content": [{"type": "text", "text": "current project reply"}]}
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        sessions_dir.join("other-session.json"),
        serde_json::to_string_pretty(&json!({
            "session_id": "other-session",
            "title": "Other project",
            "tag": null,
            "parent_session_id": null,
            "created_at": 1,
            "updated_at": 20,
            "cwd": other_project.to_string_lossy().to_string(),
            "messages": [
                {"role": "user", "content": "other project prompt"},
                {"role": "assistant", "content": [{"type": "text", "text": "other project reply"}]}
            ]
        }))
        .unwrap(),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .arg("--continue")
        .current_dir(&current_project)
        .env("KIANA_SDK_SESSIONS_DIR", &sessions_dir)
        .output()
        .unwrap();

    let _ = fs::remove_dir_all(&root);

    assert!(
        output.status.success(),
        "kiana --continue failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("\"session_id\": \"current-session\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"title\": \"Current project\""),
        "{stdout}"
    );
    assert!(!stdout.contains("other-session"), "{stdout}");
}
