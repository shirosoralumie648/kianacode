use kiana_client::{
    present_cli_output, CliCommand, CliCommandId, CliExitCode, CliLocale, CliOutput,
    CliOutputMode, CliPresenterOptions, CliSignal,
};
use kiana_protocol::{ExecutionStatus, RequestId};
use serde_json::{json, Value};

fn output(mode: CliOutputMode, status: ExecutionStatus, error: Option<&str>) -> CliOutput {
    CliOutput::new(
        CliCommandId::new(CliCommand::Status, RequestId::new()),
        mode,
        status,
        json!({"receipt": {"id": "receipt-1", "digest": "sha256:demo"}}),
        error.map(str::to_owned),
    )
}

#[test]
fn lifecycle_and_signal_exit_codes_are_stable_and_unknown_is_nonzero() {
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::Completed, None),
        CliExitCode::Success
    );
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::Blocked, Some("permission_denied")),
        CliExitCode::PermissionDenied
    );
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::AwaitingApproval, None),
        CliExitCode::ApprovalRequired
    );
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::Cancelled, None),
        CliExitCode::Cancelled
    );
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::ResultUnknown, None),
        CliExitCode::Unknown
    );
    assert_ne!(CliSignal::Sigint.exit_code(), CliExitCode::Success);
    assert_eq!(CliSignal::BrokenPipe.exit_code(), CliExitCode::BrokenPipe);
    assert_eq!(CliSignal::Sigint.exit_code().as_i32(), 130);
    assert_eq!(CliSignal::BrokenPipe.exit_code().as_i32(), 141);
}

#[test]
fn json_success_is_one_schema_dto_on_stdout_and_has_no_decorations() {
    let presentation = present_cli_output(
        &output(CliOutputMode::Json, ExecutionStatus::Completed, None),
        CliPresenterOptions::default(),
    )
    .unwrap();

    assert_eq!(presentation.exit_code, CliExitCode::Success);
    assert!(presentation.stderr.is_empty());
    assert!(!presentation.stdout.contains('\u{1b}'));
    let dto: Value = serde_json::from_str(&presentation.stdout).unwrap();
    assert_eq!(dto["schema"], "kiana.cli-output.v1");
    assert_eq!(dto["status"], "completed");
    assert_eq!(dto["payload"]["receipt"]["id"], "receipt-1");

    let warning_output = output(CliOutputMode::Json, ExecutionStatus::Completed, None)
        .with_warnings(["stale cache".to_owned()]);
    let warning_presentation = present_cli_output(&warning_output, CliPresenterOptions::default())
        .unwrap();
    assert!(warning_presentation.stdout.starts_with('{'));
    assert!(warning_presentation.stderr.contains("warning: stale cache"));
}

#[test]
fn json_errors_stay_structured_on_stderr_and_unknown_never_returns_zero() {
    let presentation = present_cli_output(
        &output(
            CliOutputMode::Json,
            ExecutionStatus::ResultUnknown,
            Some("result_unknown:response_lost"),
        ),
        CliPresenterOptions::default(),
    )
    .unwrap();

    assert!(presentation.stdout.is_empty());
    assert_eq!(presentation.exit_code, CliExitCode::Unknown);
    let dto: Value = serde_json::from_str(&presentation.stderr).unwrap();
    assert_eq!(dto["schema"], "kiana.cli-output.v1");
    assert_eq!(dto["status"], "result_unknown");
    assert_eq!(dto["error"], "result_unknown:response_lost");
}

#[test]
fn tty_requires_a_tty_paginates_only_in_tty_and_keeps_warnings_off_stdout() {
    let tty = output(CliOutputMode::Tty, ExecutionStatus::Completed, None)
        .with_warnings(["stale cache".to_owned()]);
    let no_tty = present_cli_output(&tty, CliPresenterOptions::default());
    assert_eq!(no_tty.unwrap_err(), "cli_tty_output_requires_tty");

    let mut pipe_options = CliPresenterOptions::default();
    pipe_options.paginate = true;
    assert_eq!(
        present_cli_output(
            &output(CliOutputMode::Json, ExecutionStatus::Completed, None),
            pipe_options,
        )
        .unwrap_err(),
        "cli_pager_requires_tty"
    );

    let presentation = present_cli_output(
        &tty,
        CliPresenterOptions {
            tty: true,
            locale: CliLocale::Chinese,
            paginate: true,
            max_tty_bytes: 512,
        },
    )
    .unwrap();
    assert!(presentation.stdout.contains("状态: completed"));
    assert!(presentation.stdout.contains("receipt-1"));
    assert!(presentation.stderr.contains("警告: stale cache"));
    assert!(!presentation.stdout.contains("stale cache"));
}

#[test]
fn quiet_mode_suppresses_stdout_and_bounds_tty_artifacts() {
    let quiet = output(CliOutputMode::Quiet, ExecutionStatus::Denied, Some("permission_denied"));
    let presentation = present_cli_output(&quiet, CliPresenterOptions::default()).unwrap();
    assert!(presentation.stdout.is_empty());
    assert!(presentation.stderr.contains("permission_denied"));
    assert_eq!(presentation.exit_code, CliExitCode::PermissionDenied);

    let large_payload = output(CliOutputMode::Tty, ExecutionStatus::Completed, None);
    let presentation = present_cli_output(
        &large_payload,
        CliPresenterOptions {
            tty: true,
            locale: CliLocale::English,
            paginate: false,
            max_tty_bytes: 16,
        },
    )
    .unwrap();
    assert!(presentation.truncated);
    assert!(presentation.stdout.len() <= 16);
}

#[test]
fn json_rejects_ansi_in_diagnostics_and_secret_or_oversized_warnings() {
    let ansi = output(CliOutputMode::Json, ExecutionStatus::Completed, Some("\u{1b}[31mred"));
    assert_eq!(ansi.validate().unwrap_err(), "cli_output_error_invalid");

    let secret = output(CliOutputMode::Json, ExecutionStatus::Completed, None)
        .with_warnings(["authorization: Bearer secret".to_owned()]);
    assert_eq!(secret.validate().unwrap_err(), "cli_output_warning_invalid");

    let oversized = output(CliOutputMode::Json, ExecutionStatus::Completed, None)
        .with_warnings(["x".repeat(513)]);
    assert_eq!(oversized.validate().unwrap_err(), "cli_output_warning_invalid");
}
