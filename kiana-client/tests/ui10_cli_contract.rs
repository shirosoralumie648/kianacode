use kiana_client::{
    classify_legacy_flag, cli_help_text, legacy_flag_spec, normalize_legacy_args,
    normalize_workspace, validate_command_arguments, CliArgumentError, CliCommand, CliCommandId,
    CliFacade, CliInvocation, CliLegacyDisposition, CliOutput, CliOutputMode, CliSessionRef,
    CLI_COMMAND_CONTRACT_SCHEMA, CLI_COMMAND_SPECS, CLI_LEGACY_FLAGS, CLI_MAX_ARGV_TOKENS,
    CLI_MAX_MIGRATION_NOTICES, CLI_SESSION_LATEST, UI_ACTION_CANCEL_OPERATION,
    UI_ACTION_CONTINUE_OPERATION, UI_ACTION_SUBMIT_OPERATION, UI_ARTIFACT_PAGE_OPERATION,
    UI_FEED_RESUME_OPERATION, UI_FEED_SUBSCRIBE_OPERATION, UI_QUERY_COMMAND_STATUS_OPERATION,
    UI_QUERY_HISTORY_OPERATION, UI_QUERY_SNAPSHOT_OPERATION,
};
use kiana_protocol::{ExecutionStatus, RequestId};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const CONTRACT_FIXTURE: &str = include_str!("fixtures/ui10-cli-contract.json");

fn contract_fixture() -> Value {
    serde_json::from_str(CONTRACT_FIXTURE).expect("UI-10 contract fixture must stay valid JSON")
}

fn invocation_for(command: CliCommand, session: Option<&str>, arguments: Value) -> CliInvocation {
    CliInvocation::new(
        command,
        RequestId::new(),
        "/workspace",
        session.map(str::to_owned),
        CliOutputMode::Json,
        false,
        false,
        false,
        arguments,
    )
}

#[test]
fn canonical_commands_and_legacy_aliases_have_stable_wire_names() {
    assert_eq!(CliCommand::ALL.len(), 10);
    assert_eq!(CliCommand::parse("approval").unwrap(), CliCommand::Approve);
    assert_eq!(CliCommand::parse("events").unwrap(), CliCommand::Events);
    assert_eq!(CliCommand::parse("sessions").unwrap(), CliCommand::Session);
    assert_eq!(CliCommand::Status.wire_name(), "run.status");
    assert_eq!(CliCommand::Export.wire_name(), "audit.export");
    assert_eq!(
        CliCommand::parse("unknown").unwrap_err(),
        "cli_command_unknown"
    );
}

fn make_invocation(command: CliCommand) -> CliInvocation {
    CliInvocation::new(
        command,
        RequestId::new(),
        "/workspace",
        Some("session-1".to_owned()),
        CliOutputMode::Json,
        false,
        false,
        false,
        json!({"prompt": "show status"}),
    )
}

#[test]
fn invocation_rejects_missing_scope_interaction_and_implicit_mutation_retry() {
    let mut missing_workspace = make_invocation(CliCommand::Run);
    missing_workspace.workspace.clear();
    assert_eq!(
        missing_workspace.validate().unwrap_err(),
        "cli_workspace_required"
    );

    let mut missing_session = make_invocation(CliCommand::Status);
    missing_session.session_id = None;
    assert_eq!(
        missing_session.validate().unwrap_err(),
        "cli_session_required"
    );

    let mut no_tty = make_invocation(CliCommand::Run);
    no_tty.interactive = true;
    assert_eq!(
        no_tty.validate().unwrap_err(),
        "cli_interactive_requires_tty"
    );

    let mut tty_output = make_invocation(CliCommand::Run);
    tty_output.output_mode = CliOutputMode::Tty;
    assert_eq!(
        tty_output.validate().unwrap_err(),
        "cli_tty_output_requires_tty"
    );

    let mut retry = make_invocation(CliCommand::Cancel);
    retry.implicit_retry = true;
    assert_eq!(
        retry.validate().unwrap_err(),
        "cli_implicit_retry_forbidden"
    );
}

