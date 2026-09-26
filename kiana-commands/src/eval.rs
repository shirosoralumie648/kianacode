use crate::types::{
    Command, CommandContext, CommandResult, CommandRoute, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

/// Stable legacy eval wire schema names. Removing or renaming one requires an explicit migration
/// entry and a compatibility fixture; callers must not duplicate these literals.
pub const EVAL_SUITE_SCHEMA: &str = "kiana.eval-suite.v1";
pub const EVAL_REPORT_SCHEMA: &str = "kiana.eval-report.v1";
pub const EVAL_BASELINE_SCHEMA: &str = "kiana.eval-baseline.v1";
pub const EVAL_MAX_CASES: usize = 256;
pub const EVAL_MAX_FIXTURE_BYTES: u64 = 16 * 1024 * 1024;
pub const EVAL_MAX_FIXTURE_LINES: usize = 100_000;

/// Versioned command contract for the migrated evaluation CLI surface.
///
/// The legacy `run --suite <path>` form deliberately remains a compatibility adapter. New
/// subcommands are encoded as bounded, provider-independent requests and sent through the
/// existing `DaemonHost -> ControlPlane` command route. This module never executes a model,
/// provider, broker or second runner loop.
pub const EVAL_CLI_COMMAND_SCHEMA: &str = "kiana.eval-cli.v1";
pub const EVAL_CLI_COMMAND_VERSION: u32 = 1;
pub const EVAL_CLI_COMMANDS: &[(&str, &str)] = &[
    ("run", "eval.run"),
    ("capture", "eval.capture"),
    ("compare", "eval.compare"),
    ("explain", "eval.explain"),
    ("list", "eval.list"),
];

/// Fields emitted by the legacy report/baseline command. This inventory is an explicit deletion
/// fence for the compatibility surface: a field may only disappear with a recorded upcast.
pub const LEGACY_EVAL_JSON_FIELDS: &[&str] = &[
    "schema",
    "suite_id",
    "suite_sha256",
    "status",
    "summary",
    "baseline",
    "cases",
    "provided",
    "path",
    "sha256",
    "findings",
    "id",
    "kind",
    "fixture",
    "fixture_sha256",
    "metrics",
    "event_count",
    "event_type_counts",
    "assistant_text_count",
    "tool_call_count",
    "tool_result_count",
    "tool_error_count",
    "tool_names",
    "input_tokens",
    "output_tokens",
    "final_status",
    "stop_reason",
    "final_text",
    "code",
    "expected",
    "actual",
    "message",
];

/// Stable error/finding codes asserted by the legacy compatibility suite.
pub const LEGACY_EVAL_ERROR_CODES: &[&str] = &[
    "baseline_case_missing_from_suite",
    "baseline_threshold_exceeded",
    "baseline_value_mismatch",
    "eval_fixture_empty",
    "eval_fixture_invalid_json",
    "eval_fixture_event_type_missing",
    "eval_fixture_event_not_object",
];

/// GitHub CI compatibility fixtures that pin schema, field and rejection behavior before the
/// domain quality DTOs replace this parser.
pub const LEGACY_EVAL_JSON_COMPATIBILITY_TESTS: &[&str] = &[
    "eval_run_reports_deterministic_runtime_metrics",
    "eval_run_compares_local_baseline_thresholds",
    "eval_run_fails_when_baseline_regresses",
    "eval_rejects_invalid_baseline_contracts",
    "eval_rejects_invalid_suite_and_fixture_contracts",
    "eval_rejects_missing_or_invalid_usage_tokens",
    "eval_rejects_symlink_fixture_escape",
];

pub struct EvalCommand;

#[async_trait]
impl Command for EvalCommand {
    fn name(&self) -> &str {
        "eval"
    }

    fn description(&self) -> &str {
        "Run deterministic offline evaluation suites"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    fn route(&self, context: &CommandContext) -> anyhow::Result<CommandRoute> {
        let args = command_context_argv(context).unwrap_or_else(|| split_words(&context.args));
        match parse_cli_route(&args)? {
            EvalCliRoute::Help | EvalCliRoute::LegacyRun => Ok(CommandRoute::Local),
            EvalCliRoute::ControlPlane { name, arguments } => {
                Ok(CommandRoute::ControlPlane { name, arguments })
            }
        }
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let args = command_context_argv(&context).unwrap_or_else(|| split_words(&context.args));
        if args.is_empty()
            || args
                .iter()
                .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
        {
            return Ok(CommandResult::text(usage()));
        }
        if matches!(parse_cli_route(&args)?, EvalCliRoute::ControlPlane { .. }) {
            return Err(anyhow!("command_requires_control_plane"));
        }
        let options = parse_args(&args)?;
        let report = run_suite(&options.suite, options.baseline.as_deref())?;
        if options.fail_on_failure && report.status == EvalStatus::Failed {
            return Err(anyhow!(
                "eval suite failed: {} case(s) failed, {} baseline finding(s)",
                report.summary.failed,
                report
                    .baseline
                    .as_ref()
                    .map(|baseline| baseline.findings.len())
                    .unwrap_or(0)
            ));
        }
        if options.json {
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }
        Ok(CommandResult::text(render_human(&report)))
    }
}

#[derive(Debug, PartialEq)]
enum EvalCliRoute {
    Help,
    /// The old fixture-path parser remains a compatibility-only local adapter.
    LegacyRun,
    ControlPlane {
        name: String,
        arguments: Value,
    },
}

/// Parse the migrated CLI surface without opening files or invoking an evaluator.
///
/// The parser deliberately keeps references opaque. Filesystem resolution belongs to the
/// server-owned quality ports, so a new command can never turn a CLI path into a local effect.
fn parse_cli_route(args: &[String]) -> Result<EvalCliRoute> {
    if args.is_empty()
        || args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        return Ok(EvalCliRoute::Help);
    }
    let action = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("{}", usage()))?;
    if !matches!(action, "run" | "capture" | "compare" | "explain" | "list") {
        return Err(anyhow!("unknown eval subcommand '{action}'\n\n{}", usage()));
    }

    // Preserve the exact old form, including its failure behavior for missing paths. The
    // compatibility parser is intentionally not mixed with the new opaque-reference contract.
    if action == "run"
        && args.iter().skip(1).any(|arg| {
            matches!(arg.as_str(), "--suite" | "--baseline" | "--fail-on-failure")
                || arg.starts_with("--suite=")
                || arg.starts_with("--baseline=")
        })
    {
        return Ok(EvalCliRoute::LegacyRun);
    }

    let (name, arguments) = match action {
        "run" => parse_run_route(args)?,
        "capture" => parse_capture_route(args)?,
        "compare" => parse_compare_route(args)?,
        "explain" => parse_explain_route(args)?,
        "list" => parse_list_route(args)?,
        _ => unreachable!("eval action was checked above"),
    };
    Ok(EvalCliRoute::ControlPlane { name, arguments })
}

