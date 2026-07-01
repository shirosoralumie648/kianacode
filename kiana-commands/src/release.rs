use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use std::path::{Path, PathBuf};

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
        match context.args.trim() {
            "" => {}
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow!(usage())),
        }

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
        lines
            .push("optional_live_gate: kiana remote-session code-session smoke --json".to_string());
        lines.push(format!("optional_live_gate_status: {}", live_gate.label()));
        if let Some(fix) = live_gate.fix() {
            lines.push(format!("optional_live_gate_fix: {fix}"));
        }
        lines.push("usage: bash scripts/release-smoke.sh".to_string());
        Ok(CommandResult::text(lines.join("\n")))
    }
}

fn usage() -> &'static str {
    "Usage: kiana release"
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
    use std::sync::{Mutex, OnceLock};

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
        assert!(preflight.contains("scripts/product-acceptance-report.sh"));
        assert!(workflow.contains("cargo fetch --locked"));
        assert!(workflow.contains("bash scripts/sign-release-artifacts.sh"));
        assert!(workflow.contains("KIANA_SIGNING_COMMAND"));
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
        assert!(smoke.contains("plugin --help::usage: kiana plugin"));
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