#[test]
fn invocation_and_output_reject_secret_fields_and_json_ansi() {
    let mut invocation = make_invocation(CliCommand::Run);
    invocation.arguments = json!({"api_key": "do-not-send"});
    assert_eq!(
        invocation.validate().unwrap_err(),
        "cli_arguments_sensitive_or_oversized"
    );

    let mut textual_secret = make_invocation(CliCommand::Run);
    textual_secret.arguments = json!({"message": "Bearer fixture-secret"});
    assert_eq!(
        textual_secret.validate().unwrap_err(),
        "cli_arguments_sensitive_or_oversized"
    );

    let command_id = CliCommandId::new(CliCommand::Status, RequestId::new());
    let output = CliOutput::new(
        command_id.clone(),
        CliOutputMode::Json,
        ExecutionStatus::Completed,
        json!({"authorization": "Bearer secret"}),
        None,
    );
    assert_eq!(
        output.validate().unwrap_err(),
        "cli_output_sensitive_or_oversized"
    );

    let textual_output = CliOutput::new(
        command_id.clone(),
        CliOutputMode::Json,
        ExecutionStatus::Completed,
        json!({"message": "token=fixture-secret"}),
        None,
    );
    assert_eq!(
        textual_output.validate().unwrap_err(),
        "cli_output_sensitive_or_oversized"
    );

    let ansi = CliOutput::new(
        command_id,
        CliOutputMode::Json,
        ExecutionStatus::Completed,
        json!({"text": "\u{1b}[31mred\u{1b}[0m"}),
        None,
    );
    assert_eq!(ansi.validate().unwrap_err(), "cli_json_ansi_forbidden");
}

