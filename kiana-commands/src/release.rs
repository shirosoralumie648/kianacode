use crate::tasks::TasksCommand;
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
use kiana_tasks::{list_workflow_runs, WorkflowStatus};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;

pub struct ReleaseCommand;

#[async_trait]
impl Command for ReleaseCommand {
    fn name(&self) -> &str {
        "release"
    }

    fn description(&self) -> &str {
        "Show release readiness gates"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = split_words(&context.args);
        match args.first().map(String::as_str) {
            None => release_readiness(),
            Some("help" | "--help" | "-h") => {
                reject_extra("release help", args.get(1..).unwrap_or_default())?;
                Ok(CommandResult::text(usage()))
            }
            Some("blockers") => release_blockers(args.get(1..).unwrap_or_default()),
            Some("evidence") => release_evidence(args.get(1..).unwrap_or_default()),
            Some("workflow-proof") => {
                release_workflow_proof(&context, args.get(1..).unwrap_or_default()).await
            }
            Some(_) => Err(anyhow!(usage())),
        }
    }
}

fn release_readiness() -> anyhow::Result<CommandResult> {
    let script = release_smoke_script_path();
    let workflow = release_smoke_workflow_path();
    let live_gate = release_live_gate_token_status();
    let mut lines = vec!["Release readiness".to_string()];
    lines.push(format!(
        "smoke_script: {} ({})",
        release_smoke_script_label(&script),
        if script.is_file() { "found" } else { "missing" }
    ));
    lines.push(format!(
        "ci_workflow: {} ({})",
        release_smoke_workflow_label(&workflow),
        if workflow.is_file() {
            "found"
        } else {
            "missing"
        }
    ));
    lines.push("gate: cargo fmt --all --check".to_string());
    lines.push("gate: cargo test --workspace --locked --offline --no-fail-fast".to_string());
    lines.push(
        "gate: cargo build --release --locked --offline -p kiana-entrypoints --bin kiana"
            .to_string(),
    );
    lines.push("gate: ./target/release/kiana --version".to_string());
    lines.push("gate: ./target/release/kiana doctor".to_string());
    lines.push("gate: temp INSTALL_DIR make install + installed kiana doctor".to_string());
    lines.push("commercial_gate: kiana release blockers --json".to_string());
    lines.push("commercial_gate: kiana release evidence --json".to_string());
    lines.push("commercial_gate: bash scripts/release-preflight.sh".to_string());
    lines.push("commercial_gate: bash scripts/provider-live-smoke.sh --required".to_string());
    lines.push("commercial_gate: bash scripts/remote-live-smoke.sh --required".to_string());
    lines.push("commercial_gate: bash scripts/sign-release-artifacts.sh".to_string());
    lines.push("commercial_gate: bash scripts/entitlement-proof-report.sh full".to_string());
    lines.push("commercial_gate: bash scripts/product-acceptance-report.sh full".to_string());
    lines.push("commercial_gate: bash scripts/release-ops-report.sh full".to_string());
    lines.push("commercial_gate: bash scripts/verify-commercial-release-artifacts.sh".to_string());
    lines.push("optional_live_gate: kiana remote-session code-session smoke --json".to_string());
    lines.push(format!("optional_live_gate_status: {}", live_gate.label()));
    if let Some(fix) = live_gate.fix() {
        lines.push(format!("optional_live_gate_fix: {fix}"));
    }
    lines.push("usage: bash scripts/release-smoke.sh".to_string());
    Ok(CommandResult::text(lines.join("\n")))
}

fn usage() -> &'static str {
    "Usage: kiana release [blockers|evidence|workflow-proof]\n       kiana release blockers [--json] [--dist-dir <dir>]\n       kiana release evidence [--json] [--dist-dir <dir>] [--path <file>]\n       kiana release workflow-proof [--json] [--run-id <run_id>|--latest-completed] [--out <file>]"
}

#[derive(Default)]
struct ReleaseWorkflowProofArgs {
    json: bool,
    run_id: Option<String>,
    latest_completed: bool,
    out: Option<PathBuf>,
}

async fn release_workflow_proof(
    context: &CommandContext,
    args: &[String],
) -> anyhow::Result<CommandResult> {
    let args = parse_release_workflow_proof_args(args)?;
    let (run_id, selection) = select_release_workflow_run(context, &args)?;
    let proof_path = resolve_command_path(
        context,
        args.out.unwrap_or_else(default_workflow_proof_path),
    );
    if let Some(parent) = proof_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create workflow proof directory {}",
                parent.display()
            )
        })?;
    }
    let proof = TasksCommand
        .execute(CommandContext {
            args: format!("workflow integrity verify --json {run_id}"),
            app_state: context.app_state.clone(),
        })
        .await?
        .value;
    fs::write(&proof_path, &proof)
        .with_context(|| format!("failed to write workflow proof {}", proof_path.display()))?;
    let proof_json: serde_json::Value = serde_json::from_str(&proof)
        .context("workflow integrity proof output was not valid JSON")?;
    let result = serde_json::json!({
        "schema": "kiana.release-workflow-proof.v1",
        "run_id": run_id,
        "workflow_id": proof_json.get("workflow_id").cloned().unwrap_or(serde_json::Value::Null),
        "proof_path": proof_path.to_string_lossy(),
        "proof_schema": proof_json.get("schema").cloned().unwrap_or(serde_json::Value::Null),
        "proof_status": proof_json.get("status").cloned().unwrap_or(serde_json::Value::Null),
        "selection": selection,
        "release_binding_schema": proof_json
            .get("release_binding")
            .and_then(|binding| binding.get("schema"))
            .cloned()
            .unwrap_or(serde_json::Value::Null),
        "env": {
            "KIANA_RELEASE_WORKFLOW_RUN_ID": run_id,
            "KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE": proof_path.to_string_lossy()
        }
    });
    if args.json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&result)?));
    }
    Ok(CommandResult::text(format!(
        "Release workflow proof\nrun_id: {}\nstatus: {}\nproof: {}\nexport KIANA_RELEASE_WORKFLOW_RUN_ID={}\nexport KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE={}",
        result["run_id"].as_str().unwrap_or("-"),
        result["proof_status"].as_str().unwrap_or("unknown"),
        proof_path.display(),
        result["run_id"].as_str().unwrap_or("-"),
        proof_path.display()
    )))
}

fn select_release_workflow_run(
    context: &CommandContext,
    args: &ReleaseWorkflowProofArgs,
) -> anyhow::Result<(String, serde_json::Value)> {
    if args.latest_completed {
        if args.run_id.is_some()
            || std::env::var("KIANA_RELEASE_WORKFLOW_RUN_ID")
                .ok()
                .is_some()
        {
            return Err(anyhow!(
                "kiana release workflow-proof --latest-completed cannot be combined with --run-id or KIANA_RELEASE_WORKFLOW_RUN_ID"
            ));
        }
        let runs = list_workflow_runs(command_cwd(context))?;
        let run = runs
            .iter()
            .find(|run| run.status == WorkflowStatus::Completed)
            .ok_or_else(|| anyhow!("no completed WorkflowRun was found for --latest-completed"))?;
        return Ok((
            run.run_id.clone(),
            serde_json::json!({
                "mode": "latest_completed",
                "updated_at_ms": run.updated_at_ms,
                "created_at_ms": run.created_at_ms,
                "workflow_id": run.workflow_id,
                "status": run.status,
                "current_node": run.current_node,
                "artifact_dir": run.artifact_dir.to_string_lossy(),
            }),
        ));
    }

    let run_id = args
        .run_id
        .clone()
        .or_else(|| std::env::var("KIANA_RELEASE_WORKFLOW_RUN_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "kiana release workflow-proof requires --run-id, --latest-completed, or KIANA_RELEASE_WORKFLOW_RUN_ID"
            )
        })?;
    Ok((run_id, serde_json::json!({ "mode": "explicit" })))
}