fn parse_run_route(args: &[String]) -> Result<(String, Value)> {
    let mut fields = serde_json::Map::new();
    let mut output = "text";
    let mut fail_on_failure = false;
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--json" => {
                ensure_output_flag(&mut output)?;
            }
            "--fail-on-failure" => {
                if fail_on_failure {
                    return Err(anyhow!("duplicate option --fail-on-failure"));
                }
                fail_on_failure = true;
            }
            "--suite-id" | "--dataset-id" | "--experiment-id" | "--case-id" | "--target"
            | "--baseline-id" => {
                let key = token.trim_start_matches('-').replace('-', "_");
                let value = next_cli_value(args, &mut index, token)?;
                insert_unique_ref(&mut fields, &key, value, token)?;
            }
            value
                if value.starts_with("--suite-id=")
                    || value.starts_with("--dataset-id=")
                    || value.starts_with("--experiment-id=")
                    || value.starts_with("--case-id=")
                    || value.starts_with("--target=")
                    || value.starts_with("--baseline-id=") =>
            {
                let (key, value) = split_equals_ref(value)?;
                insert_unique_ref(&mut fields, key, value, token)?;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown eval run option '{value}'\n\n{}", usage()));
            }
            value => {
                return Err(anyhow!(
                    "unexpected eval run argument '{value}'\n\n{}",
                    usage()
                ));
            }
        }
        index += 1;
    }
    if fail_on_failure {
        fields.insert("fail_on_failure".to_owned(), Value::Bool(true));
    }
    Ok((
        wire_command("run"),
        versioned_cli_arguments("run", output, fields),
    ))
}

fn parse_capture_route(args: &[String]) -> Result<(String, Value)> {
    let mut fields = serde_json::Map::new();
    let mut output = "text";
    let mut source_count = 0usize;
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--json" => ensure_output_flag(&mut output)?,
            "--source" | "--run-id" | "--fixture" | "--name" => {
                let key = match token {
                    "--source" => "source_ref",
                    "--run-id" => "run_id",
                    "--fixture" => "fixture_ref",
                    "--name" => "trace_name",
                    _ => unreachable!(),
                };
                let value = next_cli_value(args, &mut index, token)?;
                if token != "--name" {
                    source_count += 1;
                }
                insert_unique_ref(&mut fields, key, value, token)?;
            }
            value
                if value.starts_with("--source=")
                    || value.starts_with("--run-id=")
                    || value.starts_with("--fixture=")
                    || value.starts_with("--name=") =>
            {
                let (key, value) = split_equals_ref(value)?;
                let canonical = match key {
                    "source" => "source_ref",
                    "run_id" => "run_id",
                    "fixture" => "fixture_ref",
                    "name" => "trace_name",
                    _ => unreachable!(),
                };
                if canonical != "trace_name" {
                    source_count += 1;
                }
                insert_unique_ref(&mut fields, canonical, value, token)?;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!(
                    "unknown eval capture option '{value}'\n\n{}",
                    usage()
                ));
            }
            value => {
                return Err(anyhow!(
                    "unexpected eval capture argument '{value}'\n\n{}",
                    usage()
                ));
            }
        }
        index += 1;
    }
    if source_count == 0 {
        return Err(anyhow!(
            "eval capture requires exactly one source reference\n\n{}",
            usage()
        ));
    }
    if source_count > 1 {
        return Err(anyhow!(
            "eval capture accepts exactly one source reference\n\n{}",
            usage()
        ));
    }
    Ok((
        wire_command("capture"),
        versioned_cli_arguments("capture", output, fields),
    ))
}

fn parse_compare_route(args: &[String]) -> Result<(String, Value)> {
    let mut fields = serde_json::Map::new();
    let mut output = "text";
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--json" => ensure_output_flag(&mut output)?,
            "--reference" | "--candidate" => {
                let key = token.trim_start_matches('-').to_owned() + "_ref";
                let value = next_cli_value(args, &mut index, token)?;
                insert_unique_ref(&mut fields, &key, value, token)?;
            }
            value if value.starts_with("--reference=") || value.starts_with("--candidate=") => {
                let (key, value) = split_equals_ref(value)?;
                let key = format!("{key}_ref");
                insert_unique_ref(&mut fields, &key, value, token)?;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!(
                    "unknown eval compare option '{value}'\n\n{}",
                    usage()
                ));
            }
            value => {
                return Err(anyhow!(
                    "unexpected eval compare argument '{value}'\n\n{}",
                    usage()
                ));
            }
        }
        index += 1;
    }
    for key in ["reference_ref", "candidate_ref"] {
        if !fields.contains_key(key) {
            return Err(anyhow!(
                "eval compare requires --{}\n\n{}",
                key.trim_end_matches("_ref"),
                usage()
            ));
        }
    }
    Ok((
        wire_command("compare"),
        versioned_cli_arguments("compare", output, fields),
    ))
}

