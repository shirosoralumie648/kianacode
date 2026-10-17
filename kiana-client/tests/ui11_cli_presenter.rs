//! UI-11 completion fixtures: JSON/TTY/quiet presentation, the stable exit-code table, the
//! adapter writer contract and Web/CLI agreement on one server response.
//!
//! Every case is deny-first: input that must not be presented is asserted before the goldens,
//! and the presenter is only ever handed a server-owned [`CliOutput`].

use kiana_client::{
    exit_code_after_write, present_cli_output, write_cli_presentation, CliCommand, CliCommandId,
    CliExitCode, CliLocale, CliOutput, CliOutputMode, CliPagerPlan, CliPresentation,
    CliPresenterOptions, CliSignal, CliStreamFault, CLI_DEFAULT_PAGE_BYTES, CLI_DEFAULT_TTY_BYTES,
    CLI_EXIT_CODE_TABLE, CLI_MAX_OUTPUT_BYTES, CLI_MAX_PAGER_FOOTER_BYTES, CLI_MAX_PAGE_COUNT,
    CLI_MIN_PAGE_BYTES, CLI_PAGER_FOOTER_PREFIX,
};
use kiana_protocol::{
    stable_error_from_response, CapabilityErrorCode, ExecutionStatus, RequestId, ResponseEnvelope,
    PROTOCOL_SCHEMA,
};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::{self, Write};

const FIXTURE: &str = include_str!("fixtures/ui11-cli-presenter.json");

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("UI-11 fixture must stay valid JSON")
}

fn request_id() -> RequestId {
    let value = fixture()["request_id"]
        .as_str()
        .expect("fixture request id")
        .to_owned();
    RequestId::parse_str(&value).expect("fixture request id must be a uuid")
}

fn status_of(name: &str) -> ExecutionStatus {
    match name {
        "accepted" => ExecutionStatus::Accepted,
        "queued" => ExecutionStatus::Queued,
        "cancelling" => ExecutionStatus::Cancelling,
        "denied" => ExecutionStatus::Denied,
        "awaiting_approval" => ExecutionStatus::AwaitingApproval,
        "running" => ExecutionStatus::Running,
        "completed" => ExecutionStatus::Completed,
        "failed" => ExecutionStatus::Failed,
        "cancelled" => ExecutionStatus::Cancelled,
        "result_unknown" => ExecutionStatus::ResultUnknown,
        "blocked" => ExecutionStatus::Blocked,
        other => panic!("fixture uses an unknown status: {other}"),
    }
}

fn warnings_of(case: &Value) -> Vec<String> {
    let mut warnings: Vec<String> = case["warnings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    if let Some(repeat) = case["warning_repeat"].as_u64() {
        if let Some(first) = warnings.first_mut() {
            let unit = first.chars().next().unwrap_or('x');
            *first = std::iter::repeat(unit).take(repeat as usize).collect();
        }
    }
    warnings
}

fn error_of(case: &Value) -> Option<String> {
    let mut error = case["error"].as_str().map(str::to_owned);
    if let Some(repeat) = case["error_repeat"].as_u64() {
        let unit = error
            .as_deref()
            .and_then(|value| value.chars().next())
            .unwrap_or('x');
        error = Some(std::iter::repeat(unit).take(repeat as usize).collect());
    }
    error
}

fn payload_of(case: &Value) -> Value {
    if case["payload_kind"].as_str() == Some("many_lines") {
        let count = case["line_count"].as_u64().unwrap_or(0) as usize;
        let lines: Vec<Value> = (1..=count)
            .map(|index| json!(format!("line-{index:04}")))
            .collect();
        return json!({ "digest": "sha256:many", "lines": lines });
    }
    case.get("payload")
        .cloned()
        .unwrap_or_else(|| fixture()["payload"].clone())
}