/// Completion slice (2026-10-17): the declared command rows are the code tables.  Every field of
/// every row is compared both ways so the fixture cannot drift from `CLI_COMMAND_SPECS`.
#[test]
fn command_specs_match_the_declared_contract_fixture() {
    let contract = contract_fixture();
    assert_eq!(contract["schema"], CLI_COMMAND_CONTRACT_SCHEMA);
    assert_eq!(contract["step"], "UI-10");

    let rows = contract["commands"].as_array().expect("commands");
    assert_eq!(rows.len(), CLI_COMMAND_SPECS.len());
    assert_eq!(rows.len(), CliCommand::ALL.len());

    for (row, spec) in rows.iter().zip(CLI_COMMAND_SPECS.iter()) {
        assert_eq!(row["command"].as_str(), Some(spec.command.name()));
        assert_eq!(row["wire_name"].as_str(), Some(spec.wire_name));
        assert_eq!(row["wire_name"].as_str(), Some(spec.command.wire_name()));
        assert_eq!(row["summary"].as_str(), Some(spec.summary));
        assert_eq!(row["usage"].as_str(), Some(spec.usage));
        assert_eq!(row["operation"].as_str(), Some(spec.operation));
        assert_eq!(
            row["follow_up_operation"].as_str(),
            spec.follow_up_operation
        );
        assert_eq!(row["facade"].as_str(), Some(spec.facade.wire_name()));
        assert_eq!(
            row["requires_session"].as_bool(),
            Some(spec.requires_session)
        );
        assert_eq!(row["mutates_state"].as_bool(), Some(spec.mutates_state));
        assert_eq!(
            row["idempotency_required"].as_bool(),
            Some(spec.idempotency_required)
        );

        // The table agrees with the enum, and the enum routes back to the same row.
        assert_eq!(spec.requires_session, spec.command.requires_session());
        assert_eq!(spec.mutates_state, spec.command.mutates_state());
        assert_eq!(spec.command.spec().wire_name, spec.wire_name);

        let modes: Vec<&str> = row["output_modes"]
            .as_array()
            .expect("output_modes")
            .iter()
            .map(|value| value.as_str().expect("mode"))
            .collect();
        assert_eq!(modes, spec.mode_names());
        assert!(!spec.output_modes.is_empty());

        let arguments = row["arguments"].as_array().expect("arguments");
        assert_eq!(arguments.len(), spec.arguments.len());
        for (argument_row, argument) in arguments.iter().zip(spec.arguments.iter()) {
            assert_eq!(argument_row["name"].as_str(), Some(argument.name));
            assert_eq!(argument_row["required"].as_bool(), Some(argument.required));
            assert_eq!(
                argument_row["kind"].as_str(),
                Some(argument.kind.wire_name())
            );
            assert_eq!(
                argument_row["max_bytes"].as_u64(),
                Some(argument.max_bytes as u64)
            );
            match argument.kind.allowed_values() {
                Some(values) => {
                    let allowed: Vec<&str> = argument_row["allowed"]
                        .as_array()
                        .expect("allowed")
                        .iter()
                        .map(|value| value.as_str().expect("allowed value"))
                        .collect();
                    assert_eq!(allowed, values);
                }
                None => assert!(
                    argument_row.get("allowed").is_none(),
                    "{} must not declare enum values",
                    argument.name
                ),
            }
        }

        // Every command only routes through the typed client vocabulary.
        let mut operations = vec![spec.operation];
        operations.extend(spec.follow_up_operation);
        for operation in operations {
            assert!(
                [
                    kiana_client::UI_INITIALIZE_OPERATION,
                    UI_QUERY_SNAPSHOT_OPERATION,
                    UI_QUERY_HISTORY_OPERATION,
                    UI_QUERY_COMMAND_STATUS_OPERATION,
                    UI_FEED_SUBSCRIBE_OPERATION,
                    UI_FEED_RESUME_OPERATION,
                    UI_ARTIFACT_PAGE_OPERATION,
                    UI_ACTION_SUBMIT_OPERATION,
                    UI_ACTION_CANCEL_OPERATION,
                    UI_ACTION_CONTINUE_OPERATION,
                    kiana_client::UI_ACTION_STATUS_OPERATION,
                ]
                .contains(&operation),
                "command {} uses an undeclared operation: {operation}",
                spec.command.name()
            );
        }
        assert!(spec.summary.len() <= 128);
        assert!(spec.usage.starts_with("kiana "));
        assert!(spec.usage.len() <= 512);
    }

    // The operation/facade mapping is pinned command by command.
    for (command, operation, facade) in [
        (
            CliCommand::Run,
            UI_ACTION_SUBMIT_OPERATION,
            CliFacade::Action,
        ),
        (
            CliCommand::Status,
            UI_QUERY_SNAPSHOT_OPERATION,
            CliFacade::Query,
        ),
        (
            CliCommand::Events,
            UI_FEED_SUBSCRIBE_OPERATION,
            CliFacade::Feed,
        ),
        (
            CliCommand::Approve,
            UI_ACTION_SUBMIT_OPERATION,
            CliFacade::Action,
        ),
        (
            CliCommand::Deny,
            UI_ACTION_SUBMIT_OPERATION,
            CliFacade::Action,
        ),
        (
            CliCommand::Cancel,
            UI_ACTION_CANCEL_OPERATION,
            CliFacade::Action,
        ),
        (
            CliCommand::Resume,
            UI_ACTION_CONTINUE_OPERATION,
            CliFacade::Action,
        ),
        (
            CliCommand::Receipt,
            UI_QUERY_SNAPSHOT_OPERATION,
            CliFacade::Query,
        ),
        (
            CliCommand::Export,
            UI_ARTIFACT_PAGE_OPERATION,
            CliFacade::Artifact,
        ),
        (
            CliCommand::Session,
            UI_QUERY_SNAPSHOT_OPERATION,
            CliFacade::Query,
        ),
    ] {
        assert_eq!(command.spec().operation, operation);
        assert_eq!(command.spec().facade, facade);
    }

    // An accepted invocation only routes through its declared row.
    let routed = invocation_for(CliCommand::Status, Some("s1"), json!({}))
        .route()
        .expect("validated invocation routes");
    assert_eq!(routed.wire_name, "run.status");
    let unroutable = invocation_for(CliCommand::Status, Some("s1"), json!({"nope": true}));
    assert_eq!(
        unroutable.route().unwrap_err(),
        CliArgumentError::UnknownArgument
    );
}