fn parse_explain_route(args: &[String]) -> Result<(String, Value)> {
    let mut fields = serde_json::Map::new();
    let mut output = "text";
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--json" => ensure_output_flag(&mut output)?,
            "--case-id" | "--finding" | "--result-id" | "--target" => {
                let key = match token {
                    "--case-id" => "case_id",
                    "--finding" => "finding_code",
                    "--result-id" => "result_id",
                    "--target" => "target_ref",
                    _ => unreachable!(),
                };
                let value = next_cli_value(args, &mut index, token)?;
                insert_unique_ref(&mut fields, key, value, token)?;
            }
            value
                if value.starts_with("--case-id=")
                    || value.starts_with("--finding=")
                    || value.starts_with("--result-id=")
                    || value.starts_with("--target=") =>
            {
                let (key, value) = split_equals_ref(value)?;
                let key = match key {
                    "case_id" => "case_id",
                    "finding" => "finding_code",
                    "result_id" => "result_id",
                    "target" => "target_ref",
                    _ => unreachable!(),
                };
                insert_unique_ref(&mut fields, key, value, token)?;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!(
                    "unknown eval explain option '{value}'\n\n{}",
                    usage()
                ));
            }
            value => {
                if fields.contains_key("target_ref") {
                    return Err(anyhow!(
                        "eval explain accepts one target reference\n\n{}",
                        usage()
                    ));
                }
                insert_unique_ref(&mut fields, "target_ref", validate_cli_ref(value)?, value)?;
            }
        }
        index += 1;
    }
    if fields.is_empty() {
        return Err(anyhow!(
            "eval explain requires a target reference\n\n{}",
            usage()
        ));
    }
    if fields.len() > 1 && fields.contains_key("target_ref") {
        return Err(anyhow!(
            "eval explain accepts one target reference\n\n{}",
            usage()
        ));
    }
    Ok((
        wire_command("explain"),
        versioned_cli_arguments("explain", output, fields),
    ))
}

fn parse_list_route(args: &[String]) -> Result<(String, Value)> {
    let mut fields = serde_json::Map::new();
    let mut output = "text";
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--json" => ensure_output_flag(&mut output)?,
            "--kind" => {
                let value = next_cli_value(args, &mut index, token)?;
                insert_list_kind(&mut fields, value, token)?;
            }
            value if value.starts_with("--kind=") => {
                let (_, value) = split_equals_ref(value)?;
                insert_list_kind(&mut fields, value, token)?;
            }
            value if value.starts_with('-') => {
                return Err(anyhow!("unknown eval list option '{value}'\n\n{}", usage()));
            }
            value => {
                return Err(anyhow!(
                    "unexpected eval list argument '{value}'\n\n{}",
                    usage()
                ));
            }
        }
        index += 1;
    }
    Ok((
        wire_command("list"),
        versioned_cli_arguments("list", output, fields),
    ))
}

fn versioned_cli_arguments(
    action: &str,
    output: &str,
    mut fields: serde_json::Map<String, Value>,
) -> Value {
    fields.insert(
        "schema".to_owned(),
        Value::String(EVAL_CLI_COMMAND_SCHEMA.to_owned()),
    );
    fields.insert(
        "version".to_owned(),
        Value::Number(EVAL_CLI_COMMAND_VERSION.into()),
    );
    fields.insert("action".to_owned(), Value::String(action.to_owned()));
    fields.insert("output".to_owned(), Value::String(output.to_owned()));
    Value::Object(fields)
}

fn wire_command(action: &str) -> String {
    EVAL_CLI_COMMANDS
        .iter()
        .find_map(|(cli, wire)| (*cli == action).then_some((*wire).to_owned()))
        .expect("eval CLI command map must contain every parser action")
}

fn ensure_output_flag(output: &mut &str) -> Result<()> {
    if *output == "json" {
        return Err(anyhow!("duplicate option --json"));
    }
    *output = "json";
    Ok(())
}

fn next_cli_value(args: &[String], index: &mut usize, option: &str) -> Result<String> {
    *index += 1;
    let value = args
        .get(*index)
        .ok_or_else(|| anyhow!("{option} requires a value"))?;
    if value.starts_with('-') {
        return Err(anyhow!("{option} requires a value"));
    }
    validate_cli_ref(value)
}

fn split_equals_ref(token: &str) -> Result<(&str, String)> {
    let (raw_key, raw_value) = token
        .strip_prefix("--")
        .and_then(|value| value.split_once('='))
        .ok_or_else(|| anyhow!("invalid eval option '{token}'"))?;
    if raw_value.is_empty() {
        return Err(anyhow!("--{raw_key} requires a value"));
    }
    let key = raw_key.replace('-', "_");
    let value = validate_cli_ref(raw_value)?;
    match key.as_str() {
        "suite_id" => Ok(("suite_id", value)),
        "dataset_id" => Ok(("dataset_id", value)),
        "experiment_id" => Ok(("experiment_id", value)),
        "case_id" => Ok(("case_id", value)),
        "target" => Ok(("target", value)),
        "baseline_id" => Ok(("baseline_id", value)),
        "source" => Ok(("source", value)),
        "run_id" => Ok(("run_id", value)),
        "fixture" => Ok(("fixture", value)),
        "name" => Ok(("name", value)),
        "reference" => Ok(("reference", value)),
        "candidate" => Ok(("candidate", value)),
        "finding" => Ok(("finding", value)),
        "result_id" => Ok(("result_id", value)),
        "kind" => Ok(("kind", value)),
        _ => Err(anyhow!("unknown eval option '{token}'\n\n{}", usage())),
    }
}