fn parse_release_workflow_proof_args(args: &[String]) -> anyhow::Result<ReleaseWorkflowProofArgs> {
    let mut parsed = ReleaseWorkflowProofArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" | "json" => parsed.json = true,
            "--run-id" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    anyhow!("kiana release workflow-proof --run-id requires a value")
                })?;
                parsed.run_id = Some(non_empty_string("--run-id", value)?);
            }
            value if value.starts_with("--run-id=") => {
                parsed.run_id = Some(non_empty_string(
                    "--run-id",
                    value.trim_start_matches("--run-id="),
                )?);
            }
            "--latest-completed" => parsed.latest_completed = true,
            "--out" => {
                index += 1;
                let value = args.get(index).ok_or_else(|| {
                    anyhow!("kiana release workflow-proof --out requires a value")
                })?;
                parsed.out = Some(non_empty_path("--out", value)?);
            }
            value if value.starts_with("--out=") => {
                parsed.out = Some(non_empty_path("--out", value.trim_start_matches("--out="))?);
            }
            "help" | "--help" | "-h" => return Err(anyhow!(release_workflow_proof_usage())),
            other => {
                return Err(anyhow!(
                    "unknown release workflow-proof argument '{}'\n\n{}",
                    other,
                    release_workflow_proof_usage()
                ))
            }
        }
        index += 1;
    }
    Ok(parsed)
}

fn release_workflow_proof_usage() -> &'static str {
    "Usage: kiana release workflow-proof [--json] [--run-id <run_id>|--latest-completed] [--out <file>]"
}

fn non_empty_string(flag: &str, value: &str) -> anyhow::Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{flag} requires a value"))
    } else {
        Ok(value.to_string())
    }
}

fn default_workflow_proof_path() -> PathBuf {
    std::env::var_os("KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from("dist")
                .join("proofs")
                .join("workflow")
                .join("recovery-integrity.json")
        })
}

fn resolve_command_path(context: &CommandContext, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }
    command_cwd(context).join(path)
}

fn command_cwd(context: &CommandContext) -> PathBuf {
    context
        .app_state
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Default)]
struct ReleaseBlockersArgs {
    json: bool,
    dist_dir: Option<PathBuf>,
}

fn release_blockers(args: &[String]) -> anyhow::Result<CommandResult> {
    let args = parse_release_blockers_args(args)?;
    let output = commercial_release_blockers_report(&args)?;
    Ok(CommandResult::text(output.trim_end().to_string()))
}

fn parse_release_blockers_args(args: &[String]) -> anyhow::Result<ReleaseBlockersArgs> {
    let mut parsed = ReleaseBlockersArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" | "json" => parsed.json = true,
            "--dist-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("kiana release blockers --dist-dir requires a value"))?;
                parsed.dist_dir = Some(non_empty_path("--dist-dir", value)?);
            }
            value if value.starts_with("--dist-dir=") => {
                parsed.dist_dir = Some(non_empty_path(
                    "--dist-dir",
                    value.trim_start_matches("--dist-dir="),
                )?);
            }
            "help" | "--help" | "-h" => return Err(anyhow!(release_blockers_usage())),
            other => {
                return Err(anyhow!(
                    "unknown release blockers argument '{}'\n\n{}",
                    other,
                    release_blockers_usage()
                ))
            }
        }
        index += 1;
    }
    Ok(parsed)
}

fn release_blockers_usage() -> &'static str {
    "Usage: kiana release blockers [--json] [--dist-dir <dir>]"
}

#[derive(Default)]
struct ReleaseEvidenceArgs {
    json: bool,
    dist_dir: Option<PathBuf>,
    path: Option<PathBuf>,
}

fn release_evidence(args: &[String]) -> anyhow::Result<CommandResult> {
    let args = parse_release_evidence_args(args)?;
    let (path, evidence) = read_local_rc_evidence(&args)?;
    if args.json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(
            &evidence,
        )?));
    }
    Ok(CommandResult::text(local_rc_evidence_text(
        &path, &evidence,
    )))
}

fn parse_release_evidence_args(args: &[String]) -> anyhow::Result<ReleaseEvidenceArgs> {
    let mut parsed = ReleaseEvidenceArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--json" | "json" => parsed.json = true,
            "--dist-dir" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("kiana release evidence --dist-dir requires a value"))?;
                parsed.dist_dir = Some(non_empty_path("--dist-dir", value)?);
            }
            value if value.starts_with("--dist-dir=") => {
                parsed.dist_dir = Some(non_empty_path(
                    "--dist-dir",
                    value.trim_start_matches("--dist-dir="),
                )?);
            }
            "--path" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("kiana release evidence --path requires a value"))?;
                parsed.path = Some(non_empty_path("--path", value)?);
            }
            value if value.starts_with("--path=") => {
                parsed.path = Some(non_empty_path(
                    "--path",
                    value.trim_start_matches("--path="),
                )?);
            }
            "help" | "--help" | "-h" => return Err(anyhow!(release_evidence_usage())),
            other => {
                return Err(anyhow!(
                    "unknown release evidence argument '{}'\n\n{}",
                    other,
                    release_evidence_usage()
                ))
            }
        }
        index += 1;
    }
    Ok(parsed)
}

fn release_evidence_usage() -> &'static str {
    "Usage: kiana release evidence [--json] [--dist-dir <dir>] [--path <file>]"
}

fn read_local_rc_evidence(
    args: &ReleaseEvidenceArgs,
) -> anyhow::Result<(PathBuf, serde_json::Value)> {
    let root = workspace_root()?;
    let candidates = local_rc_evidence_candidates(args, &root);
    if candidates.is_empty() {
        return Err(anyhow!(
            "no local RC evidence path candidates were available"
        ));
    }

    let mut failures = Vec::new();
    for path in &candidates {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(value)
                if value.get("schema").and_then(serde_json::Value::as_str)
                    == Some("kiana.local-rc-evidence.v1") =>
            {
                return Ok((path.clone(), value));
            }
            Ok(_) => failures.push(format!("{}: schema mismatch", path.display())),
            Err(error) => failures.push(format!("{}: invalid JSON: {error}", path.display())),
        }
    }

    let attempted = candidates
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if failures.is_empty() {
        Err(anyhow!(
            "failed to find kiana.local-rc-evidence.v1; looked at: {attempted}"
        ))
    } else {
        Err(anyhow!(
            "failed to read kiana.local-rc-evidence.v1; looked at: {attempted}; failures: {}",
            failures.join(" | ")
        ))
    }
}

fn local_rc_evidence_candidates(args: &ReleaseEvidenceArgs, workspace_root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = args.path.as_ref() {
        push_unique_path(&mut candidates, path.clone());
        return candidates;
    }

    if let Some(path) = std::env::var_os("KIANA_LOCAL_RC_EVIDENCE_OUT") {
        push_unique_path(&mut candidates, PathBuf::from(path));
    }
    if let Some(dist_dir) = args.dist_dir.as_ref() {
        push_unique_path(
            &mut candidates,
            dist_dir.join("proofs").join("local-rc-evidence.json"),
        );
    }

    let mut current = std::env::current_dir().unwrap_or_else(|_| workspace_root.to_path_buf());
    loop {
        push_unique_path(
            &mut candidates,
            current.join("proofs").join("local-rc-evidence.json"),
        );
        if let Some(dist_dir) = latest_local_rc_dir_from_pointer(
            &current.join("target").join("latest-local-rc-dir.txt"),
        ) {
            push_unique_path(
                &mut candidates,
                dist_dir.join("proofs").join("local-rc-evidence.json"),
            );
        }
        if !current.pop() {
            break;
        }
    }

    if let Some(dist_dir) = latest_local_rc_dir_from_pointer(
        &workspace_root
            .join("target")
            .join("latest-local-rc-dir.txt"),
    ) {
        push_unique_path(
            &mut candidates,
            dist_dir.join("proofs").join("local-rc-evidence.json"),
        );
    }
    push_unique_path(
        &mut candidates,
        workspace_root
            .join("dist")
            .join("proofs")
            .join("local-rc-evidence.json"),
    );
    candidates
}

fn latest_local_rc_dir_from_pointer(pointer: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(pointer).ok()?;
    let value = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let path = PathBuf::from(value);
    if path.is_absolute() {
        return Some(path);
    }
    let base = pointer
        .parent()
        .and_then(Path::parent)
        .or_else(|| pointer.parent())?;
    Some(base.join(path))
}

fn push_unique_path(candidates: &mut Vec<PathBuf>, path: PathBuf) {
    if !candidates.iter().any(|candidate| candidate == &path) {
        candidates.push(path);
    }
}