/// Completion slice (2026-10-17): the help text is rendered from the same tables and covers every
/// command, argument and output mode; it never carries ANSI or a second contract.
#[test]
fn help_text_covers_every_command_and_output_mode() {
    let contract = contract_fixture();
    let shape = &contract["help_shape"];
    let first_line_contains = shape["first_line_contains"].as_str().expect("shape");
    let mode_prefix = shape["mode_line_prefix"].as_str().expect("shape");
    let usage_prefix = shape["usage_line_prefix"].as_str().expect("shape");
    let argument_prefix = shape["argument_line_prefix"].as_str().expect("shape");
    let per_command_lines = shape["per_command_lines"].as_u64().expect("shape") as usize;

    let text = cli_help_text();
    assert_eq!(text, cli_help_text(), "help text must be deterministic");
    assert!(!text.contains('\u{1b}'));
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        CLI_COMMAND_SPECS.len() + 3 + CLI_COMMAND_SPECS.len() * per_command_lines,
        "help text shape drifted"
    );
    assert!(lines[0].contains(first_line_contains));
    assert_eq!(lines[1], "Commands:");
    for spec in CLI_COMMAND_SPECS.iter() {
        assert!(text.contains(&format!("{}:", spec.wire_name)));
        assert!(text.contains(&format!("{usage_prefix}{}", spec.usage)));
        assert!(text.contains(&format!("{mode_prefix}{}", spec.mode_names().join(", "))));
        assert!(text.contains(spec.summary));
    }
    for spec in CLI_COMMAND_SPECS
        .iter()
        .filter(|spec| !spec.arguments.is_empty())
    {
        for argument in spec.arguments.iter() {
            assert!(
                text.contains(argument.name),
                "help text dropped argument {}",
                argument.name
            );
            assert!(text.contains(argument.kind.wire_name()));
        }
    }
    assert!(text.contains(argument_prefix));
    assert!(text.contains("scope: workspace required"));
}

/// Completion slice (2026-10-17): the argument error catalogue is closed, unique and every code
/// carries a remediation.
#[test]
fn argument_errors_follow_the_stable_catalogue() {
    let contract = contract_fixture();
    let declared: Vec<&str> = contract["error_codes"]
        .as_array()
        .expect("error_codes")
        .iter()
        .map(|value| value.as_str().expect("code"))
        .collect();
    let implemented: Vec<&str> = CliArgumentError::ALL
        .iter()
        .map(|error| error.code())
        .collect();
    assert_eq!(declared, implemented);
    assert_eq!(implemented.len(), 27);
    let unique: BTreeSet<&str> = implemented.iter().copied().collect();
    assert_eq!(
        unique.len(),
        implemented.len(),
        "error codes must be unique"
    );
    for error in CliArgumentError::ALL {
        assert!(error.code().starts_with("cli_"));
        assert!(!error.remediation().trim().is_empty());
        assert!(error.remediation().len() <= 128);
        assert!(!error.remediation().contains('\u{1b}'));
    }
    assert_eq!(
        CliCommand::parse_typed("nope").unwrap_err().code(),
        "cli_command_unknown"
    );
    assert_eq!(
        CliOutputMode::parse_typed("yaml").unwrap_err().code(),
        "cli_output_mode_invalid"
    );
    assert_eq!(
        CliOutputMode::parse_typed("text").unwrap(),
        CliOutputMode::Tty
    );
}

