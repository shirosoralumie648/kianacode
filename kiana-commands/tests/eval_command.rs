use kiana_commands::eval::EvalCommand;
use kiana_commands::{create_default_command_registry, Command, CommandContext};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-eval-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy()))]),
    }
}

fn write_suite(root: &Path, fixture: &str, expect: Value) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("events.jsonl"), fixture).unwrap();
    let suite = root.join("suite.json");
    fs::write(
        &suite,
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.eval-suite.v1",
            "id": "fixture-suite",
            "description": "test suite",
            "cases": [{
                "id": "case-1",
                "kind": "runtime_event_replay",
                "fixture": "events.jsonl",
                "expect": expect,
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    suite
}

fn write_baseline(root: &Path, cases: Value) -> PathBuf {
    let baseline = root.join("baseline.json");
    fs::write(
        &baseline,
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.eval-baseline.v1",
            "suite_id": "fixture-suite",
            "description": "test baseline",
            "cases": cases,
        }))
        .unwrap(),
    )
    .unwrap();
    baseline
}

fn success_fixture() -> &'static str {
    "{\"type\":\"assistant\",\"assistant_text\":\"starting\"}\n\
{\"type\":\"tool_call\",\"tool_name\":\"Read\",\"tool_call_id\":\"call-1\"}\n\
{\"type\":\"tool_result\",\"tool_name\":\"Read\",\"tool_call_id\":\"call-1\",\"is_error\":false}\n\
{\"type\":\"usage\",\"input_tokens\":12,\"output_tokens\":4}\n\
{\"type\":\"result\",\"status\":\"completed\",\"stop_reason\":\"end_turn\",\"assistant_text\":\"done\"}\n"
}

#[test]
fn default_registry_includes_eval_command() {
    assert!(create_default_command_registry().get("eval").is_some());
}

