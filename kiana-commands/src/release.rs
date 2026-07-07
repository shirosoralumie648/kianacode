use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Context};
use async_trait::async_trait;
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
    "Usage: kiana release [blockers]\n       kiana release blockers [--json] [--dist-dir <dir>]"
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
        .unwrap_or_else(|_| "./scripts/commercial-release-blockers-report.sh".to_string())
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
    use super::ReleaseCommand;
    use crate::{Command, CommandContext};
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command as ProcessCommand;
    use std::sync::{Mutex, OnceLock};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
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
        assert!(preflight.contains("scripts/generate-distribution-manifests.sh"));
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