/// Completion slice (2026-10-17): per-command arguments fail closed before dispatch.
#[test]
fn per_command_arguments_fail_closed_before_dispatch() {
    assert_eq!(
        validate_command_arguments(CliCommand::Run, &json!({})),
        Err(CliArgumentError::MissingArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Run, &json!({"prompt": "hi", "extra": true})),
        Err(CliArgumentError::UnknownArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Run, &json!({"prompt": "   "})),
        Err(CliArgumentError::InvalidArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Run, &json!({"prompt": "hi", "stream": "yes"})),
        Err(CliArgumentError::InvalidArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Run, &json!({"prompt": "hi", "stream": false})),
        Ok(())
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Deny, &json!({"approval_id": "a1"})),
        Err(CliArgumentError::MissingArgument)
    );
    assert_eq!(
        validate_command_arguments(
            CliCommand::Deny,
            &json!({"approval_id": "a1", "reason": "no"})
        ),
        Ok(())
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Events, &json!({"after_sequence": -1})),
        Err(CliArgumentError::InvalidArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Receipt, &json!({"detail": "everything"})),
        Err(CliArgumentError::InvalidArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Export, &json!({"format": "xml"})),
        Err(CliArgumentError::InvalidArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Export, &json!({"format": "jsonl"})),
        Ok(())
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Status, &json!(["detail"])),
        Err(CliArgumentError::InvalidArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Status, &json!({"detail": "s".repeat(200)})),
        Err(CliArgumentError::OversizedArgument)
    );
    assert_eq!(
        validate_command_arguments(CliCommand::Session, &json!({"action": "attach"})),
        Ok(())
    );

    // The same fences are reached through the invocation boundary.
    assert_eq!(
        invocation_for(CliCommand::Run, None, json!({})).validate(),
        Err("cli_argument_missing".to_owned())
    );
    assert_eq!(
        invocation_for(CliCommand::Run, None, json!({"prompt": "hi", "extra": 1})).validate(),
        Err("cli_argument_unknown".to_owned())
    );
    assert_eq!(
        invocation_for(CliCommand::Run, None, json!({"prompt": "hi"})).validate(),
        Ok(())
    );
    assert_eq!(
        invocation_for(CliCommand::Status, Some("s1"), json!({})).validate_typed(),
        Ok(())
    );
}

/// Completion slice (2026-10-17): output modes are per command; `quiet` never hides a payload that
/// is the result of the command.
#[test]
fn output_mode_support_is_enforced_per_command() {
    let quiet_commands: Vec<&str> = CLI_COMMAND_SPECS
        .iter()
        .filter(|spec| spec.supports(CliOutputMode::Quiet))
        .map(|spec| spec.command.name())
        .collect();
    assert_eq!(quiet_commands, vec!["run", "status", "cancel", "resume"]);

    let mut quiet_events = invocation_for(CliCommand::Events, Some("s1"), json!({}));
    quiet_events.output_mode = CliOutputMode::Quiet;
    assert_eq!(
        quiet_events.validate_typed(),
        Err(CliArgumentError::OutputModeUnsupported)
    );

    let mut quiet_receipt = invocation_for(CliCommand::Receipt, Some("s1"), json!({}));
    quiet_receipt.output_mode = CliOutputMode::Quiet;
    assert_eq!(
        quiet_receipt.validate_typed(),
        Err(CliArgumentError::OutputModeUnsupported)
    );

    let mut quiet_session = invocation_for(CliCommand::Session, None, json!({}));
    quiet_session.output_mode = CliOutputMode::Quiet;
    assert_eq!(
        quiet_session.validate_typed(),
        Err(CliArgumentError::OutputModeUnsupported)
    );

    let mut quiet_export =
        invocation_for(CliCommand::Export, Some("s1"), json!({"format": "json"}));
    quiet_export.output_mode = CliOutputMode::Quiet;
    assert_eq!(
        quiet_export.validate_typed(),
        Err(CliArgumentError::OutputModeUnsupported)
    );

    let mut quiet_cancel = invocation_for(CliCommand::Cancel, Some("s1"), json!({}));
    quiet_cancel.output_mode = CliOutputMode::Quiet;
    assert_eq!(quiet_cancel.validate_typed(), Ok(()));

    let mut quiet_run = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    quiet_run.output_mode = CliOutputMode::Quiet;
    assert_eq!(quiet_run.validate_typed(), Ok(()));

    let mut quiet_status = invocation_for(CliCommand::Status, Some("s1"), json!({}));
    quiet_status.output_mode = CliOutputMode::Quiet;
    assert_eq!(quiet_status.validate_typed(), Ok(()));

    // TTY output still requires a real TTY first, and JSON reads without one.
    let mut tty_events = invocation_for(CliCommand::Events, Some("s1"), json!({}));
    tty_events.output_mode = CliOutputMode::Tty;
    assert_eq!(
        tty_events.validate_typed(),
        Err(CliArgumentError::TtyOutputRequiresTty)
    );
    tty_events.tty = true;
    assert_eq!(tty_events.validate_typed(), Ok(()));
}