fn local_rc_evidence_text(path: &Path, evidence: &serde_json::Value) -> String {
    let mut lines = vec!["Local RC evidence".to_string()];
    lines.push(format!("path: {}", path.display()));
    lines.push(format!(
        "status: {}",
        value_string(evidence, "status").unwrap_or("unknown")
    ));
    if let Some(ready) = evidence
        .get("readiness")
        .and_then(|readiness| readiness.get("ready"))
        .and_then(serde_json::Value::as_bool)
    {
        lines.push(format!("readiness.ready: {ready}"));
    }
    if let Some(dist_dir) = value_string(evidence, "dist_dir") {
        lines.push(format!("dist_dir: {dist_dir}"));
    }
    if let Some(summary) = evidence
        .get("summary")
        .and_then(serde_json::Value::as_object)
    {
        for key in [
            "release_artifacts",
            "manifests",
            "proofs",
            "blockers_total",
            "local_blockers",
            "external_blockers",
        ] {
            if let Some(value) = summary.get(key).and_then(serde_json::Value::as_i64) {
                lines.push(format!("{key}: {value}"));
            }
        }
    }
    if let Some(handoff_status) = evidence
        .get("blockers")
        .and_then(|blockers| blockers.get("handoff_status"))
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("handoff_status: {handoff_status}"));
    }
    if let Some(scopes) = evidence
        .get("blockers")
        .and_then(|blockers| blockers.get("blocking_by_resolution_scope"))
        .and_then(serde_json::Value::as_object)
    {
        let mut scope_counts = scopes
            .iter()
            .filter_map(|(scope, value)| value.as_i64().map(|count| format!("{scope}={count}")))
            .collect::<Vec<_>>();
        scope_counts.sort();
        if !scope_counts.is_empty() {
            lines.push(format!("resolution_scopes: {}", scope_counts.join(", ")));
        }
    }
    lines.join("\n")
}

fn value_string<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(serde_json::Value::as_str)
}

fn commercial_release_blockers_report(args: &ReleaseBlockersArgs) -> anyhow::Result<String> {
    let root = workspace_root()?;
    let run_root = release_blockers_run_root(&root);
    let mut script_command = release_blockers_script_command();
    if args.json {
        script_command.push_str(" --json");
    }
    let mut failures = Vec::new();
    for bash in release_bash_candidates() {
        let mut command = ProcessCommand::new(&bash);
        command
            .arg("-lc")
            .arg(&script_command)
            .current_dir(&run_root);
        if let Some(dist_dir) = args.dist_dir.as_ref() {
            command.env("DIST_DIR", dist_dir);
        }
        match command.output() {
            Ok(output) if output.status.success() => {
                return String::from_utf8(output.stdout)
                    .context("commercial release blockers report was not valid UTF-8");
            }
            Ok(output) => failures.push(format!(
                "{} exited with status {}: {}",
                bash.display(),
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            )),
            Err(error) => failures.push(format!("{} failed to start: {error}", bash.display())),
        }
    }
    Err(anyhow!(
        "failed to run scripts/commercial-release-blockers-report.sh: {}",
        failures.join(" | ")
    ))
}

fn release_blockers_script_command() -> String {
    std::env::var("KIANA_RELEASE_BLOCKERS_SCRIPT")
        .map(|path| format!("bash {}", shell_quote(&path)))
        .unwrap_or_else(|_| "bash ./scripts/commercial-release-blockers-report.sh".to_string())
}

fn release_blockers_run_root(workspace_root: &Path) -> PathBuf {
    if std::env::var_os("KIANA_RELEASE_BLOCKERS_SCRIPT").is_some() {
        return std::env::current_dir().unwrap_or_else(|_| workspace_root.to_path_buf());
    }
    let mut current = std::env::current_dir().unwrap_or_else(|_| workspace_root.to_path_buf());
    loop {
        if current
            .join("scripts")
            .join("commercial-release-blockers-report.sh")
            .is_file()
        {
            return current;
        }
        if !current.pop() {
            break;
        }
    }
    workspace_root.to_path_buf()
}

fn workspace_root() -> anyhow::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow!("failed to resolve Kiana workspace root"))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn release_bash_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(path) = std::env::var_os("KIANA_BASH") {
        candidates.push(PathBuf::from(path));
    }
    #[cfg(windows)]
    {
        candidates.push(PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"));
        candidates.push(PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"));
        candidates.push(PathBuf::from(
            r"C:\Program Files (x86)\Git\usr\bin\bash.exe",
        ));
    }
    candidates.push(PathBuf::from("bash"));
    candidates
}

fn non_empty_path(flag: &str, value: &str) -> anyhow::Result<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{flag} requires a value"))
    } else {
        Ok(PathBuf::from(value))
    }
}

fn reject_extra(command: &str, extra: &[String]) -> anyhow::Result<()> {
    if extra.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "unknown {command} argument '{}'\n\n{}",
            extra[0],
            usage()
        ))
    }
}

fn split_words(input: &str) -> Vec<String> {
    input.split_whitespace().map(str::to_string).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveGateTokenStatus {
    Configured(&'static str),
    Missing,
    AnthropicApiKeyMisuse,
}

impl LiveGateTokenStatus {
    fn label(self) -> String {
        match self {
            LiveGateTokenStatus::Configured(source) => format!("ready ({source})"),
            LiveGateTokenStatus::Missing => "missing".to_string(),
            LiveGateTokenStatus::AnthropicApiKeyMisuse => {
                "invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)".to_string()
            }
        }
    }

    fn fix(self) -> Option<&'static str> {
        match self {
            LiveGateTokenStatus::Configured(_) => None,
            LiveGateTokenStatus::Missing => Some(
                "set KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN to a remote bearer token",
            ),
            LiveGateTokenStatus::AnthropicApiKeyMisuse => Some(
                "set KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN to a remote bearer token",
            ),
        }
    }
}

fn release_live_gate_token_status() -> LiveGateTokenStatus {
    for source in [
        "KIANA_REMOTE_ACCESS_TOKEN",
        "CLAUDE_ACCESS_TOKEN",
        "ANTHROPIC_AUTH_TOKEN",
    ] {
        let Ok(value) = std::env::var(source) else {
            continue;
        };
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        if source == "ANTHROPIC_AUTH_TOKEN" && value.starts_with("sk-") {
            return LiveGateTokenStatus::AnthropicApiKeyMisuse;
        }
        return LiveGateTokenStatus::Configured(source);
    }
    LiveGateTokenStatus::Missing
}

fn release_smoke_script_label(path: &Path) -> String {
    if path.file_name().and_then(|value| value.to_str()) == Some("release-smoke.sh")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            == Some("scripts")
    {
        return "scripts/release-smoke.sh".to_string();
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(relative) = path.strip_prefix(cwd) {
            return relative.display().to_string();
        }
    }
    path.display().to_string()
}

fn release_smoke_script_path() -> PathBuf {
    let mut current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let candidate = current.join("scripts").join("release-smoke.sh");
        if candidate.is_file() {
            return candidate;
        }
        if !current.pop() {
            break;
        }
    }

    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    manifest_root.join("scripts").join("release-smoke.sh")
}

fn release_smoke_workflow_label(path: &Path) -> String {
    if path.file_name().and_then(|value| value.to_str()) == Some("release-smoke.yml")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            == Some("workflows")
        && path
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .and_then(|value| value.to_str())
            == Some(".github")
    {
        return ".github/workflows/release-smoke.yml".to_string();
    }
    if let Ok(cwd) = std::env::current_dir() {
        if let Ok(relative) = path.strip_prefix(cwd) {
            return relative.display().to_string();
        }
    }
    path.display().to_string()
}

fn release_smoke_workflow_path() -> PathBuf {
    let mut current = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    loop {
        let candidate = current
            .join(".github")
            .join("workflows")
            .join("release-smoke.yml");
        if candidate.is_file() {
            return candidate;
        }
        if !current.pop() {
            break;
        }
    }

    let manifest_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    manifest_root
        .join(".github")
        .join("workflows")
        .join("release-smoke.yml")
}