fn build_output(case: &Value) -> CliOutput {
    let mode = case["mode"].as_str().expect("fixture case mode").to_owned();
    let status = case["status"]
        .as_str()
        .expect("fixture case status")
        .to_owned();
    CliOutput::new(
        CliCommandId::new(
            CliCommand::parse("status").expect("status is a canonical command"),
            request_id(),
        ),
        CliOutputMode::parse(&mode).expect("fixture case mode must parse"),
        status_of(&status),
        payload_of(case),
        error_of(case),
    )
    .with_warnings(warnings_of(case))
}

fn options_from(case: &Value) -> CliPresenterOptions {
    CliPresenterOptions {
        tty: case["tty"].as_bool().unwrap_or(false),
        locale: match case["locale"].as_str().unwrap_or("en") {
            "zh" => CliLocale::Chinese,
            _ => CliLocale::English,
        },
        paginate: case["paginate"].as_bool().unwrap_or(false),
        max_tty_bytes: case["max_tty_bytes"].as_u64().unwrap_or(65_536) as usize,
        page_bytes: case["page_bytes"].as_u64().unwrap_or(4_096) as usize,
    }
}

fn keys_of(expect: &Value, field: &str) -> Vec<String> {
    expect[field]
        .as_array()
        .expect("fixture key list")
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The JSON stream contract: one versioned DTO, one line, no ANSI, stable key order and the
/// structured error preserved verbatim (never flattened into a log string).
fn check_json_document(text: &str, keys: &[String], output: &CliOutput, name: &str) {
    assert!(
        text.ends_with('\n'),
        "{name}: a json document must end with a newline"
    );
    assert_eq!(
        text.matches('\n').count(),
        1,
        "{name}: a json document must stay on one line so a log line cannot hide inside it"
    );
    assert!(
        !text.contains('\u{1b}'),
        "{name}: json output must never carry ansi escapes"
    );
    let document = text.trim_end_matches('\n');
    let value: Value =
        serde_json::from_str(document).unwrap_or_else(|error| panic!("{name}: {error}"));
    assert_eq!(value["schema"], "kiana.cli-output.v1", "{name}");
    assert_eq!(value["payload"], output.payload, "{name}");
    assert_eq!(
        value["command_id"]["command"],
        output.command_id.command.wire_name(),
        "{name}"
    );
    assert_eq!(
        value["command_id"]["request_id"],
        output.command_id.request_id.as_uuid().to_string(),
        "{name}"
    );
    assert_eq!(value["mode"], output.mode.wire_name(), "{name}");
    match &output.error {
        Some(error) => assert_eq!(value["error"], Value::String(error.clone()), "{name}"),
        None => assert_eq!(value["error"], Value::Null, "{name}"),
    }
    if output.warnings.is_empty() {
        assert!(value.get("warnings").is_none(), "{name}");
    } else {
        assert_eq!(value["warnings"], json!(output.warnings), "{name}");
    }

    let mut previous = 0usize;
    for key in keys {
        let needle = format!("\"{key}\":");
        let position = document
            .find(&needle)
            .unwrap_or_else(|| panic!("{name}: missing key {needle}"));
        assert!(
            position > previous || (previous == 0 && position == 1),
            "{name}: key {needle} moved, the wire order is a contract"
        );
        previous = position;
    }
}

/// Paginated TTY output must be a pure slice of the same bounded body plus a bounded footer:
/// pages concatenate back byte-for-byte and nothing is dropped, duplicated or reordered.
fn check_paginated(
    presentation: &CliPresentation,
    output: &CliOutput,
    options: CliPresenterOptions,
    expect: &Value,
    name: &str,
) {
    let plan = presentation
        .pager
        .as_ref()
        .unwrap_or_else(|| panic!("{name}: a paginated presentation must carry a pager plan"));
    assert_eq!(
        plan.page,
        expect["page"].as_u64().unwrap_or(1) as usize,
        "{name}"
    );
    assert_eq!(
        plan.page_count,
        expect["page_count"].as_u64().unwrap() as usize,
        "{name}"
    );
    assert_eq!(
        plan.line_count,
        expect["line_count"].as_u64().unwrap() as usize,
        "{name}"
    );
    assert_eq!(plan.page_bytes, options.page_bytes, "{name}");

    let unpaginated = CliPresenterOptions {
        paginate: false,
        ..options
    };
    let body = present_cli_output(output, unpaginated)
        .unwrap_or_else(|error| panic!("{name}: unpaginated body failed: {error}"))
        .stdout;
    assert_eq!(
        body.len(),
        expect["body_bytes"].as_u64().unwrap() as usize,
        "{name}: the bounded TTY body changed shape"
    );

    let mut rebuilt = String::new();
    let mut single_line_page = false;
    for page in 1..=plan.page_count {
        let text = plan
            .page_text(&body, page)
            .unwrap_or_else(|| panic!("{name}: page {page} is missing"));
        assert!(
            !text.is_empty() || plan.page_count == 1,
            "{name}: page {page} is empty"
        );
        assert!(
            text.len() <= plan.page_bytes || text.matches('\n').count() == 1,
            "{name}: page {page} is neither inside the budget nor one whole line"
        );
        if text.len() > plan.page_bytes {
            single_line_page = true;
        }
        rebuilt.push_str(text);
    }
    assert_eq!(
        rebuilt, body,
        "{name}: pages must concatenate back to the bounded body"
    );
    assert_eq!(
        single_line_page,
        expect["require_a_single_line_page"]
            .as_bool()
            .unwrap_or(false),
        "{name}"
    );

    assert!(plan.page_text(&body, 0).is_none(), "{name}");
    assert!(
        plan.page_text(&body, plan.page_count + 1).is_none(),
        "{name}"
    );
    assert_eq!(
        plan.next_page(),
        if plan.page_count > 1 { Some(2) } else { None },
        "{name}"
    );
    assert_eq!(
        plan.remaining_pages(),
        plan.page_count - plan.page,
        "{name}"
    );

    let footer = plan.footer();
    assert!(footer.starts_with(CLI_PAGER_FOOTER_PREFIX), "{name}");
    assert!(
        footer.contains(&format!("{}/{}", plan.page, plan.page_count)),
        "{name}: the footer reports the page position"
    );
    let expected_stdout = format!(
        "{}{}\n",
        plan.page_text(&body, plan.page).unwrap_or_default(),
        footer
    );
    assert_eq!(presentation.stdout, expected_stdout, "{name}");
    assert!(
        presentation.stdout.len() <= options.max_tty_bytes,
        "{name}: a paginated page plus footer stays inside the body bound"
    );
}

#[test]
fn the_documented_exit_table_matches_the_code_that_produces_it() {
    let fixture = fixture();
    let rows = fixture["exit_codes"]
        .as_array()
        .expect("exit code table rows");
    assert_eq!(rows.len(), CLI_EXIT_CODE_TABLE.len());
    assert_eq!(rows.len(), CliExitCode::ALL.len());

    let mut names = BTreeSet::new();
    let mut codes = BTreeSet::new();
    for (row, (code, name, meaning)) in rows.iter().zip(CLI_EXIT_CODE_TABLE.iter()) {
        let expected_code = row["code"].as_i64().expect("numeric exit code");
        assert_eq!(code.as_i32() as i64, expected_code);
        assert_eq!(code.wire_name(), *name);
        assert_eq!(code.meaning(), *meaning);
        assert_eq!(CliExitCode::parse_name(name), Some(*code));
        assert_eq!(
            code.is_success(),
            row["success"].as_bool().expect("success flag")
        );
        assert!(
            names.insert((*name).to_string()),
            "duplicate exit name {name}"
        );
        assert!(
            codes.insert(expected_code),
            "duplicate exit code {expected_code}"
        );
    }
    assert!(CliExitCode::Success.is_success());
    assert_eq!(CliExitCode::Unknown.as_i32(), 8);
    assert!(
        !CliExitCode::Unknown.is_success(),
        "result_unknown must never look like success"
    );
    assert_eq!(CliExitCode::Cancelled.as_i32(), 130);
    assert_eq!(CliExitCode::PermissionDenied.as_i32(), 3);
    assert_eq!(CliExitCode::BrokenPipe.as_i32(), 141);
    assert_eq!(CliExitCode::parse_name("not_a_class"), None);
}

#[test]
fn lifecycle_scenarios_use_the_declared_exit_classes() {
    let fixture = fixture();
    for row in fixture["exit_code_scenarios"]
        .as_array()
        .expect("exit code scenarios")
    {
        let name = row["name"].as_str().expect("scenario name");
        let status = status_of(row["status"].as_str().expect("scenario status"));
        let error = row["error"].as_str();
        let code = CliExitCode::for_output(status, error);
        assert_eq!(
            code.as_i32() as i64,
            row["code"].as_i64().expect("scenario code"),
            "{name}"
        );
        assert_eq!(
            code.wire_name(),
            row["name_expected"].as_str().expect("scenario class"),
            "{name}"
        );
        assert_eq!(code.is_success(), row["code"].as_i64() == Some(0), "{name}");
    }
}

#[test]
fn signals_and_stream_faults_map_to_stable_codes_without_installing_handlers() {
    let fixture = fixture();
    for row in fixture["signals"].as_array().expect("signal rows") {
        let name = row["name"].as_str().expect("signal name");
        let signal = match name {
            "sigint" => CliSignal::Sigint,
            "broken_pipe" => CliSignal::BrokenPipe,
            other => panic!("fixture uses an unknown signal: {other}"),
        };
        assert_eq!(
            signal.exit_code().as_i32() as i64,
            row["code"].as_i64().expect("signal code"),
            "{name}"
        );
        assert_eq!(signal.wire_name(), name);
        for alias in row["parse"].as_array().expect("signal aliases") {
            let alias = alias.as_str().expect("alias text");
            assert_eq!(CliSignal::parse(alias), Some(signal), "alias {alias}");
        }
    }
    assert_eq!(CliSignal::parse("SIGTERM"), None);
    assert_eq!(CliSignal::parse(""), None);
    assert_eq!(
        CliSignal::ALL.len(),
        fixture["signals"].as_array().unwrap().len()
    );

    for row in fixture["stream_faults"].as_array().expect("stream faults") {
        let name = row["name"].as_str().expect("fault name");
        let error = match row["kind"].as_str().expect("fault kind") {
            "broken_pipe" => io::Error::from(io::ErrorKind::BrokenPipe),
            "interrupted" => io::Error::from(io::ErrorKind::Interrupted),
            _ => io::Error::from(io::ErrorKind::PermissionDenied),
        };
        let fault = CliStreamFault::classify(&error);
        assert_eq!(
            fault.wire_name(),
            row["fault"].as_str().expect("fault"),
            "{name}"
        );
        match row["code"].as_i64() {
            Some(code) => assert_eq!(fault.exit_code().as_i32() as i64, code, "{name}"),
            None => assert_eq!(fault.exit_code(), CliExitCode::Unavailable, "{name}"),
        }
        match row["signal"].as_str() {
            Some(signal) => assert_eq!(
                fault.signal().map(CliSignal::wire_name),
                Some(signal),
                "{name}"
            ),
            None => assert!(fault.signal().is_none(), "{name}"),
        }
    }
    assert_eq!(CliStreamFault::BrokenPipe.exit_code().as_i32(), 141);
    assert_eq!(CliStreamFault::Interrupted.exit_code().as_i32(), 130);
}

#[test]
fn locale_classification_is_presentation_only() {
    let fixture = fixture();
    for row in fixture["locales"].as_array().expect("locale rows") {
        let value = row["value"].as_str().expect("locale value");
        assert_eq!(
            CliLocale::from_env_value(value).wire_name(),
            row["wire"].as_str().expect("locale wire name"),
            "{value}"
        );
    }
    assert_eq!(CliLocale::default(), CliLocale::English);
    assert_eq!(CliLocale::ALL.len(), 2);
    assert_eq!(CliLocale::English.wire_name(), "en");
    assert_eq!(CliLocale::Chinese.wire_name(), "zh");
}

#[test]
fn exit_classes_follow_the_protocol_error_table_and_fail_closed() {
    for code in CapabilityErrorCode::ALL {
        let expected = code.policy().cli_exit as i32;
        assert_eq!(
            CliExitCode::from_capability_error(*code).as_i32(),
            expected,
            "{}",
            code.as_str()
        );
        assert_eq!(
            CliExitCode::for_output(ExecutionStatus::Failed, Some(code.as_str())).as_i32(),
            expected,
            "{}",
            code.as_str()
        );
        assert_eq!(
            CapabilityErrorCode::from_reason(code.as_str()),
            *code,
            "{}",
            code.as_str()
        );
        assert_ne!(expected, 141, "{}", code.as_str());
    }
    assert_eq!(
        CliExitCode::from_capability_error(CapabilityErrorCode::ResultUnknown).as_i32(),
        8
    );
    assert_eq!(
        CliExitCode::from_capability_error(CapabilityErrorCode::Cancelled).as_i32(),
        130
    );
    assert_eq!(
        CliExitCode::from_capability_error(CapabilityErrorCode::ProjectUntrusted).as_i32(),
        3
    );
    assert_eq!(
        CliExitCode::for_output(ExecutionStatus::Blocked, Some("never_heard_of_this_reason"))
            .as_i32(),
        1,
        "an unknown reason must fail closed, not succeed and not invent a class"
    );
}

#[test]
fn golden_outputs_match_the_declared_cases() {
    let fixture = fixture();
    let cases = fixture["cases"].as_array().expect("golden cases");
    assert_eq!(cases.len(), 12);
    for case in cases {
        let name = case["name"].as_str().expect("case name");
        let output = build_output(case);
        let options = options_from(case);
        let presentation =
            present_cli_output(&output, options).unwrap_or_else(|error| panic!("{name}: {error}"));
        let expect = &case["expect"];

        assert_eq!(
            presentation.exit_status() as i64,
            expect["exit"].as_i64().expect("expected exit"),
            "{name}"
        );
        assert_eq!(
            presentation.truncated,
            expect["truncated"].as_bool().expect("truncated flag"),
            "{name}"
        );
        assert_eq!(
            presentation.pager.is_some(),
            expect["paginated"].as_bool().expect("paginated flag"),
            "{name}"
        );
        assert_eq!(
            presentation.stdout_is_pipe_safe(),
            expect["pipe_safe"].as_bool().expect("pipe safe flag"),
            "{name}"
        );
        if options.tty && output.mode == CliOutputMode::Tty {
            assert!(
                presentation.stdout.len() <= options.max_tty_bytes,
                "{name}: tty stdout must stay inside the adapter bound"
            );
        }

        match expect["stdout_kind"].as_str().expect("stdout kind") {
            "empty" => assert!(presentation.stdout.is_empty(), "{name}"),
            "text" => assert_eq!(
                presentation.stdout,
                expect["stdout"].as_str().expect("golden stdout"),
                "{name}"
            ),
            "json_document" => check_json_document(
                &presentation.stdout,
                &keys_of(expect, "stdout_keys"),
                &output,
                name,
            ),
            "paginated" => check_paginated(&presentation, &output, options, expect, name),
            other => panic!("{name}: unknown stdout kind {other}"),
        }
        match expect["stderr_kind"].as_str() {
            None => assert_eq!(
                presentation.stderr,
                expect["stderr"].as_str().unwrap_or_default(),
                "{name}"
            ),
            Some("json_document") => check_json_document(
                &presentation.stderr,
                &keys_of(expect, "stderr_keys"),
                &output,
                name,
            ),
            Some(other) => panic!("{name}: unknown stderr kind {other}"),
        }
    }
}

#[test]
fn denied_inputs_fail_closed_before_any_rendering() {
    let fixture = fixture();
    let denied = fixture["denied"].as_array().expect("denied matrix");
    assert!(denied.len() >= 12, "the deny matrix must stay complete");
    for case in denied {
        let name = case["name"].as_str().expect("denied case name");
        let output = build_output(case);
        let error = present_cli_output(&output, options_from(case))
            .err()
            .unwrap_or_else(|| panic!("{name}: must be rejected"));
        assert_eq!(error, case["code"].as_str().expect("denied code"), "{name}");
    }
}

#[test]
fn pager_plan_rejects_budgets_outside_the_contract() {
    assert_eq!(
        CliPagerPlan::plan("body\n", 0).unwrap_err(),
        "cli_pager_page_bytes_invalid"
    );
    assert_eq!(
        CliPagerPlan::plan("body\n", CLI_MIN_PAGE_BYTES - 1).unwrap_err(),
        "cli_pager_page_bytes_invalid"
    );
    assert_eq!(
        CliPagerPlan::plan("body\n", CLI_MAX_OUTPUT_BYTES + 1).unwrap_err(),
        "cli_pager_page_bytes_invalid"
    );
    let empty = CliPagerPlan::plan("", CLI_MIN_PAGE_BYTES).expect("an empty body still has a page");
    assert_eq!(empty.page_count, 1);
    assert_eq!(empty.line_count, 0);
    assert_eq!(empty.page_text("", 1), Some(""));
    assert_eq!(empty.page_text("", 2), None);
    assert_eq!(empty.next_page(), None);
    assert_eq!(empty.remaining_pages(), 0);
    assert_eq!(
        CLI_DEFAULT_PAGE_BYTES,
        CliPresenterOptions::default().page_bytes
    );
    assert_eq!(CLI_MAX_PAGE_COUNT, 512);
    assert!(CLI_MIN_PAGE_BYTES + CLI_MAX_PAGER_FOOTER_BYTES <= CLI_DEFAULT_TTY_BYTES);
    assert!(
        CliPagerPlan::plan("body\n", CLI_MIN_PAGE_BYTES)
            .expect("a minimal page budget is valid")
            .footer()
            .len()
            <= CLI_MAX_PAGER_FOOTER_BYTES
    );
}

#[derive(Clone, Copy)]
enum WriterMode {
    Collect,
    Fail(io::ErrorKind),
}

struct FaultWriter {
    mode: WriterMode,
    bytes: Vec<u8>,
    writes: usize,
}

impl FaultWriter {
    fn collecting() -> Self {
        Self {
            mode: WriterMode::Collect,
            bytes: Vec::new(),
            writes: 0,
        }
    }

    fn failing(kind: io::ErrorKind) -> Self {
        Self {
            mode: WriterMode::Fail(kind),
            bytes: Vec::new(),
            writes: 0,
        }
    }
}

impl Write for FaultWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.writes += 1;
        match self.mode {
            WriterMode::Collect => {
                self.bytes.extend_from_slice(buffer);
                Ok(buffer.len())
            }
            WriterMode::Fail(kind) => Err(io::Error::from(kind)),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn first_case(name: &str) -> Value {
    fixture()["cases"]
        .as_array()
        .expect("golden cases")
        .iter()
        .find(|case| case["name"] == name)
        .cloned()
        .unwrap_or_else(|| panic!("fixture case {name} is missing"))
}

#[test]
fn writes_stop_at_the_first_fault_and_never_hide_the_response_class() {
    let case = first_case("tty_success_zh_renders_a_bounded_text_view");
    let output = build_output(&case);
    let options = options_from(&case);
    let presentation = present_cli_output(&output, options).expect("tty presentation");
    assert!(!presentation.stdout.is_empty());
    assert!(!presentation.stderr.is_empty());

    let mut stdout = FaultWriter::collecting();
    let mut stderr = FaultWriter::collecting();
    let written = write_cli_presentation(&presentation, &mut stdout, &mut stderr)
        .expect("collecting writers cannot fail");
    assert_eq!(
        written,
        presentation.stdout.len() + presentation.stderr.len()
    );
    assert_eq!(stdout.bytes.as_slice(), presentation.stdout.as_bytes());
    assert_eq!(stderr.bytes.as_slice(), presentation.stderr.as_bytes());
    assert_eq!(stdout.writes, 1);
    assert_eq!(stderr.writes, 1);
    assert_eq!(
        exit_code_after_write(&presentation, None),
        presentation.exit_code
    );

    let mut broken = FaultWriter::failing(io::ErrorKind::BrokenPipe);
    let mut untouched = FaultWriter::collecting();
    let fault = write_cli_presentation(&presentation, &mut broken, &mut untouched)
        .expect_err("a broken pipe must surface as a fault");
    assert_eq!(fault, CliStreamFault::BrokenPipe);
    assert_eq!(
        untouched.writes, 0,
        "the second stream is never written after the first fault"
    );
    assert_eq!(
        exit_code_after_write(&presentation, Some(fault)).as_i32(),
        141
    );

    let mut interrupted = FaultWriter::failing(io::ErrorKind::Interrupted);
    let mut untouched = FaultWriter::collecting();
    let fault = write_cli_presentation(&presentation, &mut interrupted, &mut untouched)
        .expect_err("an interrupted write must surface as a fault");
    assert_eq!(fault, CliStreamFault::Interrupted);
    assert_eq!(
        exit_code_after_write(&presentation, Some(fault)).as_i32(),
        130
    );

    // A denial stays a denial: an ordinary io fault must not rewrite the server class.
    let denied = first_case("quiet_denial_suppresses_stdout_and_keeps_the_reason");
    let denied_output = build_output(&denied);
    let denied_presentation =
        present_cli_output(&denied_output, options_from(&denied)).expect("denied presentation");
    assert_eq!(denied_presentation.exit_code, CliExitCode::PermissionDenied);
    assert!(denied_presentation.stdout.is_empty());
    let mut stdout = FaultWriter::collecting();
    let mut stderr = FaultWriter::failing(io::ErrorKind::PermissionDenied);
    let fault = write_cli_presentation(&denied_presentation, &mut stdout, &mut stderr)
        .expect_err("an ordinary io fault must surface as a fault");
    assert_eq!(fault, CliStreamFault::Io);
    assert_eq!(
        exit_code_after_write(&denied_presentation, Some(fault)),
        CliExitCode::PermissionDenied
    );
    assert_eq!(
        exit_code_after_write(&denied_presentation, None),
        CliExitCode::PermissionDenied
    );
}

#[test]
fn cli_and_web_agree_on_the_same_server_response_without_touching_authority() {
    let fixture = fixture();
    let rows = fixture["parity"].as_array().expect("parity rows");
    assert!(rows.len() >= 11);
    for row in rows {
        let name = row["status"].as_str().expect("parity status");
        let status = status_of(name);
        let error = row["error"].as_str();
        let cli = CliExitCode::for_output(status, error);
        assert_eq!(
            cli.as_i32() as i64,
            row["cli_exit"].as_i64().expect("expected cli exit"),
            "{name}"
        );

        // The Web/UI projection classifies the same server response; one action must not be a
        // failure on one surface and a success on the other.
        let response = ResponseEnvelope {
            schema: PROTOCOL_SCHEMA.to_owned(),
            request_id: request_id(),
            status,
            output: Value::Null,
            error: error.map(str::to_owned),
        };
        let projection = stable_error_from_response(&response);
        match row["ui_error"].as_str() {
            None => {
                assert!(projection.is_none(), "{name}: web must not invent an error");
                assert_eq!(
                    cli.as_i32(),
                    0,
                    "{name}: cli must agree the action succeeded"
                );
            }
            Some(ui_code) => {
                assert_ne!(cli.as_i32(), 0, "{name}: a rejected action must not exit 0");
                let projection = projection
                    .unwrap_or_else(|| panic!("{name}: web must classify the same response"));
                let value = serde_json::to_value(&projection).expect("ui error is serializable");
                assert_eq!(value["code"], Value::String(ui_code.to_owned()), "{name}");
                assert_eq!(
                    value["retry"],
                    Value::String(row["retry"].as_str().expect("retry").to_owned()),
                    "{name}"
                );
                assert_eq!(value["schema"], "kiana.ui-error.v1", "{name}");
            }
        }
    }
}