/// Completion slice (2026-10-17): session resolution is server-owned; the local fence refuses ids
/// the server snapshot does not contain and keeps `latest` for the server to resolve.
#[test]
fn session_resolution_refuses_unknown_ids_from_the_server_snapshot() {
    assert_eq!(
        CliSessionRef::parse(CLI_SESSION_LATEST),
        Ok(CliSessionRef::Latest)
    );
    assert_eq!(
        CliSessionRef::parse(" s1 "),
        Ok(CliSessionRef::Explicit("s1".to_owned()))
    );
    assert_eq!(
        CliSessionRef::parse("   "),
        Err(CliArgumentError::SessionInvalid)
    );
    assert_eq!(
        CliSessionRef::parse("bad\nid"),
        Err(CliArgumentError::SessionInvalid)
    );
    assert_eq!(CliSessionRef::Latest.resolve(&[]), Ok(None));
    assert_eq!(
        CliSessionRef::Explicit("s1".to_owned()).resolve(&["s1".to_owned()]),
        Ok(Some("s1".to_owned()))
    );
    assert_eq!(
        CliSessionRef::Explicit("s9".to_owned()).resolve(&["s1".to_owned()]),
        Err(CliArgumentError::SessionUnknown)
    );
    assert_eq!(CliSessionRef::Latest.session_id(), None);
    assert_eq!(
        CliSessionRef::Explicit("s1".to_owned()).session_id(),
        Some("s1")
    );

    let known = vec!["s1".to_owned(), "s2".to_owned()];
    let status = invocation_for(CliCommand::Status, Some("s1"), json!({}));
    assert_eq!(status.resolve_session(&known), Ok(Some("s1".to_owned())));
    let latest = invocation_for(CliCommand::Status, Some(CLI_SESSION_LATEST), json!({}));
    assert_eq!(latest.session_ref(), Ok(Some(CliSessionRef::Latest)));
    assert_eq!(latest.resolve_session(&known), Ok(None));
    let unknown = invocation_for(CliCommand::Status, Some("s9"), json!({}));
    // The shape is fine locally; existence is a server fact and only `resolve_session` refuses it.
    assert_eq!(unknown.validate(), Ok(()));
    assert_eq!(
        unknown.resolve_session(&known),
        Err(CliArgumentError::SessionUnknown)
    );
    let no_session = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    assert_eq!(no_session.session_ref(), Ok(None));
    assert_eq!(no_session.resolve_session(&known), Ok(None));

    // Workspace normalization trims but never accepts an empty or control-character value.
    assert_eq!(normalize_workspace("  /w  "), Ok("/w".to_owned()));
    assert_eq!(
        normalize_workspace("\n  "),
        Err(CliArgumentError::WorkspaceRequired)
    );
    assert_eq!(
        normalize_workspace("/w\n/w"),
        Err(CliArgumentError::WorkspaceRequired)
    );
}