fn insert_unique_ref(
    fields: &mut serde_json::Map<String, Value>,
    key: &str,
    value: String,
    option: &str,
) -> Result<()> {
    if fields
        .insert(key.to_owned(), Value::String(value))
        .is_some()
    {
        return Err(anyhow!("duplicate option {option}"));
    }
    Ok(())
}

fn insert_list_kind(
    fields: &mut serde_json::Map<String, Value>,
    value: String,
    option: &str,
) -> Result<()> {
    if !matches!(
        value.as_str(),
        "dataset" | "suite" | "case" | "trace" | "experiment" | "result"
    ) {
        return Err(anyhow!("invalid eval list kind '{value}'"));
    }
    insert_unique_ref(fields, "kind", value, option)
}

fn validate_cli_ref(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 256 || value.bytes().any(|byte| byte == 0) {
        return Err(anyhow!(
            "eval reference must be 1..=256 bytes and contain no NUL"
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(anyhow!("eval reference contains a control character"));
    }
    if value.starts_with('/') || value.starts_with('~') || value.split('/').any(|part| part == "..")
    {
        return Err(anyhow!("eval reference must not escape the project scope"));
    }
    Ok(value.to_owned())
}

#[derive(Debug)]
struct EvalOptions {
    suite: PathBuf,
    baseline: Option<PathBuf>,
    json: bool,
    fail_on_failure: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalBaseline {
    schema: String,
    suite_id: String,
    #[serde(default)]
    description: String,
    cases: BTreeMap<String, EvalBaselineCase>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalBaselineCase {
    max_event_count: Option<u64>,
    max_tool_call_count: Option<u64>,
    max_tool_result_count: Option<u64>,
    max_tool_error_count: Option<u64>,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
    required_status: Option<String>,
    required_stop_reason: Option<String>,
}

impl EvalBaselineCase {
    fn is_empty(&self) -> bool {
        self.max_event_count.is_none()
            && self.max_tool_call_count.is_none()
            && self.max_tool_result_count.is_none()
            && self.max_tool_error_count.is_none()
            && self.max_input_tokens.is_none()
            && self.max_output_tokens.is_none()
            && self.required_status.is_none()
            && self.required_stop_reason.is_none()
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalSuite {
    schema: String,
    id: String,
    #[serde(default)]
    description: String,
    cases: Vec<EvalCase>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalCase {
    id: String,
    kind: String,
    fixture: String,
    expect: EvalExpectations,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct EvalExpectations {
    final_status: Option<String>,
    stop_reason: Option<String>,
    min_event_count: Option<u64>,
    tool_call_count: Option<u64>,
    tool_error_count: Option<u64>,
    required_tool_names: Option<Vec<String>>,
    final_text_contains: Option<Vec<String>>,
    max_input_tokens: Option<u64>,
    max_output_tokens: Option<u64>,
}

impl EvalExpectations {
    fn is_empty(&self) -> bool {
        self.final_status.is_none()
            && self.stop_reason.is_none()
            && self.min_event_count.is_none()
            && self.tool_call_count.is_none()
            && self.tool_error_count.is_none()
            && self.required_tool_names.is_none()
            && self.final_text_contains.is_none()
            && self.max_input_tokens.is_none()
            && self.max_output_tokens.is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum EvalStatus {
    Passed,
    Failed,
}

#[derive(Debug, Serialize)]
struct EvalReport {
    schema: &'static str,
    suite_id: String,
    suite_sha256: String,
    status: EvalStatus,
    summary: EvalSummary,
    baseline: Option<EvalBaselineReport>,
    cases: Vec<EvalCaseReport>,
}

#[derive(Debug, Serialize)]
struct EvalBaselineReport {
    schema: &'static str,
    provided: bool,
    path: String,
    sha256: String,
    suite_id: String,
    status: EvalStatus,
    findings: Vec<EvalFinding>,
}

#[derive(Debug, Serialize)]
struct EvalSummary {
    total: u64,
    passed: u64,
    failed: u64,
    events: u64,
    tool_calls: u64,
    tool_errors: u64,
}

#[derive(Debug, Serialize)]
struct EvalCaseReport {
    id: String,
    kind: String,
    fixture: String,
    fixture_sha256: String,
    status: EvalStatus,
    metrics: EvalMetrics,
    findings: Vec<EvalFinding>,
}

#[derive(Debug, Default, Serialize)]
struct EvalMetrics {
    event_count: u64,
    event_type_counts: BTreeMap<String, u64>,
    assistant_text_count: u64,
    tool_call_count: u64,
    tool_result_count: u64,
    tool_error_count: u64,
    tool_names: Vec<String>,
    input_tokens: u64,
    output_tokens: u64,
    final_status: Option<String>,
    stop_reason: Option<String>,
    final_text: Option<String>,
}

#[derive(Debug, Serialize)]
struct EvalFinding {
    code: String,
    expected: Value,
    actual: Value,
    message: String,
}

fn parse_args(args: &[String]) -> Result<EvalOptions> {
    if args.first().map(String::as_str) != Some("run") {
        return Err(anyhow!("expected eval subcommand 'run'\n\n{}", usage()));
    }
    let mut suite = None;
    let mut baseline = None;
    let mut json = false;
    let mut fail_on_failure = false;
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--suite" => {
                if suite.is_some() {
                    return Err(anyhow!("duplicate option --suite"));
                }
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--suite requires a path"))?;
                suite = Some(PathBuf::from(value));
            }
            "--baseline" => {
                if baseline.is_some() {
                    return Err(anyhow!("duplicate option --baseline"));
                }
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("--baseline requires a path"))?;
                baseline = Some(PathBuf::from(value));
            }
            "--json" => {
                if json {
                    return Err(anyhow!("duplicate option --json"));
                }
                json = true;
            }
            "--fail-on-failure" => {
                if fail_on_failure {
                    return Err(anyhow!("duplicate option --fail-on-failure"));
                }
                fail_on_failure = true;
            }
            _ if token.starts_with("--suite=") => {
                if suite.is_some() {
                    return Err(anyhow!("duplicate option --suite"));
                }
                let value = token.trim_start_matches("--suite=");
                if value.is_empty() {
                    return Err(anyhow!("--suite requires a path"));
                }
                suite = Some(PathBuf::from(value));
            }
            _ if token.starts_with("--baseline=") => {
                if baseline.is_some() {
                    return Err(anyhow!("duplicate option --baseline"));
                }
                let value = token.trim_start_matches("--baseline=");
                if value.is_empty() {
                    return Err(anyhow!("--baseline requires a path"));
                }
                baseline = Some(PathBuf::from(value));
            }
            other => return Err(anyhow!("unknown eval option '{other}'\n\n{}", usage())),
        }
        index += 1;
    }
    Ok(EvalOptions {
        suite: suite.ok_or_else(|| anyhow!("missing required option --suite\n\n{}", usage()))?,
        baseline,
        json,
        fail_on_failure,
    })
}

fn run_suite(path: &Path, baseline_path: Option<&Path>) -> Result<EvalReport> {
    let suite_path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve eval suite {}", path.display()))?;
    if !suite_path.is_file() {
        return Err(anyhow!("eval suite is not a regular file"));
    }
    let suite_dir = suite_path
        .parent()
        .ok_or_else(|| anyhow!("eval suite has no parent directory"))?
        .canonicalize()?;
    let suite_bytes = fs::read(&suite_path)
        .with_context(|| format!("failed to read eval suite {}", path.display()))?;
    let suite_value: Value =
        serde_json::from_slice(&suite_bytes).context("invalid eval suite JSON")?;
    let suite: EvalSuite =
        serde_json::from_value(suite_value.clone()).context("invalid eval suite JSON")?;
    validate_suite(&suite)?;
    kiana_domain::adapt_legacy_eval_suite(suite_value, "legacy-eval")
        .map_err(|error| anyhow!("legacy eval quality adapter rejected suite: {error}"))?;

    let mut case_reports = Vec::with_capacity(suite.cases.len());
    for case in suite.cases {
        case_reports.push(run_case(&suite_dir, case)?);
    }
    let passed = case_reports
        .iter()
        .filter(|case| case.status == EvalStatus::Passed)
        .count() as u64;
    let total = case_reports.len() as u64;
    let failed = total - passed;
    let summary = EvalSummary {
        total,
        passed,
        failed,
        events: case_reports
            .iter()
            .map(|case| case.metrics.event_count)
            .sum(),
        tool_calls: case_reports
            .iter()
            .map(|case| case.metrics.tool_call_count)
            .sum(),
        tool_errors: case_reports
            .iter()
            .map(|case| case.metrics.tool_error_count)
            .sum(),
    };
    let baseline = baseline_path
        .map(|path| evaluate_baseline(path, &suite.id, &case_reports))
        .transpose()?;
    let baseline_failed = baseline
        .as_ref()
        .is_some_and(|baseline| baseline.status == EvalStatus::Failed);
    Ok(EvalReport {
        schema: EVAL_REPORT_SCHEMA,
        suite_id: suite.id,
        suite_sha256: sha256_hex(&suite_bytes),
        status: if failed == 0 && !baseline_failed {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        },
        summary,
        baseline,
        cases: case_reports,
    })
}

fn evaluate_baseline(
    path: &Path,
    suite_id: &str,
    case_reports: &[EvalCaseReport],
) -> Result<EvalBaselineReport> {
    let baseline_path = path
        .canonicalize()
        .with_context(|| format!("failed to resolve eval baseline {}", path.display()))?;
    if !baseline_path.is_file() {
        return Err(anyhow!("eval baseline is not a regular file"));
    }
    let baseline_bytes = fs::read(&baseline_path)
        .with_context(|| format!("failed to read eval baseline {}", path.display()))?;
    let baseline: EvalBaseline =
        serde_json::from_slice(&baseline_bytes).context("invalid eval baseline JSON")?;
    validate_baseline(&baseline, suite_id)?;

    let case_by_id: BTreeMap<&str, &EvalCaseReport> = case_reports
        .iter()
        .map(|case| (case.id.as_str(), case))
        .collect();
    let mut findings = Vec::new();
    for (case_id, expected) in &baseline.cases {
        let Some(case) = case_by_id.get(case_id.as_str()) else {
            findings.push(EvalFinding {
                code: "baseline_case_missing_from_suite".to_string(),
                expected: json!(case_id),
                actual: Value::Null,
                message: format!("baseline references unknown eval case '{case_id}'"),
            });
            continue;
        };
        evaluate_baseline_case(&mut findings, case_id, &case.metrics, expected);
    }

    Ok(EvalBaselineReport {
        schema: EVAL_BASELINE_SCHEMA,
        provided: true,
        path: baseline_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("baseline.json")
            .to_string(),
        sha256: sha256_hex(&baseline_bytes),
        suite_id: baseline.suite_id,
        status: if findings.is_empty() {
            EvalStatus::Passed
        } else {
            EvalStatus::Failed
        },
        findings,
    })
}

fn validate_baseline(baseline: &EvalBaseline, suite_id: &str) -> Result<()> {
    if baseline.schema != EVAL_BASELINE_SCHEMA {
        return Err(anyhow!(
            "unsupported eval baseline schema '{}'; expected {EVAL_BASELINE_SCHEMA}",
            baseline.schema
        ));
    }
    if baseline.suite_id != suite_id {
        return Err(anyhow!(
            "eval baseline suite_id '{}' does not match suite '{suite_id}'",
            baseline.suite_id
        ));
    }
    if baseline.cases.is_empty() {
        return Err(anyhow!("eval baseline must contain at least one case"));
    }
    if baseline.cases.len() > EVAL_MAX_CASES {
        return Err(anyhow!("eval baseline exceeds {EVAL_MAX_CASES} cases"));
    }
    for (case_id, expected) in &baseline.cases {
        if case_id.trim().is_empty() {
            return Err(anyhow!("eval baseline case id must not be empty"));
        }
        if expected.is_empty() {
            return Err(anyhow!(
                "eval baseline case '{case_id}' must define at least one threshold"
            ));
        }
    }
    let _ = &baseline.description;
    Ok(())
}

fn evaluate_baseline_case(
    findings: &mut Vec<EvalFinding>,
    case_id: &str,
    metrics: &EvalMetrics,
    baseline: &EvalBaselineCase,
) {
    if let Some(maximum) = baseline.max_event_count {
        compare_baseline_maximum(
            findings,
            case_id,
            "event_count",
            maximum,
            metrics.event_count,
        );
    }
    if let Some(maximum) = baseline.max_tool_call_count {
        compare_baseline_maximum(
            findings,
            case_id,
            "tool_call_count",
            maximum,
            metrics.tool_call_count,
        );
    }
    if let Some(maximum) = baseline.max_tool_result_count {
        compare_baseline_maximum(
            findings,
            case_id,
            "tool_result_count",
            maximum,
            metrics.tool_result_count,
        );
    }
    if let Some(maximum) = baseline.max_tool_error_count {
        compare_baseline_maximum(
            findings,
            case_id,
            "tool_error_count",
            maximum,
            metrics.tool_error_count,
        );
    }
    if let Some(maximum) = baseline.max_input_tokens {
        compare_baseline_maximum(
            findings,
            case_id,
            "input_tokens",
            maximum,
            metrics.input_tokens,
        );
    }
    if let Some(maximum) = baseline.max_output_tokens {
        compare_baseline_maximum(
            findings,
            case_id,
            "output_tokens",
            maximum,
            metrics.output_tokens,
        );
    }
    if let Some(expected) = &baseline.required_status {
        compare_baseline_string(
            findings,
            case_id,
            "final_status",
            expected,
            metrics.final_status.as_deref(),
        );
    }
    if let Some(expected) = &baseline.required_stop_reason {
        compare_baseline_string(
            findings,
            case_id,
            "stop_reason",
            expected,
            metrics.stop_reason.as_deref(),
        );
    }
}

fn compare_baseline_maximum(
    findings: &mut Vec<EvalFinding>,
    case_id: &str,
    field: &str,
    maximum: u64,
    actual: u64,
) {
    if actual > maximum {
        findings.push(EvalFinding {
            code: "baseline_threshold_exceeded".to_string(),
            expected: json!({"case_id": case_id, "field": field, "maximum": maximum}),
            actual: json!(actual),
            message: format!(
                "baseline case '{case_id}' expected {field} <= {maximum}, got {actual}"
            ),
        });
    }
}

fn compare_baseline_string(
    findings: &mut Vec<EvalFinding>,
    case_id: &str,
    field: &str,
    expected: &str,
    actual: Option<&str>,
) {
    if actual != Some(expected) {
        findings.push(EvalFinding {
            code: "baseline_value_mismatch".to_string(),
            expected: json!({"case_id": case_id, "field": field, "value": expected}),
            actual: json!(actual),
            message: format!(
                "baseline case '{case_id}' expected {field}='{expected}', got {actual:?}"
            ),
        });
    }
}

fn validate_suite(suite: &EvalSuite) -> Result<()> {
    if suite.schema != EVAL_SUITE_SCHEMA {
        return Err(anyhow!(
            "unsupported eval suite schema '{}'; expected {EVAL_SUITE_SCHEMA}",
            suite.schema
        ));
    }
    if suite.id.trim().is_empty() {
        return Err(anyhow!("eval suite id must not be empty"));
    }
    if suite.cases.is_empty() {
        return Err(anyhow!("eval suite must contain at least one case"));
    }
    if suite.cases.len() > EVAL_MAX_CASES {
        return Err(anyhow!("eval suite exceeds {EVAL_MAX_CASES} cases"));
    }
    let mut ids = BTreeSet::new();
    for case in &suite.cases {
        if case.id.trim().is_empty() {
            return Err(anyhow!("eval case id must not be empty"));
        }
        if !ids.insert(case.id.clone()) {
            return Err(anyhow!("duplicate eval case id '{}'", case.id));
        }
        if case.kind != "runtime_event_replay" {
            return Err(anyhow!("unknown eval case kind '{}'", case.kind));
        }
        if case.fixture.trim().is_empty() {
            return Err(anyhow!("eval case '{}' fixture must not be empty", case.id));
        }
        if case.expect.is_empty() {
            return Err(anyhow!(
                "eval case '{}' must define at least one expectation",
                case.id
            ));
        }
    }
    let _ = &suite.description;
    Ok(())
}

fn run_case(suite_dir: &Path, case: EvalCase) -> Result<EvalCaseReport> {
    let fixture_relative = PathBuf::from(&case.fixture);
    if fixture_relative.is_absolute()
        || fixture_relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!(
            "eval fixture '{}' is outside suite directory",
            case.fixture
        ));
    }
    let fixture_path = suite_dir
        .join(&fixture_relative)
        .canonicalize()
        .with_context(|| format!("failed to resolve eval fixture '{}'", case.fixture))?;
    if !fixture_path.starts_with(suite_dir) {
        return Err(anyhow!(
            "eval fixture '{}' is outside suite directory",
            case.fixture
        ));
    }
    if !fixture_path.is_file() {
        return Err(anyhow!(
            "eval fixture '{}' is not a regular file",
            case.fixture
        ));
    }
    let metadata = fs::metadata(&fixture_path)?;
    if metadata.len() > EVAL_MAX_FIXTURE_BYTES {
        return Err(anyhow!(
            "eval fixture '{}' exceeds {} bytes",
            case.fixture,
            EVAL_MAX_FIXTURE_BYTES
        ));
    }
    let fixture_bytes = fs::read(&fixture_path)?;
    let fixture_text = std::str::from_utf8(&fixture_bytes)
        .with_context(|| format!("eval fixture '{}' is not UTF-8", case.fixture))?;
    let metrics = replay_metrics(fixture_text, &case.id)?;
    let findings = evaluate_expectations(&metrics, &case.expect);
    let status = if findings.is_empty() {
        EvalStatus::Passed
    } else {
        EvalStatus::Failed
    };
    Ok(EvalCaseReport {
        id: case.id,
        kind: case.kind,
        fixture: case.fixture,
        fixture_sha256: sha256_hex(&fixture_bytes),
        status,
        metrics,
        findings,
    })
}

fn replay_metrics(contents: &str, case_id: &str) -> Result<EvalMetrics> {
    let mut metrics = EvalMetrics::default();
    let mut tool_names = BTreeSet::new();
    for (index, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        if metrics.event_count as usize >= EVAL_MAX_FIXTURE_LINES {
            return Err(anyhow!(
                "eval fixture for case '{case_id}' exceeds {EVAL_MAX_FIXTURE_LINES} events"
            ));
        }
        let record: Value = serde_json::from_str(line).with_context(|| {
            format!(
                "invalid JSONL in eval case '{case_id}' at line {}",
                index + 1
            )
        })?;
        let event = record.get("event").unwrap_or(&record);
        let event = event.as_object().ok_or_else(|| {
            anyhow!(
                "invalid JSONL in eval case '{case_id}' at line {}: event is not an object",
                index + 1
            )
        })?;
        let event_type = string_field(event, &["type", "kind"]).ok_or_else(|| {
            anyhow!(
                "invalid JSONL in eval case '{case_id}' at line {}: missing event type",
                index + 1
            )
        })?;
        metrics.event_count += 1;
        *metrics
            .event_type_counts
            .entry(event_type.clone())
            .or_default() += 1;

        if let Some(text) = assistant_text(event) {
            metrics.assistant_text_count += 1;
            metrics.final_text = Some(text);
        }
        if is_tool_call(&event_type) {
            metrics.tool_call_count += 1;
        }
        if is_tool_result(&event_type) {
            metrics.tool_result_count += 1;
            if event
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                || event.get("error").is_some_and(|value| !value.is_null())
                || string_field(event, &["status"]).is_some_and(|status| status == "failed")
            {
                metrics.tool_error_count += 1;
            }
        }
        if let Some(tool_name) = string_field(event, &["tool_name", "name"]) {
            if is_tool_call(&event_type) || is_tool_result(&event_type) {
                tool_names.insert(tool_name);
            }
        }
        let usage_event = event_type == "usage";
        metrics.input_tokens += token_field(
            event,
            &["input_tokens", "prompt_tokens"],
            usage_event,
            "input",
            case_id,
            index + 1,
        )?;
        metrics.output_tokens += token_field(
            event,
            &["output_tokens", "completion_tokens"],
            usage_event,
            "output",
            case_id,
            index + 1,
        )?;
        if is_result(&event_type) {
            metrics.final_status = string_field(event, &["status"]);
            metrics.stop_reason = string_field(event, &["stop_reason", "stopReason"]);
        }
    }
    if metrics.event_count == 0 {
        return Err(anyhow!(
            "eval fixture for case '{case_id}' contains no events"
        ));
    }
    metrics.tool_names = tool_names.into_iter().collect();
    Ok(metrics)
}

fn evaluate_expectations(metrics: &EvalMetrics, expect: &EvalExpectations) -> Vec<EvalFinding> {
    let mut findings = Vec::new();
    if let Some(expected) = &expect.final_status {
        compare_string(
            &mut findings,
            "final_status_mismatch",
            "final_status",
            expected,
            metrics.final_status.as_deref(),
        );
    }
    if let Some(expected) = &expect.stop_reason {
        compare_string(
            &mut findings,
            "stop_reason_mismatch",
            "stop_reason",
            expected,
            metrics.stop_reason.as_deref(),
        );
    }
    if let Some(expected) = expect.min_event_count {
        if metrics.event_count < expected {
            findings.push(number_finding(
                "min_event_count_not_met",
                expected,
                metrics.event_count,
                format!(
                    "expected event_count >= {expected}, got {}",
                    metrics.event_count
                ),
            ));
        }
    }
    if let Some(expected) = expect.tool_call_count {
        compare_number(
            &mut findings,
            "tool_call_count_mismatch",
            "tool_call_count",
            expected,
            metrics.tool_call_count,
        );
    }
    if let Some(expected) = expect.tool_error_count {
        compare_number(
            &mut findings,
            "tool_error_count_mismatch",
            "tool_error_count",
            expected,
            metrics.tool_error_count,
        );
    }
    if let Some(required) = &expect.required_tool_names {
        for name in required {
            if !metrics.tool_names.contains(name) {
                findings.push(EvalFinding {
                    code: "required_tool_name_missing".to_string(),
                    expected: json!(name),
                    actual: json!(metrics.tool_names),
                    message: format!("required tool name '{name}' was not observed"),
                });
            }
        }
    }
    if let Some(required) = &expect.final_text_contains {
        let final_text = metrics.final_text.as_deref().unwrap_or_default();
        for fragment in required {
            if !final_text.contains(fragment) {
                findings.push(EvalFinding {
                    code: "final_text_missing_fragment".to_string(),
                    expected: json!(fragment),
                    actual: json!(metrics.final_text),
                    message: format!("final_text does not contain '{fragment}'"),
                });
            }
        }
    }
    if let Some(maximum) = expect.max_input_tokens {
        compare_maximum(
            &mut findings,
            "input_token_limit_exceeded",
            "input_tokens",
            maximum,
            metrics.input_tokens,
        );
    }
    if let Some(maximum) = expect.max_output_tokens {
        compare_maximum(
            &mut findings,
            "output_token_limit_exceeded",
            "output_tokens",
            maximum,
            metrics.output_tokens,
        );
    }
    findings
}

fn compare_string(
    findings: &mut Vec<EvalFinding>,
    code: &str,
    field: &str,
    expected: &str,
    actual: Option<&str>,
) {
    if actual != Some(expected) {
        findings.push(EvalFinding {
            code: code.to_string(),
            expected: json!(expected),
            actual: json!(actual),
            message: format!("expected {field}='{expected}', got {actual:?}"),
        });
    }
}

fn compare_number(
    findings: &mut Vec<EvalFinding>,
    code: &str,
    field: &str,
    expected: u64,
    actual: u64,
) {
    if actual != expected {
        findings.push(number_finding(
            code,
            expected,
            actual,
            format!("expected {field}={expected}, got {actual}"),
        ));
    }
}

fn compare_maximum(
    findings: &mut Vec<EvalFinding>,
    code: &str,
    field: &str,
    maximum: u64,
    actual: u64,
) {
    if actual > maximum {
        findings.push(number_finding(
            code,
            maximum,
            actual,
            format!("expected {field} <= {maximum}, got {actual}"),
        ));
    }
}

fn number_finding(code: &str, expected: u64, actual: u64, message: String) -> EvalFinding {
    EvalFinding {
        code: code.to_string(),
        expected: json!(expected),
        actual: json!(actual),
        message,
    }
}

fn string_field(event: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| event.get(*key).and_then(Value::as_str))
        .map(ToString::to_string)
}

fn token_field(
    event: &serde_json::Map<String, Value>,
    keys: &[&str],
    required: bool,
    direction: &str,
    case_id: &str,
    line: usize,
) -> Result<u64> {
    for key in keys {
        if let Some(value) = event.get(*key) {
            return value.as_u64().ok_or_else(|| {
                anyhow!(
                    "invalid JSONL in eval case '{case_id}' at line {line}: token field '{key}' must be a non-negative integer"
                )
            });
        }
    }
    if required {
        return Err(anyhow!(
            "invalid JSONL in eval case '{case_id}' at line {line}: usage event is missing {direction} token field"
        ));
    }
    Ok(0)
}

fn assistant_text(event: &serde_json::Map<String, Value>) -> Option<String> {
    string_field(event, &["assistant_text", "text"]).or_else(|| {
        event
            .get("message")?
            .get("text")?
            .as_str()
            .map(ToString::to_string)
    })
}

fn is_tool_call(event_type: &str) -> bool {
    matches!(event_type, "tool_call" | "tool_use" | "tool_started")
}

fn is_tool_result(event_type: &str) -> bool {
    matches!(event_type, "tool_result" | "tool_completed" | "tool_failed")
}

fn is_result(event_type: &str) -> bool {
    matches!(event_type, "result" | "turn_result" | "runtime_result")
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn render_human(report: &EvalReport) -> String {
    let baseline = report
        .baseline
        .as_ref()
        .map(|baseline| {
            format!(
                "\nbaseline: {} ({} finding(s))",
                match baseline.status {
                    EvalStatus::Passed => "passed",
                    EvalStatus::Failed => "failed",
                },
                baseline.findings.len()
            )
        })
        .unwrap_or_default();
    format!(
        "Eval suite: {}\nstatus: {}\ncases: {}/{} passed\nevents: {}\ntool_calls: {}\ntool_errors: {}{}",
        report.suite_id,
        match report.status {
            EvalStatus::Passed => "passed",
            EvalStatus::Failed => "failed",
        },
        report.summary.passed,
        report.summary.total,
        report.summary.events,
        report.summary.tool_calls,
        report.summary.tool_errors,
        baseline
    )
}

fn command_context_argv(context: &CommandContext) -> Option<Vec<String>> {
    context
        .app_state
        .get(COMMAND_ARGV_APP_STATE_KEY)?
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
}

fn split_words(input: &str) -> Vec<String> {
    input.split_whitespace().map(ToString::to_string).collect()
}

fn usage() -> &'static str {
    "Usage: kiana eval <run|capture|compare|explain|list> [options]\n\n  kiana eval run --suite <path> [--baseline <path>] [--json] [--fail-on-failure]\n      Legacy fixture-path compatibility form.\n  kiana eval run [--suite-id ID] [--dataset-id ID] [--experiment-id ID] [--json]\n      Route a provider-independent evaluation request through ControlPlane.\n  kiana eval capture --source REF [--name NAME] [--json]\n  kiana eval compare --reference REF --candidate REF [--json]\n  kiana eval explain (--case-id ID|--finding CODE|--result-id ID|--target REF) [--json]\n  kiana eval list [--kind dataset|suite|case|trace|experiment|result] [--json]"
}