#[tokio::test]
async fn eval_run_reports_deterministic_runtime_metrics() {
    let root = root("pass");
    let suite = write_suite(
        &root,
        success_fixture(),
        json!({
            "final_status": "completed",
            "stop_reason": "end_turn",
            "min_event_count": 5,
            "tool_call_count": 1,
            "tool_error_count": 0,
            "required_tool_names": ["Read"],
            "final_text_contains": ["done"],
            "max_input_tokens": 12,
            "max_output_tokens": 4,
        }),
    );

    let output = EvalCommand
        .execute(context(
            format!("run --suite {} --json", suite.display()),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["schema"], "kiana.eval-report.v1");
    assert_eq!(report["suite_id"], "fixture-suite");
    assert_eq!(report["status"], "passed");
    assert_eq!(report["summary"]["passed"], 1);
    assert_eq!(report["summary"]["failed"], 0);
    assert!(report["baseline"].is_null());
    assert_eq!(report["cases"][0]["metrics"]["event_count"], 5);
    assert_eq!(report["cases"][0]["metrics"]["tool_call_count"], 1);
    assert_eq!(report["cases"][0]["metrics"]["tool_result_count"], 1);
    assert_eq!(report["cases"][0]["metrics"]["tool_error_count"], 0);
    assert_eq!(report["cases"][0]["metrics"]["input_tokens"], 12);
    assert_eq!(report["cases"][0]["metrics"]["output_tokens"], 4);
    assert_eq!(report["cases"][0]["metrics"]["final_text"], "done");
    assert_eq!(report["suite_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(
        report["cases"][0]["fixture_sha256"].as_str().unwrap().len(),
        64
    );
    assert!(!output.value.contains(root.to_string_lossy().as_ref()));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eval_run_compares_local_baseline_thresholds() {
    let root = root("baseline-pass");
    let suite = write_suite(
        &root,
        success_fixture(),
        json!({
            "final_status": "completed",
            "stop_reason": "end_turn",
            "min_event_count": 5,
            "tool_call_count": 1,
            "tool_error_count": 0,
            "max_input_tokens": 12,
            "max_output_tokens": 4,
        }),
    );
    let baseline = write_baseline(
        &root,
        json!({
            "case-1": {
                "max_event_count": 5,
                "max_tool_call_count": 1,
                "max_tool_result_count": 1,
                "max_tool_error_count": 0,
                "max_input_tokens": 12,
                "max_output_tokens": 4,
                "required_status": "completed",
                "required_stop_reason": "end_turn"
            }
        }),
    );

    let output = EvalCommand
        .execute(context(
            format!(
                "run --suite {} --baseline {} --json --fail-on-failure",
                suite.display(),
                baseline.display()
            ),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["status"], "passed");
    assert_eq!(report["baseline"]["schema"], "kiana.eval-baseline.v1");
    assert_eq!(report["baseline"]["provided"], true);
    assert_eq!(report["baseline"]["path"], "baseline.json");
    assert_eq!(report["baseline"]["suite_id"], "fixture-suite");
    assert_eq!(report["baseline"]["status"], "passed");
    assert_eq!(report["baseline"]["findings"].as_array().unwrap().len(), 0);
    assert_eq!(report["baseline"]["sha256"].as_str().unwrap().len(), 64);

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eval_run_fails_when_baseline_regresses() {
    let root = root("baseline-fail");
    let suite = write_suite(
        &root,
        success_fixture(),
        json!({
            "final_status": "completed",
            "stop_reason": "end_turn",
            "min_event_count": 5,
            "tool_call_count": 1,
            "tool_error_count": 0,
        }),
    );
    let baseline = write_baseline(
        &root,
        json!({
            "case-1": {
                "max_event_count": 4,
                "max_input_tokens": 10,
                "required_status": "failed"
            }
        }),
    );

    let output = EvalCommand
        .execute(context(
            format!(
                "run --suite {} --baseline {} --json",
                suite.display(),
                baseline.display()
            ),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["summary"]["failed"], 0);
    assert_eq!(report["status"], "failed");
    assert_eq!(report["baseline"]["status"], "failed");
    let findings = report["baseline"]["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 3);
    assert!(findings.iter().any(|finding| {
        finding["code"] == "baseline_threshold_exceeded"
            && finding["message"].as_str().unwrap().contains("event_count")
    }));
    assert!(findings
        .iter()
        .any(|finding| finding["code"] == "baseline_value_mismatch"));

    let error = EvalCommand
        .execute(context(
            format!(
                "run --suite {} --baseline {} --json --fail-on-failure",
                suite.display(),
                baseline.display()
            ),
            &root,
        ))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("3 baseline finding(s)"));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eval_rejects_invalid_baseline_contracts() {
    let root = root("baseline-invalid");
    let suite = write_suite(&root, success_fixture(), json!({"min_event_count": 1}));
    let cases = [
        (
            "schema",
            json!({"schema":"wrong","suite_id":"fixture-suite","cases":{"case-1":{"max_event_count":5}}}),
            "unsupported eval baseline schema",
        ),
        (
            "suite-id",
            json!({"schema":"kiana.eval-baseline.v1","suite_id":"other","cases":{"case-1":{"max_event_count":5}}}),
            "does not match suite",
        ),
        (
            "empty-cases",
            json!({"schema":"kiana.eval-baseline.v1","suite_id":"fixture-suite","cases":{}}),
            "must contain at least one case",
        ),
        (
            "empty-threshold",
            json!({"schema":"kiana.eval-baseline.v1","suite_id":"fixture-suite","cases":{"case-1":{}}}),
            "must define at least one threshold",
        ),
    ];

    for (name, baseline_value, expected) in cases {
        let baseline = root.join(format!("{name}.json"));
        fs::write(
            &baseline,
            serde_json::to_vec_pretty(&baseline_value).unwrap(),
        )
        .unwrap();
        let error = EvalCommand
            .execute(context(
                format!(
                    "run --suite {} --baseline {} --json",
                    suite.display(),
                    baseline.display()
                ),
                &root,
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{name}: {error:#}");
    }

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eval_run_reports_findings_and_fail_on_failure() {
    let root = root("fail");
    let suite = write_suite(
        &root,
        success_fixture(),
        json!({"tool_error_count": 1, "final_text_contains": ["missing"]}),
    );

    let output = EvalCommand
        .execute(context(
            format!("run --suite {} --json", suite.display()),
            &root,
        ))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();
    assert_eq!(report["status"], "failed");
    assert_eq!(report["cases"][0]["findings"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["cases"][0]["findings"][0]["code"],
        "tool_error_count_mismatch"
    );

    let error = EvalCommand
        .execute(context(
            format!("run --suite {} --json --fail-on-failure", suite.display()),
            &root,
        ))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("eval suite failed"));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eval_rejects_invalid_suite_and_fixture_contracts() {
    let root = root("invalid");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("events.jsonl"), "not-json\n").unwrap();

    let cases = [
        (
            "duplicate",
            json!([
                {"id":"same","kind":"runtime_event_replay","fixture":"events.jsonl","expect":{"min_event_count":1}},
                {"id":"same","kind":"runtime_event_replay","fixture":"events.jsonl","expect":{"min_event_count":1}}
            ]),
            "duplicate eval case id",
        ),
        (
            "unknown-kind",
            json!([{"id":"case","kind":"shell","fixture":"events.jsonl","expect":{"min_event_count":1}}]),
            "unknown eval case kind",
        ),
        (
            "empty-expect",
            json!([{"id":"case","kind":"runtime_event_replay","fixture":"events.jsonl","expect":{}}]),
            "at least one expectation",
        ),
        (
            "escape",
            json!([{"id":"case","kind":"runtime_event_replay","fixture":"../outside.jsonl","expect":{"min_event_count":1}}]),
            "outside suite directory",
        ),
    ];

    for (name, case_values, expected) in cases {
        let suite = root.join(format!("{name}.json"));
        fs::write(
            &suite,
            serde_json::to_vec_pretty(&json!({
                "schema": "kiana.eval-suite.v1",
                "id": name,
                "cases": case_values,
            }))
            .unwrap(),
        )
        .unwrap();
        let error = EvalCommand
            .execute(context(
                format!("run --suite {} --json", suite.display()),
                &root,
            ))
            .await
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{name}: {error:#}");
    }

    let malformed_suite = write_suite(
        &root.join("malformed"),
        "not-json\n",
        json!({"min_event_count": 1}),
    );
    let malformed = EvalCommand
        .execute(context(
            format!("run --suite {} --json", malformed_suite.display()),
            &root,
        ))
        .await
        .unwrap_err();
    assert!(malformed.to_string().contains("invalid JSONL"));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eval_rejects_missing_or_invalid_usage_tokens() {
    let root = root("usage-contract");
    let cases = [
        (
            "missing-input",
            "{\"type\":\"usage\",\"output_tokens\":4}\n",
            "usage event is missing input token field",
        ),
        (
            "missing-output",
            "{\"type\":\"usage\",\"input_tokens\":12}\n",
            "usage event is missing output token field",
        ),
        (
            "invalid-input",
            "{\"type\":\"usage\",\"input_tokens\":\"12\",\"output_tokens\":4}\n",
            "token field 'input_tokens' must be a non-negative integer",
        ),
        (
            "invalid-output",
            "{\"type\":\"usage\",\"input_tokens\":12,\"output_tokens\":-1}\n",
            "token field 'output_tokens' must be a non-negative integer",
        ),
    ];

    for (name, fixture, expected) in cases {
        let suite = write_suite(
            &root.join(name),
            fixture,
            json!({"max_input_tokens": 100, "max_output_tokens": 100}),
        );
        let error = EvalCommand
            .execute(context(
                format!("run --suite {} --json", suite.display()),
                &root,
            ))
            .await
            .unwrap_err();
        let message = error.to_string();
        assert!(message.contains("line 1"), "{name}: {message}");
        assert!(message.contains(expected), "{name}: {message}");
    }

    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn eval_rejects_symlink_fixture_escape() {
    use std::os::unix::fs::symlink;

    let root = root("symlink");
    let outside = root.with_extension("outside.jsonl");
    fs::create_dir_all(&root).unwrap();
    fs::write(&outside, success_fixture()).unwrap();
    symlink(&outside, root.join("events.jsonl")).unwrap();
    let suite = root.join("suite.json");
    fs::write(
        &suite,
        serde_json::to_vec_pretty(&json!({
            "schema": "kiana.eval-suite.v1",
            "id": "symlink",
            "cases": [{
                "id": "case",
                "kind": "runtime_event_replay",
                "fixture": "events.jsonl",
                "expect": {"min_event_count": 1}
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let error = EvalCommand
        .execute(context(
            format!("run --suite {} --json", suite.display()),
            &root,
        ))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("outside suite directory"));

    let _ = fs::remove_dir_all(root);
    let _ = fs::remove_file(outside);
}