/// Completion slice (2026-10-17): legacy flags rewrite, drop or fail closed exactly as declared.
#[test]
fn legacy_args_rewrite_or_fail_closed_per_the_migration_table() {
    let contract = contract_fixture();
    let rows = contract["legacy_argv"].as_array().expect("legacy_argv");
    assert!(
        rows.len() >= 12,
        "the migration fixture keeps its deny rows"
    );
    for row in rows {
        let name = row["name"].as_str().expect("row name");
        let argv: Vec<String> = row["argv"]
            .as_array()
            .expect("argv")
            .iter()
            .map(|value| value.as_str().expect("token").to_owned())
            .collect();
        match row["disposition"].as_str().expect("disposition") {
            "retired" => {
                assert_eq!(
                    normalize_legacy_args(&argv).unwrap_err(),
                    CliArgumentError::LegacyFlagRetired,
                    "row {name} must fail closed"
                );
                assert_eq!(row["error"].as_str(), Some("cli_legacy_flag_retired"));
            }
            "rewrite" | "drop" | "pass_through" => {
                let normalized = normalize_legacy_args(&argv).expect("row normalizes");
                let expected: Vec<&str> = row["expected"]
                    .as_array()
                    .expect("expected")
                    .iter()
                    .map(|value| value.as_str().expect("token"))
                    .collect();
                assert_eq!(normalized.args, expected, "row {name}");
                assert_eq!(
                    normalized.notices.len(),
                    row["notices"].as_u64().expect("notices") as usize,
                    "row {name}"
                );
            }
            other => panic!("unknown disposition {other}"),
        }
    }

    let declared: Vec<&str> = contract["legacy_flags"]
        .as_array()
        .expect("legacy_flags")
        .iter()
        .map(|row| row["legacy"].as_str().expect("legacy flag"))
        .collect();
    let implemented: Vec<&str> = CLI_LEGACY_FLAGS.iter().map(|spec| spec.legacy).collect();
    assert_eq!(declared, implemented);
    for spec in CLI_LEGACY_FLAGS.iter() {
        assert_eq!(
            legacy_flag_spec(spec.legacy).map(|found| found.legacy),
            Some(spec.legacy)
        );
        match spec.disposition {
            CliLegacyDisposition::Retired => assert_eq!(
                classify_legacy_flag(spec.legacy),
                Some(Err(CliArgumentError::LegacyFlagRetired))
            ),
            _ => assert!(matches!(classify_legacy_flag(spec.legacy), Some(Ok(_)))),
        }
    }
    assert_eq!(classify_legacy_flag("--json"), None);
    assert_eq!(classify_legacy_flag("--workspace"), None);

    let normalized =
        normalize_legacy_args(&["run".to_owned(), "--session-id".to_owned(), "s1".to_owned()])
            .expect("alias normalizes");
    assert_eq!(normalized.notices[0].legacy, "--session-id");
    assert_eq!(
        normalized.notices[0].canonical.as_deref(),
        Some("--session")
    );
    assert!(!normalized.notices[0].guidance.trim().is_empty());
    assert!(!normalized.notices[0].guidance.contains('\u{1b}'));
}

