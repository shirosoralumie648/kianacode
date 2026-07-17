use crate::types::{
    Command, CommandContext, CommandResult, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

const SUITE_SCHEMA: &str = "kiana.eval-suite.v1";
const REPORT_SCHEMA: &str = "kiana.eval-report.v1";
const BASELINE_SCHEMA: &str = "kiana.eval-baseline.v1";
const MAX_CASES: usize = 256;
const MAX_FIXTURE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_FIXTURE_LINES: usize = 100_000;

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

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let args = command_context_argv(&context).unwrap_or_else(|| split_words(&context.args));
        if args.is_empty()
            || args
                .iter()
                .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
        {
            return Ok(CommandResult::text(usage()));
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
    let suite: EvalSuite =
        serde_json::from_slice(&suite_bytes).context("invalid eval suite JSON")?;
    validate_suite(&suite)?;

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
        schema: REPORT_SCHEMA,
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
        schema: BASELINE_SCHEMA,
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
    if baseline.schema != BASELINE_SCHEMA {
        return Err(anyhow!(
            "unsupported eval baseline schema '{}'; expected {BASELINE_SCHEMA}",
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
    if baseline.cases.len() > MAX_CASES {
        return Err(anyhow!("eval baseline exceeds {MAX_CASES} cases"));
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
    if suite.schema != SUITE_SCHEMA {
        return Err(anyhow!(
            "unsupported eval suite schema '{}'; expected {SUITE_SCHEMA}",
            suite.schema
        ));
    }
    if suite.id.trim().is_empty() {
        return Err(anyhow!("eval suite id must not be empty"));
    }
    if suite.cases.is_empty() {
        return Err(anyhow!("eval suite must contain at least one case"));
    }
    if suite.cases.len() > MAX_CASES {
        return Err(anyhow!("eval suite exceeds {MAX_CASES} cases"));
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
    if metadata.len() > MAX_FIXTURE_BYTES {
        return Err(anyhow!(
            "eval fixture '{}' exceeds {} bytes",
            case.fixture,
            MAX_FIXTURE_BYTES
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
        if metrics.event_count as usize >= MAX_FIXTURE_LINES {
            return Err(anyhow!(
                "eval fixture for case '{case_id}' exceeds {MAX_FIXTURE_LINES} events"
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
    "Usage: kiana eval run --suite <path> [--baseline <path>] [--json] [--fail-on-failure]\n\nRuns a deterministic, read-only RuntimeEvent JSONL evaluation suite."
}
