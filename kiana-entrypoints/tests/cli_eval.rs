use serde_json::Value;
use std::fs;
use std::process::Command;

#[test]
fn eval_help_and_inline_fixture_route_through_the_real_binary() {
    let help = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args(["eval", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success(), "{}", stderr(&help));
    let help_stdout = String::from_utf8(help.stdout).unwrap();
    assert!(
        help_stdout.contains("Usage: kiana eval run"),
        "{help_stdout}"
    );

    let tmp = std::env::temp_dir().join(format!("kiana-cli-eval-{}", std::process::id()));
    fs::create_dir_all(&tmp).unwrap();
    fs::write(
        tmp.join("basic-tool-success.jsonl"),
        concat!(
            r#"{"type":"assistant","assistant_text":"starting"}"#,
            "\n",
            r#"{"type":"tool_call","tool_name":"Read","tool_call_id":"call-1"}"#,
            "\n",
            r#"{"type":"tool_result","tool_name":"Read","tool_call_id":"call-1","is_error":false}"#,
            "\n",
            r#"{"type":"usage","input_tokens":12,"output_tokens":4}"#,
            "\n",
            r#"{"type":"result","status":"completed","stop_reason":"end_turn","assistant_text":"done"}"#,
            "\n",
        ),
    )
    .unwrap();
    fs::write(
        tmp.join("basic-runtime-suite.json"),
        r#"{
  "schema": "kiana.eval-suite.v1",
  "id": "basic-runtime",
  "description": "Offline RuntimeEvent replay contract",
  "cases": [
    {
      "id": "tool-success",
      "kind": "runtime_event_replay",
      "fixture": "basic-tool-success.jsonl",
      "expect": {
        "final_status": "completed",
        "stop_reason": "end_turn",
        "min_event_count": 5,
        "tool_call_count": 1,
        "tool_error_count": 0,
        "required_tool_names": ["Read"],
        "final_text_contains": ["done"],
        "max_input_tokens": 32,
        "max_output_tokens": 16
      }
    }
  ]
}
"#,
    )
    .unwrap();
    let suite = tmp.join("basic-runtime-suite.json");
    let output = Command::new(env!("CARGO_BIN_EXE_kiana"))
        .args([
            "eval",
            "run",
            "--suite",
            suite.to_str().unwrap(),
            "--json",
            "--fail-on-failure",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", stderr(&output));

    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "kiana.eval-report.v1");
    assert_eq!(report["suite_id"], "basic-runtime");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["summary"]["total"], 1);
    assert_eq!(report["summary"]["failed"], 0);
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