/// Completion slice (2026-10-17): argv normalization and the notice list stay bounded, and `--`
/// keeps its end-of-options meaning.
#[test]
fn legacy_normalization_bounds_and_passthrough_are_enforced() {
    let oversized: Vec<String> = (0..=CLI_MAX_ARGV_TOKENS)
        .map(|index| format!("--json{index}"))
        .collect();
    assert_eq!(
        normalize_legacy_args(&oversized).unwrap_err(),
        CliArgumentError::TooManyArguments
    );

    let mut too_many_notices: Vec<String> = Vec::new();
    for index in 0..=CLI_MAX_MIGRATION_NOTICES {
        too_many_notices.push("--session-id".to_owned());
        too_many_notices.push(format!("s{index}"));
    }
    assert_eq!(
        normalize_legacy_args(&too_many_notices).unwrap_err(),
        CliArgumentError::TooManyArguments
    );

    let mut at_bound: Vec<String> = Vec::new();
    for index in 0..CLI_MAX_MIGRATION_NOTICES {
        at_bound.push("--session-id".to_owned());
        at_bound.push(format!("s{index}"));
    }
    let normalized = normalize_legacy_args(&at_bound).expect("bound is inclusive");
    assert_eq!(normalized.notices.len(), CLI_MAX_MIGRATION_NOTICES);

    let passthrough = normalize_legacy_args(&[
        "run".to_owned(),
        "--".to_owned(),
        "--sdk-url".to_owned(),
        "x".to_owned(),
    ])
    .expect("end of options protects literals");
    assert_eq!(passthrough.args, vec!["run", "--", "--sdk-url", "x"]);
    assert!(passthrough.notices.is_empty());

    let empty = normalize_legacy_args(&[]).expect("empty argv is valid");
    assert!(empty.args.is_empty());
    assert!(empty.notices.is_empty());

    // A dropped boolean token cannot smuggle an attached value.
    assert_eq!(
        normalize_legacy_args(&["--print=1".to_owned(), "hello".to_owned()]).unwrap_err(),
        CliArgumentError::LegacyFlagRetired
    );
}

/// Completion slice (2026-10-17): the typed API and the string API stay the same fence; the
/// catalogue did not rename an existing code.
#[test]
fn typed_validation_maps_to_the_same_codes_as_the_string_api() {
    let mut missing_workspace = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    missing_workspace.workspace.clear();
    let missing_session = invocation_for(CliCommand::Status, None, json!({}));
    let mut interactive = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    interactive.interactive = true;
    let mut tty_output = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    tty_output.output_mode = CliOutputMode::Tty;
    let mut quiet_events = invocation_for(CliCommand::Events, Some("s1"), json!({}));
    quiet_events.output_mode = CliOutputMode::Quiet;
    let mut retry = invocation_for(CliCommand::Cancel, Some("s1"), json!({}));
    retry.implicit_retry = true;
    let secret = invocation_for(CliCommand::Run, None, json!({"api_key": "nope"}));
    let missing_argument = invocation_for(CliCommand::Run, None, json!({}));

    for (invocation, code) in [
        (&missing_workspace, "cli_workspace_required"),
        (&missing_session, "cli_session_required"),
        (&interactive, "cli_interactive_requires_tty"),
        (&tty_output, "cli_tty_output_requires_tty"),
        (&quiet_events, "cli_output_mode_unsupported"),
        (&retry, "cli_implicit_retry_forbidden"),
        (&secret, "cli_arguments_sensitive_or_oversized"),
        (&missing_argument, "cli_argument_missing"),
    ] {
        assert_eq!(invocation.validate(), Err(code.to_owned()));
        let typed = invocation.validate_typed().expect_err("typed fence");
        assert_eq!(typed.code(), code);
        assert!(!typed.remediation().trim().is_empty());
    }

    let command_id = CliCommandId::new(CliCommand::Status, RequestId::new());
    let ansi = CliOutput::new(
        command_id.clone(),
        CliOutputMode::Json,
        ExecutionStatus::Completed,
        json!({"text": "\u{1b}[0m"}),
        None,
    );
    assert_eq!(ansi.validate(), Err("cli_json_ansi_forbidden".to_owned()));
    assert_eq!(
        ansi.validate_typed().unwrap_err(),
        CliArgumentError::JsonAnsiForbidden
    );

    let mut schema_drift = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    schema_drift.schema = "kiana.cli-command.v0".to_owned();
    assert_eq!(
        schema_drift.validate_typed().unwrap_err(),
        CliArgumentError::CommandSchemaInvalid
    );

    let mut command_mismatch = invocation_for(CliCommand::Run, None, json!({"prompt": "hi"}));
    command_mismatch.command_id = CliCommandId::new(CliCommand::Status, RequestId::new());
    assert_eq!(
        command_mismatch.validate_typed().unwrap_err(),
        CliArgumentError::CommandIdMismatch
    );
}