#[cfg(test)]
mod tests {
    use super::{release_blockers_script_command, ReleaseCommand};
    use crate::tasks::TasksCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command as ProcessCommand;
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        crate::test_env_lock()
    }

    struct EnvGuard {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(values: &[(&'static str, Option<&str>)]) -> Self {
            let previous = values
                .iter()
                .map(|(key, _)| (*key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            for (key, value) in values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self { values: previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.values {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn unique_temp_dir(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "kiana-release-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn test_bash() -> Option<String> {
        let mut candidates = Vec::new();
        #[cfg(windows)]
        {
            candidates.push(r"C:\Program Files\Git\bin\bash.exe".to_string());
            candidates.push(r"C:\Program Files\Git\usr\bin\bash.exe".to_string());
        }
        candidates.push("bash".to_string());
        candidates.into_iter().find(|candidate| {
            ProcessCommand::new(candidate)
                .arg("--version")
                .output()
                .is_ok_and(|output| output.status.success())
        })
    }

    #[test]
    fn default_release_blockers_script_command_uses_bash_interpreter() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::set(&[("KIANA_RELEASE_BLOCKERS_SCRIPT", None)]);

        assert_eq!(
            release_blockers_script_command(),
            "bash ./scripts/commercial-release-blockers-report.sh"
        );
    }

    #[tokio::test]
    async fn release_rejects_unknown_args_instead_of_reporting_readiness() {
        let result = ReleaseCommand
            .execute(CommandContext {
                args: "anything".to_string(),
                app_state: HashMap::new(),
            })
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn release_command_reports_script_and_gates() {
        let result = ReleaseCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Release readiness"));
        assert!(result
            .value
            .contains("smoke_script: scripts/release-smoke.sh (found)"));
        assert!(result
            .value
            .contains("ci_workflow: .github/workflows/release-smoke.yml (found)"));
        assert!(result
            .value
            .contains("optional_live_gate: kiana remote-session code-session smoke --json"));
        assert!(result.value.contains("gate: cargo fmt --all --check"));
        assert!(result
            .value
            .contains("gate: cargo test --workspace --locked --offline --no-fail-fast"));
        assert!(result.value.contains(
            "gate: cargo build --release --locked --offline -p kiana-entrypoints --bin kiana"
        ));
        assert!(result
            .value
            .contains("gate: ./target/release/kiana --version"));
        assert!(result.value.contains("gate: ./target/release/kiana doctor"));
        assert!(result
            .value
            .contains("gate: temp INSTALL_DIR make install + installed kiana doctor"));
        assert!(result
            .value
            .contains("commercial_gate: kiana release blockers --json"));
        assert!(result
            .value
            .contains("commercial_gate: kiana release evidence --json"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/provider-live-smoke.sh --required"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/remote-live-smoke.sh --required"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/sign-release-artifacts.sh"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/entitlement-proof-report.sh full"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/product-acceptance-report.sh full"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/release-ops-report.sh full"));
        assert!(result
            .value
            .contains("commercial_gate: bash scripts/verify-commercial-release-artifacts.sh"));
    }

    #[tokio::test]
    async fn release_blockers_json_runs_script_with_dist_dir() {
        let _lock = env_lock().lock().unwrap();
        let bash = test_bash().expect("bash is required for release blockers command test");
        let root = unique_temp_dir("blockers");
        let script = root.join("blockers.sh");
        fs::write(
            &script,
            "#!/usr/bin/env bash\nset -euo pipefail\nprintf '{\"schema\":\"kiana.commercial-release-blockers.v1\",\"dist\":\"%s\",\"args\":\"%s\"}\\n' \"${DIST_DIR:-}\" \"$*\"\n",
        )
        .unwrap();
        let script_value = script.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_BASH", Some(&bash)),
            ("KIANA_RELEASE_BLOCKERS_SCRIPT", Some(&script_value)),
            ("DIST_DIR", None),
        ]);

        let result = ReleaseCommand
            .execute(CommandContext {
                args: "blockers --json --dist-dir target/release-blockers-fixture".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.commercial-release-blockers.v1");
        assert_eq!(report["dist"], "target/release-blockers-fixture");
        assert_eq!(report["args"], "--json");

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn workflow_integrity_git_binding_round_trips_untracked_content() {
        let _lock = env_lock().lock().unwrap();
        let bash = test_bash().expect("bash is required for workflow proof round-trip test");
        let source_workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let root = unique_temp_dir("workflow-proof-round-trip");
        let workspace = root.join("repo");
        let home = root.join("home");
        fs::create_dir_all(workspace.join("scripts")).unwrap();
        fs::copy(
            source_workspace.join("scripts/commercial-release-blockers-report.sh"),
            workspace.join("scripts/commercial-release-blockers-report.sh"),
        )
        .unwrap();
        fs::copy(source_workspace.join("VERSION"), workspace.join("VERSION")).unwrap();
        fs::copy(
            source_workspace.join("Cargo.lock"),
            workspace.join("Cargo.lock"),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "release-test@example.test"],
            vec!["config", "user.name", "Release Test"],
            vec![
                "add",
                "scripts/commercial-release-blockers-report.sh",
                "VERSION",
                "Cargo.lock",
            ],
            vec!["commit", "-qm", "baseline"],
        ] {
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(&workspace)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let home_value = home.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_HOME", Some(&home_value)),
            ("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", None),
        ]);
        let command_context = |args: &str| CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                serde_json::json!(workspace.to_string_lossy()),
            )]),
        };
        TasksCommand
            .execute(command_context("workflow integrity init --json"))
            .await
            .unwrap();
        let workflow: serde_json::Value = serde_json::from_str(
            &TasksCommand
                .execute(command_context(
                    "workflow init --json release proof round trip",
                ))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let run_id = workflow["run_id"].as_str().unwrap();
        let untracked_path = workspace.join("untracked-source.txt");
        fs::write(&untracked_path, b"alpha\n").unwrap();
        let proof = TasksCommand
            .execute(command_context(&format!(
                "workflow integrity verify --json {run_id}"
            )))
            .await
            .unwrap()
            .value;
        let proof_path = workspace.join("dist/proofs/workflow/recovery-integrity.json");
        fs::create_dir_all(proof_path.parent().unwrap()).unwrap();
        fs::write(&proof_path, proof).unwrap();

        let run_report = || {
            let output = ProcessCommand::new(&bash)
                .arg("scripts/commercial-release-blockers-report.sh")
                .arg("--json")
                .current_dir(&workspace)
                .env("KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE", &proof_path)
                .env("KIANA_RELEASE_WORKFLOW_RUN_ID", run_id)
                .env(
                    "KIANA_RELEASE_WORKFLOW_RUNS_DIR",
                    workspace.join(".kiana/workflows"),
                )
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        };
        let check_evidence = |report: &serde_json::Value| {
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|check| check["id"] == "workflow.recovery-integrity")
                .unwrap()["evidence"]
                .as_str()
                .unwrap()
                .to_string()
        };

        let original = run_report();
        assert!(!check_evidence(&original).contains("Git binding"));

        fs::write(&untracked_path, b"bravo\n").unwrap();
        let mutated = run_report();
        assert!(check_evidence(&mutated).contains("Git binding"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn release_workflow_proof_json_writes_integrity_proof_file() {
        let _lock = env_lock().lock().unwrap();
        let root = unique_temp_dir("workflow-proof-command");
        let workspace = root.join("repo");
        let home = root.join("home");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(workspace.join("README.md"), "release proof fixture\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "release-test@example.test"],
            vec!["config", "user.name", "Release Test"],
            vec!["add", "README.md"],
            vec!["commit", "-qm", "baseline"],
        ] {
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(&workspace)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let home_value = home.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_HOME", Some(&home_value)),
            ("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", None),
            ("KIANA_RELEASE_WORKFLOW_RUN_ID", None),
            ("KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE", None),
        ]);
        let command_context = |args: &str| CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                serde_json::json!(workspace.to_string_lossy()),
            )]),
        };
        TasksCommand
            .execute(command_context("workflow integrity init --json"))
            .await
            .unwrap();
        let workflow: serde_json::Value = serde_json::from_str(
            &TasksCommand
                .execute(command_context(
                    "workflow init --json release proof command",
                ))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let run_id = workflow["run_id"].as_str().unwrap();
        let proof_path = workspace.join("dist/proofs/workflow/recovery-integrity.json");
        let output = ReleaseCommand
            .execute(command_context(&format!(
                "workflow-proof --json --run-id {run_id} --out {}",
                proof_path.display()
            )))
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&output.value).unwrap();
        assert_eq!(result["schema"], "kiana.release-workflow-proof.v1");
        assert_eq!(result["run_id"], run_id);
        assert_eq!(result["proof_schema"], "kiana.workflow-integrity-report.v1");
        assert_eq!(result["proof_status"], "verified");
        assert_eq!(
            result["release_binding_schema"],
            "kiana.workflow-release-binding.v1"
        );
        assert_eq!(result["env"]["KIANA_RELEASE_WORKFLOW_RUN_ID"], run_id);
        assert_eq!(
            result["env"]["KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE"],
            proof_path.to_string_lossy().as_ref()
        );
        let proof: serde_json::Value =
            serde_json::from_slice(&fs::read(&proof_path).unwrap()).unwrap();
        assert_eq!(proof["run_id"], run_id);
        assert_eq!(proof["release_binding"]["run_id"], run_id);
        assert_eq!(
            proof["release_binding"]["schema"],
            "kiana.workflow-release-binding.v1"
        );
        assert!(!output.value.contains("secret_hex"));
        assert!(!serde_json::to_string(&proof)
            .unwrap()
            .contains("secret_hex"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn release_workflow_proof_latest_completed_selects_newest_completed_run() {
        let _lock = env_lock().lock().unwrap();
        let root = unique_temp_dir("workflow-proof-latest-completed");
        let workspace = root.join("repo");
        let home = root.join("home");
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            workspace.join("README.md"),
            "release latest proof fixture\n",
        )
        .unwrap();
        fs::create_dir_all(workspace.join("scripts")).unwrap();
        let smoke_path = workspace.join("scripts/release-smoke.sh");
        fs::write(
            &smoke_path,
            "#!/usr/bin/env bash\nset -euo pipefail\nexit 0\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&smoke_path).unwrap().permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&smoke_path, permissions).unwrap();
        }
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "release-test@example.test"],
            vec!["config", "user.name", "Release Test"],
            vec!["add", "README.md"],
            vec!["add", "scripts/release-smoke.sh"],
            vec!["commit", "-qm", "baseline"],
        ] {
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(&workspace)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let home_value = home.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_HOME", Some(&home_value)),
            ("KIANA_WORKFLOW_INTEGRITY_KEY_FILE", None),
            ("KIANA_RELEASE_WORKFLOW_RUN_ID", None),
            ("KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE", None),
        ]);
        let command_context = |args: &str| CommandContext {
            args: args.to_string(),
            app_state: HashMap::from([(
                "cwd".to_string(),
                serde_json::json!(workspace.to_string_lossy()),
            )]),
        };
        TasksCommand
            .execute(command_context("workflow integrity init --json"))
            .await
            .unwrap();
        let _open_workflow: serde_json::Value = serde_json::from_str(
            &TasksCommand
                .execute(command_context(
                    "workflow init --json --type ship --profile gated open release workflow",
                ))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let completed_workflow: serde_json::Value = serde_json::from_str(
            &TasksCommand
                .execute(command_context(
                    "workflow init --json --type ship --profile gated completed release workflow",
                ))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let run_id = completed_workflow["run_id"].as_str().unwrap();
        let evidence_path = workspace
            .join(".kiana")
            .join("workflows")
            .join(run_id)
            .join("release-proof-evidence.md");
        fs::write(
            &evidence_path,
            "# Release Proof Evidence\n\nLatest completed WorkflowRun fixture evidence.\n",
        )
        .unwrap();
        for decision in [
            "clear",
            "not_needed",
            "fresh_enough",
            "ship",
            "report_only",
            "learned",
            "done",
        ] {
            TasksCommand
                .execute(command_context(&format!(
                    "workflow advance --json --decision {decision} --evidence release-proof-evidence.md {run_id}"
                )))
                .await
                .unwrap();
        }
        let validation: serde_json::Value = serde_json::from_str(
            &crate::validate::ValidateCommand
                .execute(command_context(&format!("--json --workflow {run_id}")))
                .await
                .unwrap()
                .value,
        )
        .unwrap();
        let verification_id = validation["verification_id"].as_str().unwrap();
        TasksCommand
            .execute(command_context(&format!(
                "workflow complete --json --verification {verification_id} {run_id}"
            )))
            .await
            .unwrap();

        let proof_path = workspace.join("dist/proofs/workflow/latest-recovery-integrity.json");
        let output = ReleaseCommand
            .execute(command_context(&format!(
                "workflow-proof --json --latest-completed --out {}",
                proof_path.display()
            )))
            .await
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(&output.value).unwrap();
        assert_eq!(result["schema"], "kiana.release-workflow-proof.v1");
        assert_eq!(result["run_id"], run_id);
        assert_eq!(result["selection"]["mode"], "latest_completed");
        assert_eq!(result["selection"]["status"], "completed");
        assert_eq!(result["proof_status"], "verified");
        assert_eq!(result["env"]["KIANA_RELEASE_WORKFLOW_RUN_ID"], run_id);
        assert!(proof_path.is_file());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn commercial_release_blockers_validate_workflow_recovery_integrity_proof() {
        let _lock = env_lock().lock().unwrap();
        let bash = test_bash().expect("bash is required for recovery proof contract test");
        let source_workspace = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let root = unique_temp_dir("workflow-recovery-proof");
        let workspace_dir = root.join("repo");
        fs::create_dir_all(workspace_dir.join("scripts")).unwrap();
        fs::copy(
            source_workspace.join("scripts/commercial-release-blockers-report.sh"),
            workspace_dir.join("scripts/commercial-release-blockers-report.sh"),
        )
        .unwrap();
        fs::copy(
            source_workspace.join("VERSION"),
            workspace_dir.join("VERSION"),
        )
        .unwrap();
        fs::copy(
            source_workspace.join("Cargo.lock"),
            workspace_dir.join("Cargo.lock"),
        )
        .unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "user.email", "release-test@example.test"],
            vec!["config", "user.name", "Release Test"],
            vec![
                "add",
                "scripts/commercial-release-blockers-report.sh",
                "VERSION",
                "Cargo.lock",
            ],
            vec!["commit", "-qm", "baseline"],
        ] {
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(&workspace_dir)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let workspace = workspace_dir.as_path();
        let proof_path = workspace.join("dist/proofs/workflow/recovery-integrity.json");
        fs::create_dir_all(proof_path.parent().unwrap()).unwrap();
        let workflow_runs_dir = workspace.join(".kiana/workflows");
        let key_id = format!("sha256:{}", "a".repeat(64));
        let run_id = "run-release-fixture";
        let workflow_id = "wf-release-fixture";
        let git_hash = |args: &[&str]| {
            use sha2::{Digest, Sha256};
            let output = ProcessCommand::new("git")
                .args(args)
                .current_dir(workspace)
                .output()
                .unwrap();
            assert!(output.status.success());
            let mut digest = Sha256::new();
            digest.update(output.stdout);
            format!("{:x}", digest.finalize())
        };
        let file_hash = |path: &std::path::Path| {
            use sha2::{Digest, Sha256};
            let mut digest = Sha256::new();
            digest.update(fs::read(path).unwrap());
            format!("{:x}", digest.finalize())
        };
        let workflow_artifact_dir = workflow_runs_dir.join(run_id);
        fs::create_dir_all(&workflow_artifact_dir).unwrap();
        let write_workflow_binding = |status: &str, current_node: &str, kind: &str| {
            let state_path = workflow_artifact_dir.join("state.json");
            let eventlog_path = workflow_artifact_dir.join("eventlog.jsonl");
            fs::write(
                &state_path,
                serde_json::to_vec_pretty(&serde_json::json!({
                    "schema": "kiana.workflow-state.v1",
                    "run_id": run_id,
                    "workflow_id": workflow_id,
                    "status": status,
                    "current_node": current_node,
                    "last_event_seq": 4,
                }))
                .unwrap(),
            )
            .unwrap();
            fs::write(
                &eventlog_path,
                format!(
                    "{}\n",
                    serde_json::to_string(&serde_json::json!({
                        "event": {
                            "seq": 4,
                            "kind": kind,
                        }
                    }))
                    .unwrap()
                ),
            )
            .unwrap();
            serde_json::json!({
                "status": status,
                "current_node": current_node,
                "last_event_seq": 4,
                "last_event_kind": kind,
                "state_sha256": file_hash(&state_path),
                "eventlog_sha256": file_hash(&eventlog_path),
            })
        };
        let completed_workflow_binding =
            write_workflow_binding("completed", "learn", "workflow_completed");
        let exclusions = [
            ".",
            ":(exclude).kiana/**",
            ":(exclude)target/**",
            ":(exclude)node_modules/**",
            ":(exclude).venv/**",
            ":(exclude)dist/**",
            ":(exclude)build/**",
        ];
        let mut index_args = vec!["diff", "--cached", "--binary", "--no-ext-diff", "--"];
        index_args.extend(exclusions);
        let mut worktree_args = vec!["diff", "--binary", "--no-ext-diff", "--"];
        worktree_args.extend(exclusions);
        let mut status_args = vec![
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--",
        ];
        status_args.extend(exclusions);
        let git_status_hash = || {
            use sha2::{Digest, Sha256};

            fn update_field(digest: &mut Sha256, value: &[u8]) {
                digest.update((value.len() as u64).to_be_bytes());
                digest.update(value);
            }

            let status = ProcessCommand::new("git")
                .args(&status_args)
                .current_dir(workspace)
                .output()
                .unwrap();
            assert!(status.status.success());
            let mut untracked_args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
            untracked_args.extend(exclusions);
            let untracked = ProcessCommand::new("git")
                .args(&untracked_args)
                .current_dir(workspace)
                .output()
                .unwrap();
            assert!(untracked.status.success());
            let mut paths = untracked
                .stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            paths.sort();
            if paths.is_empty() {
                let mut digest = Sha256::new();
                digest.update(status.stdout);
                return format!("{:x}", digest.finalize());
            }

            let mut digest = Sha256::new();
            update_field(&mut digest, b"kiana.release-git-status.v2");
            update_field(&mut digest, &status.stdout);
            digest.update((paths.len() as u64).to_be_bytes());
            for raw_path in paths {
                let relative = String::from_utf8(raw_path.clone()).unwrap();
                let path = workspace.join(&relative);
                let metadata = fs::symlink_metadata(&path).unwrap();
                let (mode, content) = if metadata.file_type().is_symlink() {
                    (
                        b"120000".as_slice(),
                        fs::read_link(&path)
                            .unwrap()
                            .to_string_lossy()
                            .as_bytes()
                            .to_vec(),
                    )
                } else {
                    #[cfg(unix)]
                    let executable = {
                        use std::os::unix::fs::PermissionsExt;
                        metadata.permissions().mode() & 0o111 != 0
                    };
                    #[cfg(not(unix))]
                    let executable = false;
                    (
                        if executable {
                            b"100755".as_slice()
                        } else {
                            b"100644".as_slice()
                        },
                        fs::read(&path).unwrap(),
                    )
                };
                let mut content_digest = Sha256::new();
                content_digest.update(&content);
                update_field(&mut digest, &raw_path);
                update_field(&mut digest, mode);
                update_field(&mut digest, metadata.len().to_string().as_bytes());
                update_field(
                    &mut digest,
                    format!("{:x}", content_digest.finalize()).as_bytes(),
                );
            }
            format!("{:x}", digest.finalize())
        };
        let head = ProcessCommand::new("git")
            .args(["rev-parse", "--verify", "HEAD"])
            .current_dir(workspace)
            .output()
            .unwrap();
        assert!(head.status.success());
        let head = String::from_utf8(head.stdout).unwrap().trim().to_string();
        let valid = serde_json::json!({
            "schema": "kiana.workflow-integrity-report.v1",
            "status": "verified",
            "event_count": 4,
            "verified_event_count": 4,
            "run_id": run_id,
            "workflow_id": workflow_id,
            "key_id": key_id,
            "release_binding": {
                "schema": "kiana.workflow-release-binding.v1",
                "run_id": run_id,
                "workflow_id": workflow_id,
                "workflow": completed_workflow_binding,
                "git": {
                    "repository": true,
                    "head": head,
                    "index_diff_sha256": git_hash(&index_args),
                    "worktree_diff_sha256": git_hash(&worktree_args),
                    "status_sha256": git_status_hash(),
                }
            },
            "recovery_integrity": {
                "schema": "kiana.swarm-recovery-integrity-report.v1",
                "status": "verified",
                "active_journal_count": 1,
                "verified_active_journal_count": 1,
                "archived_journal_count": 2,
                "verified_archived_journal_count": 2,
                "recoverable_unanchored_tail_count": 0,
                "legacy_unsigned_count": 0,
                "mismatch_count": 0,
                "key_id": key_id,
            }
        });
        fs::write(&proof_path, serde_json::to_vec_pretty(&valid).unwrap()).unwrap();

        let run_report = |path: &std::path::Path, selected_run_id: Option<&str>| {
            let mut command = ProcessCommand::new(&bash);
            command
                .arg("scripts/commercial-release-blockers-report.sh")
                .arg("--json")
                .current_dir(workspace)
                .env("KIANA_WORKFLOW_RECOVERY_INTEGRITY_PROOF_FILE", path)
                .env("KIANA_RELEASE_WORKFLOW_RUNS_DIR", &workflow_runs_dir);
            match selected_run_id {
                Some(value) => {
                    command.env("KIANA_RELEASE_WORKFLOW_RUN_ID", value);
                }
                None => {
                    command.env_remove("KIANA_RELEASE_WORKFLOW_RUN_ID");
                }
            }
            let output = command.output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
        };
        let check = |report: &serde_json::Value| {
            report["checks"]
                .as_array()
                .unwrap()
                .iter()
                .find(|check| check["id"] == "workflow.recovery-integrity")
                .unwrap()
                .clone()
        };

        let missing_selection = run_report(&proof_path, None);
        assert_eq!(check(&missing_selection)["status"], "blocking");
        assert!(check(&missing_selection)["evidence"]
            .as_str()
            .unwrap()
            .contains("KIANA_RELEASE_WORKFLOW_RUN_ID"));

        let accepted = run_report(&proof_path, Some(run_id));
        assert_eq!(check(&accepted)["status"], "satisfied");

        let untracked_path = workspace.join(format!(
            "kiana-release-proof-untracked-{}.tmp",
            std::process::id()
        ));
        fs::write(&untracked_path, b"alpha\n").unwrap();
        let mut untracked = valid.clone();
        untracked["release_binding"]["git"]["status_sha256"] = serde_json::json!(git_status_hash());
        fs::write(&proof_path, serde_json::to_vec_pretty(&untracked).unwrap()).unwrap();
        let untracked_report = run_report(&proof_path, Some(run_id));

        fs::write(&untracked_path, b"bravo\n").unwrap();
        let mutated_untracked_report = run_report(&proof_path, Some(run_id));
        fs::remove_file(&untracked_path).unwrap();
        fs::write(&proof_path, serde_json::to_vec_pretty(&valid).unwrap()).unwrap();

        assert_eq!(check(&untracked_report)["status"], "satisfied");
        assert_eq!(check(&mutated_untracked_report)["status"], "blocking");
        assert!(check(&mutated_untracked_report)["evidence"]
            .as_str()
            .unwrap()
            .contains("Git binding"));

        fs::write(
            workflow_artifact_dir.join("state.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema": "kiana.workflow-state.v1",
                "run_id": run_id,
                "workflow_id": workflow_id,
                "status": "completed",
                "current_node": "learn",
                "last_event_seq": 4,
                "updated_at_ms": 5,
            }))
            .unwrap(),
        )
        .unwrap();
        let stale_workflow_report = run_report(&proof_path, Some(run_id));
        assert_eq!(check(&stale_workflow_report)["status"], "blocking");
        assert!(check(&stale_workflow_report)["evidence"]
            .as_str()
            .unwrap()
            .contains("lifecycle binding"));

        let mut running = valid.clone();
        running["release_binding"]["workflow"] =
            write_workflow_binding("running", "execute", "node_entered");
        fs::write(&proof_path, serde_json::to_vec_pretty(&running).unwrap()).unwrap();
        let running_report = run_report(&proof_path, Some(run_id));
        assert_eq!(check(&running_report)["status"], "blocking");
        assert!(check(&running_report)["evidence"]
            .as_str()
            .unwrap()
            .contains("completed"));

        let restored_workflow_binding =
            write_workflow_binding("completed", "learn", "workflow_completed");
        assert_eq!(
            restored_workflow_binding,
            valid["release_binding"]["workflow"]
        );
        fs::write(&proof_path, serde_json::to_vec_pretty(&valid).unwrap()).unwrap();

        let wrong_run = run_report(&proof_path, Some("run-wrong"));
        assert_eq!(check(&wrong_run)["status"], "blocking");

        let mut stale = valid.clone();
        stale["release_binding"]["git"]["head"] = serde_json::json!("0".repeat(40));
        fs::write(&proof_path, serde_json::to_vec_pretty(&stale).unwrap()).unwrap();
        let stale_report = run_report(&proof_path, Some(run_id));
        assert_eq!(check(&stale_report)["status"], "blocking");
        assert!(check(&stale_report)["evidence"]
            .as_str()
            .unwrap()
            .contains("Git binding"));

        let mut invalid = valid;
        invalid["recovery_integrity"]["recoverable_unanchored_tail_count"] = serde_json::json!(1);
        invalid["secret_hex"] = serde_json::json!("must-not-leak");
        fs::write(&proof_path, serde_json::to_vec_pretty(&invalid).unwrap()).unwrap();
        let rejected = run_report(&proof_path, Some(run_id));
        let rejected_check = check(&rejected);
        assert_eq!(rejected_check["status"], "blocking");
        assert!(rejected_check["evidence"]
            .as_str()
            .unwrap()
            .contains("sensitive field"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn release_evidence_json_reads_dist_dir() {
        let _lock = env_lock().lock().unwrap();
        let root = unique_temp_dir("evidence-json");
        let dist = root.join("dist");
        let proofs = dist.join("proofs");
        fs::create_dir_all(&proofs).unwrap();
        fs::write(
            proofs.join("local-rc-evidence.json"),
            r#"{
  "schema": "kiana.local-rc-evidence.v1",
  "status": "local_rc_ready",
  "dist_dir": "fixture-dist",
  "summary": {
    "release_artifacts": 1,
    "manifests": 2,
    "proofs": 3,
    "blockers_total": 4,
    "local_blockers": 0,
    "external_blockers": 4
  },
  "readiness": {
    "ready": true
  },
  "blockers": {
    "handoff_status": "external_action_required",
    "blocking_by_resolution_scope": {
      "release-owner": 2
    }
  }
}
"#,
        )
        .unwrap();
        let _guard = EnvGuard::set(&[("KIANA_LOCAL_RC_EVIDENCE_OUT", None)]);

        let result = ReleaseCommand
            .execute(CommandContext {
                args: format!("evidence --json --dist-dir={}", dist.display()),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.local-rc-evidence.v1");
        assert_eq!(report["status"], "local_rc_ready");
        assert_eq!(report["readiness"]["ready"], true);
        assert_eq!(report["summary"]["local_blockers"], 0);

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn release_evidence_text_reads_explicit_path() {
        let root = unique_temp_dir("evidence-text");
        let path = root.join("local-rc-evidence.json");
        fs::write(
            &path,
            r#"{
  "schema": "kiana.local-rc-evidence.v1",
  "status": "local_rc_ready",
  "dist_dir": "fixture-dist",
  "summary": {
    "release_artifacts": 1,
    "manifests": 2,
    "proofs": 3,
    "blockers_total": 4,
    "local_blockers": 0,
    "external_blockers": 4
  },
  "readiness": {
    "ready": true
  },
  "blockers": {
    "handoff_status": "external_action_required",
    "blocking_by_resolution_scope": {
      "release-owner": 2
    }
  }
}
"#,
        )
        .unwrap();

        let result = ReleaseCommand
            .execute(CommandContext {
                args: format!("evidence --path={}", path.display()),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("Local RC evidence"));
        assert!(result.value.contains("status: local_rc_ready"));
        assert!(result.value.contains("readiness.ready: true"));
        assert!(result.value.contains("local_blockers: 0"));
        assert!(result.value.contains("resolution_scopes: release-owner=2"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn release_command_reports_live_smoke_token_misuse() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("KIANA_REMOTE_ACCESS_TOKEN", None),
            ("CLAUDE_ACCESS_TOKEN", None),
            ("ANTHROPIC_AUTH_TOKEN", Some("sk-test-api-key")),
        ]);

        let result = ReleaseCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("optional_live_gate_status: invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)"));
        assert!(result.value.contains(
            "optional_live_gate_fix: set KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN to a remote bearer token"
        ));
    }

    #[test]
    fn github_actions_release_smoke_uses_shared_script() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let workflow_path = root
            .join(".github")
            .join("workflows")
            .join("release-smoke.yml");
        let workflow = std::fs::read_to_string(&workflow_path)
            .expect("missing .github/workflows/release-smoke.yml");

        assert!(workflow.contains("name: Release Smoke"));
        assert!(workflow.contains("ubuntu-latest"));
        assert!(workflow.contains("cargo fetch --locked"));
        assert!(workflow.contains("bash scripts/release-smoke.sh"));
    }

    #[test]
    fn install_script_is_source_checkout_installer_with_doctor_verification() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let install_path = root.join("install.sh");
        let install = std::fs::read_to_string(&install_path).expect("missing install.sh");

        assert!(!install.contains("releases/download"));
        assert!(!install.contains("download_binary"));
        assert!(!install.contains(r#"! -d ".git""#));
        assert!(install.contains("KIANA_SKIP_PATH_SETUP"));
        assert!(install.contains("cargo build --release -p kiana-entrypoints --bin kiana"));
        assert!(install.contains(r#""$INSTALL_DIR/kiana${EXE_EXT}" doctor"#));
        assert!(install.contains("kiana config init"));
        assert!(install.contains("kiana login"));
        assert!(!install.contains("运行配置向导"));
    }

    #[test]
    fn release_makefile_and_install_docs_use_explicit_source_build_gate() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let makefile = std::fs::read_to_string(root.join("Makefile")).expect("missing Makefile");
        let install_doc =
            std::fs::read_to_string(root.join("INSTALL.md")).expect("missing INSTALL.md");
        let readme = std::fs::read_to_string(root.join("README.md")).expect("missing README.md");

        assert!(makefile.contains("INSTALL_DIR ?= $(HOME)/.local/bin"));
        assert!(
            makefile.contains("$(CARGO) build --release -p kiana-entrypoints --bin $(BIN_NAME)")
        );
        assert!(makefile.contains("$(INSTALLED_BIN) --version"));
        assert!(makefile.contains("$(INSTALLED_BIN) doctor"));
        assert!(!makefile.contains("cargo build --release --bin kiana"));
        assert!(!install_doc.contains("二进制下载"));
        assert!(!install_doc.contains("配置向导"));
        assert!(install_doc.contains("kiana config init"));
        assert!(install_doc.contains("kiana login"));
        assert!(install_doc.contains("cargo build --release -p kiana-entrypoints --bin kiana"));
        assert!(readme.contains("cargo build --release -p kiana-entrypoints --bin kiana"));
    }

    #[test]
    fn quickstart_and_usage_docs_describe_current_cli_paths() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let quickstart =
            std::fs::read_to_string(root.join("QUICKSTART.md")).expect("missing QUICKSTART.md");
        let usage = std::fs::read_to_string(root.join("USAGE.md")).expect("missing USAGE.md");
        let combined = format!("{quickstart}\n{usage}");

        assert!(quickstart.contains("kiana config init"));
        assert!(quickstart.contains("cargo run -p kiana-entrypoints --bin kiana"));
        assert!(quickstart.contains("kiana -p"));
        assert!(usage.contains("kiana session reply"));
        assert!(usage.contains("kiana doctor"));
        assert!(usage.contains("kiana release"));
        assert!(!combined.contains("cargo run --bin kiana"));
        assert!(!combined.contains("MVP 使用指南"));
        assert!(!combined.contains("Phase 1"));
        assert!(!combined.contains("Phase 2"));
        assert!(!combined.contains("工具调用未启用"));
    }

    #[test]
    fn release_package_generates_distribution_manifest_dry_runs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let package_script =
            std::fs::read_to_string(root.join("scripts").join("package-release.sh"))
                .expect("missing scripts/package-release.sh");
        let manifest_script = std::fs::read_to_string(
            root.join("scripts")
                .join("generate-distribution-manifests.sh"),
        )
        .expect("missing scripts/generate-distribution-manifests.sh");
        let preflight = std::fs::read_to_string(root.join("scripts").join("release-preflight.sh"))
            .expect("missing scripts/release-preflight.sh");
        let workflow =
            std::fs::read_to_string(root.join(".github").join("workflows").join("release.yml"))
                .expect("missing .github/workflows/release.yml");

        assert!(package_script.contains("bash scripts/generate-distribution-manifests.sh"));
        assert!(package_script
            .contains("cargo build --release --locked --offline -p kiana-entrypoints --bin kiana"));
        assert!(package_script.contains("rustc -vV"));
        assert!(package_script.contains("does not match host package target"));
        assert!(package_script.contains("verify_binary_format"));
        assert!(preflight.contains("scripts/generate-distribution-manifests.sh"));
        assert!(preflight.contains("kiana-workflow-release-binding.v1.schema.json"));
        assert!(preflight.contains("scripts/sign-release-artifacts.sh"));
        assert!(preflight.contains("scripts/entitlement-proof-report.sh"));
        assert!(preflight.contains("scripts/product-acceptance-report.sh"));
        assert!(preflight.contains("scripts/release-ops-report.sh"));
        assert!(workflow.contains("cargo fetch --locked"));
        assert!(workflow.contains("bash scripts/sign-release-artifacts.sh"));
        assert!(workflow.contains("KIANA_SIGNING_COMMAND"));
        assert!(workflow.contains("dist/proofs/**"));
        assert!(workflow.contains("KIANA_LIVE_SMOKE_DIR: dist/proofs/live-smoke"));
        assert!(workflow.contains(
            "KIANA_ENTITLEMENT_PROOF_OUT: dist/proofs/entitlement/entitlement-proof.json"
        ));
        assert!(workflow
            .contains("KIANA_PRODUCT_ACCEPTANCE_OUT: dist/proofs/product/product-acceptance.json"));
        assert!(
            workflow.contains("KIANA_RELEASE_OPS_OUT: dist/proofs/release-ops/release-ops.json")
        );
        assert!(workflow.contains("dist/manifests/**"));
        assert!(manifest_script.contains("offline-manifest.json"));
        assert!(manifest_script.contains("kiana.enterprise.offline-manifest.v1"));
        assert!(manifest_script.contains("Homebrew Manifest Blocked"));
        assert!(manifest_script.contains("winget Manifest Blocked"));
        assert!(manifest_script.contains("InstallerType: zip"));
        assert!(manifest_script.contains("NestedInstallerType: portable"));
        assert!(manifest_script.contains("KIANA_RELEASE_BASE_URL"));
    }

    #[test]
    fn package_lifecycle_smoke_checks_every_packaged_schema() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let lifecycle_smoke =
            std::fs::read_to_string(root.join("scripts").join("package-lifecycle-smoke.sh"))
                .expect("missing scripts/package-lifecycle-smoke.sh");

        assert!(lifecycle_smoke.contains("for schema in docs/schemas/*.json"));
        assert!(lifecycle_smoke.contains("package schema file missing"));
        assert!(lifecycle_smoke.contains("package schema file differs from source"));
        assert!(lifecycle_smoke.contains("archive target does not match host package target"));
        assert!(lifecycle_smoke.contains("verify_binary_format"));
    }

    #[test]
    fn release_smoke_script_exercises_install_path_with_temp_dir() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let smoke = std::fs::read_to_string(root.join("scripts").join("release-smoke.sh"))
            .expect("missing scripts/release-smoke.sh");

        assert!(smoke.contains("mktemp -d"));
        assert!(smoke.contains("cargo test --workspace --locked --offline --no-fail-fast"));
        assert!(smoke
            .contains("cargo build --release --locked --offline -p kiana-entrypoints --bin kiana"));
        assert!(smoke.contains("real_cargo_home="));
        assert!(smoke.contains("real_rustup_home="));
        assert!(smoke.contains("smoke_home="));
        assert!(smoke.contains("run_clean_kiana"));
        assert!(smoke.contains(r#"return "$status""#));
        assert!(!smoke.contains(r#"*) return 0 ;;"#));
        assert!(smoke.contains("CARGO_HOME=\"$real_cargo_home\""));
        assert!(smoke.contains("RUSTUP_HOME=\"$real_rustup_home\""));
        assert!(smoke.contains("KIANA_CONFIG_FILE=\"$smoke_home/.kiana/config.toml\""));
        assert!(smoke.contains("KIANA_HOME=\"$smoke_home/.kiana\""));
        assert!(smoke.contains("INSTALL_DIR=\"$install_dir\""));
        assert!(smoke.contains("KIANA_SKIP_PATH_SETUP=1"));
        assert!(smoke.contains("make install"));
        assert!(smoke.contains("bash install.sh"));
        assert!(smoke.contains(r#"installed_bin="$install_dir/kiana$(exe_ext)""#));
        assert!(smoke.contains(r#"smoke_version "$installed_bin""#));
        assert!(smoke.contains(r#"smoke_doctor "$installed_bin""#));
        assert!(smoke.contains("smoke_mcp_config"));
        assert!(smoke.contains("KIANA_MCP_SERVERS_JSON"));
        assert!(smoke.contains("mcp get docs"));
        assert!(smoke.contains("smoke_mcp_project_config"));
        assert!(smoke.contains("mcp add-json docs"));
        assert!(smoke.contains("mcp add -e 'API_KEY=abc def' docs -- node server.js --watch"));
        assert!(smoke.contains(r#""API_KEY": "abc def""#));
        assert!(smoke.contains("packages/app"));
        assert!(smoke.contains("root-server.js"));
        assert!(smoke.contains("${KIANA_TEST_MCP_COMMAND}"));
        assert!(smoke.contains("${KIANA_TEST_MCP_ARG:-fallback.js}"));
        assert!(smoke.contains("node fallback.js"));
        assert!(smoke.contains("mcp add --transport http docs https://example.test/mcp"));
        assert!(smoke.contains("Authorization: Bearer abc"));
        assert!(smoke.contains("Added HTTP MCP server docs"));
        assert!(smoke.contains("--scope project"));
        assert!(smoke.contains("mcp remove docs"));
        assert!(smoke.contains("-s project"));
        assert!(smoke.contains("mcp add-from-claude-desktop --scope project"));
        assert!(smoke.contains("KIANA_CLAUDE_DESKTOP_CONFIG"));
        assert!(smoke.contains("desktop-server.js"));
        assert!(smoke.contains("mcp reset-project-choices"));
        assert!(smoke.contains("mcp-project-choices.json"));
        assert!(smoke.contains("enabledMcpjsonServers"));
        assert!(smoke.contains(".mcp.json"));
        assert!(smoke.contains("smoke_auth_config"));
        assert!(smoke.contains("auth login sk-ant-smoke-auth-key"));
        assert!(smoke.contains("auth status --json"));
        assert!(smoke.contains("auth logout"));
        assert!(smoke.contains("__fish_seen_subcommand_from auto-mode"));
        assert!(smoke.contains("'auto-mode:kiana command'"));
        assert!(smoke.contains("smoke_completion_scripts"));
        assert!(smoke.contains("completion bash"));
        assert!(smoke.contains("completion fish"));
        assert!(smoke.contains("completion zsh --output"));
        assert!(smoke.contains("smoke_plugin_marketplace"));
        assert!(smoke.contains("plugin marketplace add"));
        assert!(smoke.contains("plugin marketplace list --json"));
        assert!(smoke.contains("plugin install review-tools@tools-marketplace"));
        assert!(smoke.contains("plugin list review-tools"));
        assert!(smoke.contains("plugin disable review-tools"));
        assert!(smoke.contains("plugin uninstall review-tools"));
        assert!(smoke.contains("plugin marketplace remove"));
        assert!(smoke.contains("smoke_help_usage"));
        assert!(smoke.contains("--help::kiana mcp serve"));
        assert!(smoke.contains("auto-mode --help::Usage: kiana auto-mode"));
        assert!(smoke.contains("auth --help::Usage: kiana auth"));
        assert!(smoke.contains("auth status --help::Usage: kiana auth status"));
        assert!(smoke.contains("completion --help::Usage: kiana completion <shell>"));
        assert!(smoke.contains("plugin --help::--scope user|project|local"));
        assert!(smoke.contains("plugin install --help::Usage: kiana plugin install"));
        assert!(smoke.contains("agents --help::Usage: kiana agents"));
        assert!(smoke.contains("open --help::Usage: kiana open <cc-url>"));
        assert!(smoke.contains("server --help::Usage: kiana server"));
        assert!(smoke.contains("plugin marketplace --help::Usage: kiana plugin marketplace"));
        assert!(smoke.contains("mcp --help::kiana mcp serve [--debug] [--verbose]"));
        assert!(smoke.contains("mcp serve --help::Usage: kiana mcp serve"));
        assert!(smoke.contains(
            "mcp add-from-claude-desktop --help::Usage: kiana mcp add-from-claude-desktop"
        ));
        assert!(smoke.contains("mcp-server-http --help::Usage: kiana mcp-server"));
        assert!(smoke.contains("computer-mcp --help::Usage: kiana computer-mcp"));
        assert!(smoke.contains("bridge start --help::Usage: kiana bridge"));
        assert!(smoke.contains("chrome-native-host --help::Usage: kiana chrome"));
        assert!(smoke.contains("--print --help::Usage: kiana -p"));
        assert!(smoke.contains("--output-format json --print --help::Usage: kiana -p"));
        assert!(smoke.contains("--print --record-only --help::Usage: kiana -p"));
        assert!(smoke.contains("--continue --help::Usage: kiana --continue"));
        assert!(smoke.contains("--continue --record-only --help::Usage: kiana --continue"));
        assert!(smoke.contains("--resume abc --output-format json --help::Usage: kiana --continue"));
        assert!(smoke.contains("url handle --help::Usage: kiana url"));
        assert!(smoke.contains(
            "remote-session code-session create --help::Usage: kiana remote-session code-session"
        ));
        assert!(smoke.contains("daemon enqueue --help::Usage: kiana daemon"));
        assert!(smoke.contains("session show --help::Usage: kiana session"));
    }
}
