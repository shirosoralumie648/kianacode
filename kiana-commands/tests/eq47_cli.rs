use kiana_commands::eval::{EvalCommand, EVAL_CLI_COMMAND_SCHEMA, EVAL_CLI_COMMAND_VERSION};
use kiana_commands::{Command, CommandContext, CommandRoute};
use serde_json::json;
use std::collections::HashMap;

fn context(args: &[&str]) -> CommandContext {
    CommandContext {
        args: args.join(" "),
        app_state: HashMap::new(),
    }
}

#[test]
fn migrated_eval_commands_use_the_control_plane_route() {
    let cases = [
        (vec!["run", "--suite-id", "suite-a"], "eval.run"),
        (
            vec!["capture", "--source", "run-1", "--name", "golden"],
            "eval.capture",
        ),
        (
            vec!["compare", "--reference", "ref-1", "--candidate", "cand-1"],
            "eval.compare",
        ),
        (
            vec!["explain", "--finding", "safety_violation"],
            "eval.explain",
        ),
        (vec!["list", "--kind", "suite"], "eval.list"),
    ];

    for (args, expected_name) in cases {
        let route = EvalCommand.route(&context(&args)).unwrap();
        let CommandRoute::ControlPlane { name, arguments } = route else {
            panic!("{expected_name} did not use ControlPlane route");
        };
        assert_eq!(name, expected_name);
        assert_eq!(arguments["schema"], EVAL_CLI_COMMAND_SCHEMA);
        assert_eq!(arguments["version"], EVAL_CLI_COMMAND_VERSION);
        assert_eq!(arguments["output"], "text");
    }
}

#[test]
fn legacy_run_suite_path_stays_a_compatibility_local_route() {
    let route = EvalCommand
        .route(&context(&[
            "run",
            "--suite",
            "fixtures/suite.json",
            "--json",
        ]))
        .unwrap();
    assert_eq!(route, CommandRoute::Local);
}

#[test]
fn migrated_eval_arguments_are_bounded_and_canonical() {
    let route = EvalCommand
        .route(&context(&[
            "compare",
            "--reference=ref-1",
            "--candidate=candidate-1",
            "--json",
        ]))
        .unwrap();
    let CommandRoute::ControlPlane { arguments, .. } = route else {
        panic!("compare must use ControlPlane route");
    };
    assert_eq!(
        arguments,
        json!({
            "schema": EVAL_CLI_COMMAND_SCHEMA,
            "version": EVAL_CLI_COMMAND_VERSION,
            "action": "compare",
            "output": "json",
            "reference_ref": "ref-1",
            "candidate_ref": "candidate-1",
        })
    );
    assert!(arguments["candidate_ref"].is_string());
}

#[test]
fn unknown_or_incomplete_eval_arguments_fail_closed() {
    for args in [
        vec!["unknown"],
        vec!["run", "--unknown", "value"],
        vec!["capture"],
        vec!["capture", "--source", "run-1", "--source", "run-2"],
        vec!["compare", "--reference", "ref-1"],
        vec!["list", "--kind", "not-a-kind"],
        vec!["explain", "--target", "../outside"],
    ] {
        let error = EvalCommand.route(&context(&args)).unwrap_err().to_string();
        assert!(!error.is_empty(), "args {args:?} unexpectedly accepted");
    }
}

#[tokio::test]
async fn migrated_eval_execution_does_not_create_a_local_second_loop() {
    let error = EvalCommand
        .execute(context(&["list", "--json"]))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(error, "command_requires_control_plane");
}

#[test]
fn help_remains_local_and_reports_all_subcommands() {
    let route = EvalCommand.route(&context(&["--help"])).unwrap();
    assert_eq!(route, CommandRoute::Local);
}
