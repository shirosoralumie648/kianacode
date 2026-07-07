use crate::local_state::{bool_label, config_path, sdk_sessions_dir};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use async_trait::async_trait;
use kiana_tools::bash_sandbox::{bash_sandbox_diagnostic, BashSandboxStatus};
use kiana_tools::create_default_registry;
use kiana_tools::permissions::{effective_tool_permissions, EffectiveToolPermissions};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Command as ProcessCommand;

pub struct DoctorCommand;

const DOCTOR_SCHEMA: &str = "kiana.doctor.v1";

#[async_trait]
impl Command for DoctorCommand {
    fn name(&self) -> &str {
        "doctor"
    }

    fn description(&self) -> &str {
        "Run diagnostics"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let json = match context.args.trim() {
            "" => false,
            "--json" => true,
            "help" | "--help" | "-h" => return Ok(CommandResult::text(usage())),
            _ => return Err(anyhow::anyhow!(usage())),
        };

        let report = build_doctor_report(&context)?;
        if json {
            Ok(CommandResult::text(serde_json::to_string_pretty(&report)?))
        } else {
            Ok(CommandResult::text(render_doctor_text(&report)))
        }
    }
}

fn usage() -> &'static str {
    "Usage: kiana doctor [--json]"
}

#[derive(Debug, Serialize)]
struct DoctorReport {
    schema: &'static str,
    status: String,
    cwd: String,
    cargo: ProbeReport,
    git_root: ProbeReport,
    config_file: PathReport,
    sdk_sessions_dir: PathReport,
    api_key_set: bool,
    model: String,
    remote_settings: RemoteSettingsReport,
    tui_permission_request: TuiPermissionRequestReport,
    mcp_transport: McpTransportReport,
    modifiers: ModifiersReport,
    remote_bridge: RemoteBridgeReport,
    remote_code_session: RemoteCodeSessionReport,
    oauth_token_file: OAuthTokenFileReport,
    bash_sandbox: BashSandboxReport,
    commercial_security: CommercialSecurityReport,
    tool_parity: ToolParityReport,
    reference_capabilities: Vec<ReferenceCapabilityReport>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ProbeReport {
    available: bool,
    value: String,
}

#[derive(Debug, Serialize)]
struct PathReport {
    path: String,
    found: bool,
}

#[derive(Debug, Serialize)]
struct RemoteSettingsReport {
    status: String,
    file: String,
}

#[derive(Debug, Serialize)]
struct TuiPermissionRequestReport {
    active: bool,
    queued: usize,
}

#[derive(Debug, Serialize)]
struct McpTransportReport {
    wired: bool,
    transports: Vec<&'static str>,
    surfaces: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct ModifiersReport {
    platform: String,
    backend: String,
    available: bool,
    current: Vec<String>,
}

#[derive(Debug, Serialize)]
struct RemoteBridgeReport {
    start_command_wired: bool,
    token_configured: bool,
}

#[derive(Debug, Serialize)]
struct RemoteCodeSessionReport {
    live_smoke_token: String,
    configured: bool,
    source: Option<&'static str>,
}

#[derive(Debug, Serialize)]
struct OAuthTokenFileReport {
    status: String,
    valid: bool,
    refreshable: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct BashSandboxReport {
    enabled: bool,
    status: String,
    runtime: String,
    fail_if_unavailable: bool,
    allow_unsandboxed_commands: bool,
    bwrap: String,
}

#[derive(Debug, Serialize)]
struct CommercialSecurityReport {
    ready: bool,
    status: String,
    platform: String,
    isolation: String,
    controls: Vec<String>,
    issues: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ToolParityReport {
    schema: &'static str,
    source: &'static str,
    plugin_tools_included: bool,
    total_tools: usize,
    read_only_tools: usize,
    concurrency_safe_tools: usize,
    workbenches: Vec<ToolWorkbenchReport>,
    built_in_tools: Vec<ToolParityEntry>,
}

#[derive(Debug, Serialize)]
struct ToolWorkbenchReport {
    name: String,
    tools: usize,
}

#[derive(Debug, Serialize)]
struct ToolParityEntry {
    name: String,
    source: &'static str,
    read_only: bool,
    concurrency_safe: bool,
    workbench: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReferenceCapabilityReport {
    id: &'static str,
    domain: &'static str,
    status: &'static str,
    references: Vec<&'static str>,
    surfaces: Vec<&'static str>,
    evidence: Vec<&'static str>,
    risks: Vec<String>,
}

fn build_doctor_report(context: &CommandContext) -> anyhow::Result<DoctorReport> {
    let config = kiana_bootstrap::config::load_config();
    let config_path = config_path();
    let sdk_dir = sdk_sessions_dir();
    let cwd = std::env::current_dir()?;
    let cargo_status = command_first_line("cargo", &["--version"]);
    let git_status = command_first_line("git", &["rev-parse", "--show-toplevel"]);
    let modifier_status = kiana_modifiers::get_modifier_status();
    let code_session_token_status = code_session_live_smoke_token_status();
    let oauth_token_file_status = oauth_token_file_status();
    let sandbox_state = sandbox_diagnostic_app_state(&context.app_state, config.sandbox.as_ref());
    let bash_sandbox = bash_sandbox_diagnostic(&sandbox_state);
    let tool_permissions = effective_tool_permissions(&context.app_state);
    let commercial_security = commercial_security_report(&tool_permissions, &bash_sandbox);
    let api_key_set = config
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty());
    let remote_bridge_token_configured = bridge_access_token_configured();

    let mut warnings = Vec::new();
    if !api_key_set {
        warnings.push(
            "set ANTHROPIC_API_KEY or ~/.kiana/config.toml before sending model prompts"
                .to_string(),
        );
    }
    if !remote_bridge_token_configured {
        warnings.push(
            "remote bridge start requires KIANA_BRIDGE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN"
                .to_string(),
        );
    }
    if let Some(warning) = code_session_token_status.warning() {
        warnings.push(warning.trim_start_matches("warning: ").to_string());
    }
    if let Some(warning) = oauth_token_file_status.warning() {
        warnings.push(warning.trim_start_matches("warning: ").to_string());
    }
    if !modifier_status.available {
        let reason = modifier_status
            .message
            .clone()
            .unwrap_or_else(|| "modifier polling is unavailable".to_string());
        warnings.push(format!("modifier key polling unavailable: {reason}"));
    }
    if bash_sandbox.status == BashSandboxStatus::Unavailable && bash_sandbox.fail_if_unavailable {
        warnings.push(
            "bash sandbox enabled with failIfUnavailable=true, but bubblewrap (bwrap) is not available"
                .to_string(),
        );
    } else if bash_sandbox.status == BashSandboxStatus::Unavailable
        && !bash_sandbox.allow_unsandboxed_commands
    {
        warnings.push(
            "bash sandbox enabled but unavailable and allowUnsandboxedCommands=false; Bash commands will fail"
                .to_string(),
        );
    } else if bash_sandbox.status == BashSandboxStatus::Unavailable {
        warnings.push(
            "bash sandbox enabled but bubblewrap (bwrap) is not available; Bash may run unsandboxed if allowed"
                .to_string(),
        );
    }

    let status = if commercial_security.ready && warnings.is_empty() {
        "ready".to_string()
    } else {
        "warning".to_string()
    };

    Ok(DoctorReport {
        schema: DOCTOR_SCHEMA,
        status,
        cwd: cwd.display().to_string(),
        cargo: ProbeReport {
            available: cargo_status.is_some(),
            value: cargo_status.unwrap_or_else(|| "missing".to_string()),
        },
        git_root: ProbeReport {
            available: git_status.is_some(),
            value: git_status.unwrap_or_else(|| "not a git repository".to_string()),
        },
        config_file: PathReport {
            path: config_path.display().to_string(),
            found: config_path.is_file(),
        },
        sdk_sessions_dir: PathReport {
            path: sdk_dir.display().to_string(),
            found: sdk_dir.is_dir(),
        },
        api_key_set,
        model: config.model,
        remote_settings: RemoteSettingsReport {
            status: remote_settings_status(),
            file: remote_settings_file_label(),
        },
        tui_permission_request: TuiPermissionRequestReport {
            active: app_state_bool(&context.app_state, "tui_permission_request_active"),
            queued: app_state_usize(&context.app_state, "tui_permission_request_queue_len"),
        },
        mcp_transport: McpTransportReport {
            wired: true,
            transports: vec!["stdio", "http", "sse", "ws"],
            surfaces: vec!["tools", "resources", "resource_templates", "prompts"],
        },
        modifiers: ModifiersReport {
            platform: modifier_status.platform.to_string(),
            backend: modifier_status.backend.to_string(),
            available: modifier_status.available,
            current: modifier_status.modifiers,
        },
        remote_bridge: RemoteBridgeReport {
            start_command_wired: true,
            token_configured: remote_bridge_token_configured,
        },
        remote_code_session: RemoteCodeSessionReport {
            live_smoke_token: code_session_token_status.label(),
            configured: matches!(
                code_session_token_status,
                CodeSessionLiveSmokeTokenStatus::Configured(_)
            ),
            source: code_session_token_status.source(),
        },
        oauth_token_file: OAuthTokenFileReport {
            status: oauth_token_file_status.label(),
            valid: !matches!(oauth_token_file_status, OAuthTokenFileStatus::Invalid(_)),
            refreshable: matches!(oauth_token_file_status, OAuthTokenFileStatus::Refreshable),
            error: oauth_token_file_status.error(),
        },
        bash_sandbox: BashSandboxReport {
            enabled: bash_sandbox.enabled,
            status: bash_sandbox.status.as_str().to_string(),
            runtime: bash_sandbox.runtime_label().to_string(),
            fail_if_unavailable: bash_sandbox.fail_if_unavailable,
            allow_unsandboxed_commands: bash_sandbox.allow_unsandboxed_commands,
            bwrap: bash_sandbox.bwrap_label(),
        },
        tool_parity: tool_parity_report(),
        reference_capabilities: reference_capability_matrix(
            &commercial_security,
            remote_bridge_token_configured,
            code_session_token_status,
        ),
        commercial_security,
        warnings,
    })
}

fn render_doctor_text(report: &DoctorReport) -> String {
    let modifier_keys = if report.modifiers.current.is_empty() {
        "none".to_string()
    } else {
        report.modifiers.current.join(",")
    };
    let mut lines = vec![
        "Doctor".to_string(),
        format!("cwd: {}", report.cwd),
        format!("cargo: {}", report.cargo.value),
        format!("git_root: {}", report.git_root.value),
        format!(
            "config_file: {} ({})",
            report.config_file.path,
            if report.config_file.found {
                "found"
            } else {
                "missing"
            }
        ),
        format!(
            "sdk_sessions_dir: {} ({})",
            report.sdk_sessions_dir.path,
            if report.sdk_sessions_dir.found {
                "found"
            } else {
                "missing"
            }
        ),
        format!("api_key_set: {}", bool_label(report.api_key_set)),
        format!("model: {}", report.model),
        format!(
            "remote_settings: status={} file={}",
            report.remote_settings.status, report.remote_settings.file
        ),
        format!(
            "tui_permission_request: active={} queued={}",
            bool_label(report.tui_permission_request.active),
            report.tui_permission_request.queued
        ),
        "mcp_transport: stdio,http,sse,ws wired; surfaces: tools,resources,resource_templates,prompts"
            .to_string(),
        format!(
            "modifiers: platform={} backend={} available={} current={}",
            report.modifiers.platform,
            report.modifiers.backend,
            bool_label(report.modifiers.available),
            modifier_keys
        ),
        format!(
            "remote_bridge: start command wired with SDK runner; token_configured: {}",
            bool_label(report.remote_bridge.token_configured)
        ),
        format!(
            "remote_code_session: live_smoke_token={}",
            report.remote_code_session.live_smoke_token
        ),
        format!("oauth_token_file: {}", report.oauth_token_file.status),
        format!(
            "bash_sandbox: enabled={} status={} runtime={} fail_if_unavailable={} allow_unsandboxed_commands={} bwrap={}",
            bool_label(report.bash_sandbox.enabled),
            report.bash_sandbox.status,
            report.bash_sandbox.runtime,
            bool_label(report.bash_sandbox.fail_if_unavailable),
            bool_label(report.bash_sandbox.allow_unsandboxed_commands),
            report.bash_sandbox.bwrap
        ),
        format!(
            "commercial_security: {} platform={} isolation={}",
            if report.commercial_security.ready {
                "ready".to_string()
            } else {
                format!(
                    "not_ready({} issue(s))",
                    report.commercial_security.issues.len()
                )
            },
            report.commercial_security.platform,
            report.commercial_security.isolation
        ),
        format!(
            "tool_parity: {} built-in tools read_only={} concurrency_safe={} plugin_tools_included={}",
            report.tool_parity.total_tools,
            report.tool_parity.read_only_tools,
            report.tool_parity.concurrency_safe_tools,
            bool_label(report.tool_parity.plugin_tools_included)
        ),
    ];

    for capability in &report.reference_capabilities {
        lines.push(format!(
            "capability: {} status={} surfaces={} evidence={}",
            capability.id,
            capability.status,
            capability.surfaces.join(","),
            capability.evidence.join(",")
        ));
        for risk in &capability.risks {
            lines.push(format!("capability_risk: {}: {risk}", capability.id));
        }
    }

    for warning in &report.warnings {
        lines.push(format!("warning: {warning}"));
    }
    for issue in &report.commercial_security.issues {
        lines.push(format!("commercial_security_issue: {issue}"));
    }

    lines.join("\n")
}

fn tool_parity_report() -> ToolParityReport {
    let registry = create_default_registry();
    let mut built_in_tools: Vec<ToolParityEntry> = registry
        .list_tools()
        .into_iter()
        .map(|tool| ToolParityEntry {
            name: tool.name().to_string(),
            source: "builtin",
            read_only: tool.is_read_only(),
            concurrency_safe: tool.is_concurrency_safe(),
            workbench: tool.workbench().map(str::to_string),
        })
        .collect();
    built_in_tools.sort_by(|left, right| left.name.cmp(&right.name));

    let mut workbench_counts = HashMap::<String, usize>::new();
    for tool in &built_in_tools {
        if let Some(workbench) = &tool.workbench {
            *workbench_counts.entry(workbench.clone()).or_default() += 1;
        }
    }
    let mut workbenches: Vec<ToolWorkbenchReport> = workbench_counts
        .into_iter()
        .map(|(name, tools)| ToolWorkbenchReport { name, tools })
        .collect();
    workbenches.sort_by(|left, right| left.name.cmp(&right.name));

    ToolParityReport {
        schema: "kiana.tool-parity.v1",
        source: "builtin-registry",
        plugin_tools_included: false,
        total_tools: built_in_tools.len(),
        read_only_tools: built_in_tools.iter().filter(|tool| tool.read_only).count(),
        concurrency_safe_tools: built_in_tools
            .iter()
            .filter(|tool| tool.concurrency_safe)
            .count(),
        workbenches,
        built_in_tools,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodeSessionLiveSmokeTokenStatus {
    Configured(&'static str),
    Missing,
    AnthropicApiKeyMisuse,
}

impl CodeSessionLiveSmokeTokenStatus {
    fn label(self) -> String {
        match self {
            CodeSessionLiveSmokeTokenStatus::Configured(source) => {
                format!("yes ({source})")
            }
            CodeSessionLiveSmokeTokenStatus::Missing => "no".to_string(),
            CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse => {
                "invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)".to_string()
            }
        }
    }

    fn warning(self) -> Option<String> {
        match self {
            CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse => Some(
                "warning: live smoke requires KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN; ANTHROPIC_AUTH_TOKEN=sk-* is an API key, not a remote access token"
                    .to_string(),
            ),
            CodeSessionLiveSmokeTokenStatus::Missing => Some(
                "warning: live smoke requires KIANA_REMOTE_ACCESS_TOKEN, CLAUDE_ACCESS_TOKEN, or an OAuth token file"
                    .to_string(),
            ),
            CodeSessionLiveSmokeTokenStatus::Configured(_) => None,
        }
    }

    fn source(self) -> Option<&'static str> {
        match self {
            CodeSessionLiveSmokeTokenStatus::Configured(source) => Some(source),
            CodeSessionLiveSmokeTokenStatus::Missing
            | CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OAuthTokenFileStatus {
    Missing,
    AccessOnly,
    Refreshable,
    Invalid(String),
}

impl OAuthTokenFileStatus {
    fn label(&self) -> String {
        match self {
            OAuthTokenFileStatus::Missing => "missing".to_string(),
            OAuthTokenFileStatus::AccessOnly => "access_only".to_string(),
            OAuthTokenFileStatus::Refreshable => "refreshable".to_string(),
            OAuthTokenFileStatus::Invalid(_) => "invalid".to_string(),
        }
    }

    fn warning(&self) -> Option<String> {
        match self {
            OAuthTokenFileStatus::Invalid(error) => {
                Some(format!("warning: OAuth token file is invalid: {error}"))
            }
            _ => None,
        }
    }

    fn error(&self) -> Option<String> {
        match self {
            OAuthTokenFileStatus::Invalid(error) => Some(error.clone()),
            _ => None,
        }
    }
}

fn app_state_bool(app_state: &HashMap<String, Value>, key: &str) -> bool {
    app_state.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn app_state_usize(app_state: &HashMap<String, Value>, key: &str) -> usize {
    app_state
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0)
}

fn sandbox_diagnostic_app_state(
    app_state: &HashMap<String, Value>,
    config_sandbox: Option<&Value>,
) -> HashMap<String, Value> {
    let mut state = app_state.clone();
    if !state.contains_key("sandbox") {
        if let Some(sandbox) = config_sandbox {
            state.insert("sandbox".to_string(), sandbox.clone());
        }
    }
    state
}

fn commercial_security_report(
    permissions: &EffectiveToolPermissions,
    bash_sandbox: &kiana_tools::bash_sandbox::BashSandboxDiagnostic,
) -> CommercialSecurityReport {
    commercial_security_report_for_values(
        std::env::consts::OS,
        &permissions.profile,
        &permissions.mode,
        bash_sandbox,
    )
}

fn commercial_security_report_for_values(
    platform: &str,
    permission_profile: &str,
    permission_mode: &str,
    bash_sandbox: &kiana_tools::bash_sandbox::BashSandboxDiagnostic,
) -> CommercialSecurityReport {
    let isolation = commercial_security_isolation(platform);
    let controls = commercial_security_controls(isolation);
    let mut issues = Vec::new();
    if permission_profile != "commercial" {
        issues.push("set `kiana permissions profile commercial`".to_string());
    }
    if permission_mode != "ask" {
        issues.push("commercial profile must resolve to ask permission mode".to_string());
    }

    match isolation {
        "linux_bwrap" => {
            if !bash_sandbox.enabled {
                issues.push("enable bash sandbox in config sandbox.enabled=true".to_string());
            }
            if bash_sandbox.status != BashSandboxStatus::Ready {
                issues.push("install/configure bubblewrap (bwrap) for bash sandbox".to_string());
            }
            if !bash_sandbox.fail_if_unavailable {
                issues.push("set sandbox.failIfUnavailable=true".to_string());
            }
            if bash_sandbox.allow_unsandboxed_commands {
                issues.push("set sandbox.allowUnsandboxedCommands=false".to_string());
            }
        }
        "windows_exec_policy" | "macos_exec_policy" => {}
        _ => issues.push(format!(
            "commercial security platform isolation is not defined for {platform}"
        )),
    }

    let ready = issues.is_empty();
    CommercialSecurityReport {
        ready,
        status: if ready {
            "ready".to_string()
        } else {
            "not_ready".to_string()
        },
        platform: platform.to_string(),
        isolation: isolation.to_string(),
        controls,
        issues,
    }
}

fn commercial_security_isolation(platform: &str) -> &'static str {
    match platform {
        "linux" => "linux_bwrap",
        "windows" => "windows_exec_policy",
        "macos" => "macos_exec_policy",
        _ => "unsupported_platform",
    }
}

fn commercial_security_controls(isolation: &str) -> Vec<String> {
    let mut controls = vec![
        "permission_profile:commercial".to_string(),
        "permission_mode:ask".to_string(),
        "exec_policy:bash+powershell".to_string(),
        "exec_policy:destructive-root-sync-deny".to_string(),
    ];
    match isolation {
        "linux_bwrap" => {
            controls.push("bash_sandbox:enabled".to_string());
            controls.push("bash_sandbox:fail_if_unavailable".to_string());
            controls.push("bash_sandbox:deny_unsandboxed_fallback".to_string());
        }
        "windows_exec_policy" => {
            controls.push("platform_shell:windows-powershell-policy".to_string());
        }
        "macos_exec_policy" => {
            controls.push("platform_shell:macos-exec-policy".to_string());
        }
        _ => {
            controls.push("platform_shell:unsupported".to_string());
        }
    }
    controls
}

fn reference_capability_matrix(
    commercial_security: &CommercialSecurityReport,
    remote_bridge_token_configured: bool,
    code_session_token_status: CodeSessionLiveSmokeTokenStatus,
) -> Vec<ReferenceCapabilityReport> {
    let remote_status = if remote_bridge_token_configured
        && matches!(
            code_session_token_status,
            CodeSessionLiveSmokeTokenStatus::Configured(_)
        ) {
        "local_ready_external_required"
    } else {
        "external_required"
    };
    let security_risks = if commercial_security.ready {
        vec![
            "accepted platform-security proof from real release runners is still required"
                .to_string(),
        ]
    } else {
        commercial_security.issues.clone()
    };

    vec![
        ReferenceCapabilityReport {
            id: "runtime-session-core",
            domain: "runtime/session",
            status: "ready",
            references: vec!["codex", "cline", "pi", "claude-code-rev-main"],
            surfaces: vec!["sdk-session-tree", "stream-json", "tui", "remote", "bridge"],
            evidence: vec![
                "kiana-runtime-event.v1",
                "jsonl-session-tree",
                "session-import-export",
                "release-smoke",
            ],
            risks: vec!["future public RPC surfaces need matching schema fixtures".to_string()],
        },
        ReferenceCapabilityReport {
            id: "tool-lifecycle-mcp",
            domain: "tools/mcp",
            status: "ready",
            references: vec!["codex", "Roo-Code", "claude-code-rev-main"],
            surfaces: vec![
                "tool-registry",
                "runtime-events",
                "mcp-prompts",
                "mcp-stdio",
                "mcp-http",
                "mcp-sse",
                "mcp-ws",
            ],
            evidence: vec![
                "read-only-tool-batching",
                "mcp-resource-templates",
                "mcp-prompt-list-get",
                "mcp-prompt-lifecycle",
                "mcp-error-lifecycle",
                "stream-json-tool-result",
            ],
            risks: vec!["new public tool surfaces must keep lifecycle/error assertions in sync".to_string()],
        },
        ReferenceCapabilityReport {
            id: "security-policy",
            domain: "permissions/trust/sandbox",
            status: if commercial_security.ready { "ready" } else { "not_ready" },
            references: vec!["codex", "continue", "OpenHands", "pi"],
            surfaces: vec!["permission-profile", "project-trust", "exec-policy", "network-policy", "commercial-security"],
            evidence: vec![
                "permission-precedence-tests",
                "trust-gates",
                "bash-powershell-exec-policy",
                "network-ssrf-policy",
            ],
            risks: security_risks,
        },
        ReferenceCapabilityReport {
            id: "provider-registry",
            domain: "provider/model/auth",
            status: "local_ready_external_required",
            references: vec!["cline", "continue", "pi", "langchain"],
            surfaces: vec!["model-list", "model-catalog", "model-smoke", "auth-status", "provider-standard"],
            evidence: vec![
                "fake-provider-standard-tests",
                "openai-compatible-tool-loop",
                "ollama-tool-loop",
                "offline-model-catalog",
            ],
            risks: vec!["production-like provider live catalog and smoke proof are external release blockers".to_string()],
        },
        ReferenceCapabilityReport {
            id: "local-coding-workflow",
            domain: "repo/edit/review",
            status: "ready",
            references: vec!["aider", "continue", "Roo-Code"],
            surfaces: vec!["repo-map", "file-sets", "checkpoint", "diff", "review", "checks", "repair-loop"],
            evidence: vec![
                "deterministic-repo-map-budget",
                "editable-read-only-file-sets",
                "write-edit-delete-file-changes",
                "checkpoint-create-restore-undo",
                "last-assistant-diff",
                "isolated-review",
                "isolated-checks",
                "repair-checks-nonstreaming",
                "repair-checks-streaming",
                "late-user-edit-conflict",
            ],
            risks: Vec::new(),
        },
        ReferenceCapabilityReport {
            id: "product-shell",
            domain: "tui/app-server",
            status: "in_progress",
            references: vec!["codex", "cline", "OpenHands", "Roo-Code", "pi"],
            surfaces: vec!["tui", "settings-readiness", "prompt-history", "app-server"],
            evidence: vec![
                "product-shell-smoke",
                "headless-tui-render-tests",
                "direct-connect-app-contract",
                "app-server-events-view",
                "app-server-release-proof-surfaces",
            ],
            risks: vec!["target-customer walkthrough and acceptance proof remain external release blockers".to_string()],
        },
        ReferenceCapabilityReport {
            id: "plugin-extension-contracts",
            domain: "plugins/skills/hooks",
            status: "ready",
            references: vec!["claude-code-main", "cline", "pi", "continue"],
            surfaces: vec![
                "plugin-marketplace",
                "plugin-install-receipt",
                "plugin-component-validate",
                "skills",
                "hooks",
                "agents",
                "mcp",
                "lsp",
            ],
            evidence: vec![
                "managed-plugin-policy",
                "scoped-plugin-roots",
                "plugin-install-receipt-integrity",
                "plugin-component-json-preflight",
                "plugin-enable-disable-visibility",
                "hook-trust-boundaries",
            ],
            risks: vec![
                "external cryptographic plugin provenance remains an enterprise deployment policy item"
                    .to_string(),
            ],
        },
        ReferenceCapabilityReport {
            id: "remote-commercial-release",
            domain: "remote/release",
            status: remote_status,
            references: vec!["codex", "OpenHands", "cline"],
            surfaces: vec!["remote-bridge", "code-session-smoke", "proof-manifest", "artifact-verifier"],
            evidence: vec![
                "commercial-release-blockers-report",
                "stage-commercial-release-proofs",
                "verify-commercial-release-artifacts",
            ],
            risks: vec!["production remote smoke, signed multi-platform artifacts, and release channel proofs are external blockers".to_string()],
        },
        ReferenceCapabilityReport {
            id: "knowledge-agent-foundation",
            domain: "context/agents",
            status: "in_progress",
            references: vec!["AutoGen", "MetaGPT", "LangChain", "OpenHands"],
            surfaces: vec!["context-index", "context-search", "context-vector-search", "context-pack", "context-artifact-ingest", "context-artifact-graph", "team-runtime", "notebook-execution", "subagent-tool-contract"],
            evidence: vec![
                "deterministic-context-index",
                "path-aware-context-search",
                "deterministic-hash-vector-search",
                "root-scoped-context-pack",
                "durable-context-artifact-ingest",
                "context-pack-artifact-graph",
                "deterministic-team-runtime-smoke",
                "notebook-execution-isolation-smoke",
            ],
            risks: vec!["production embedding backends/vector stores, production artifact sync semantics beyond local file ingest, notebook kernel parity/stronger OS sandboxing, and richer role-runtime UX remain future work".to_string()],
        },
    ]
}

fn bridge_access_token_configured() -> bool {
    std::env::var("KIANA_BRIDGE_ACCESS_TOKEN")
        .or_else(|_| std::env::var("CLAUDE_ACCESS_TOKEN"))
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
        || matches!(
            oauth_token_file_status(),
            OAuthTokenFileStatus::AccessOnly | OAuthTokenFileStatus::Refreshable
        )
}

fn code_session_live_smoke_token_status() -> CodeSessionLiveSmokeTokenStatus {
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
            return CodeSessionLiveSmokeTokenStatus::AnthropicApiKeyMisuse;
        }
        return CodeSessionLiveSmokeTokenStatus::Configured(source);
    }
    if matches!(
        oauth_token_file_status(),
        OAuthTokenFileStatus::AccessOnly | OAuthTokenFileStatus::Refreshable
    ) {
        return CodeSessionLiveSmokeTokenStatus::Configured("oauth_file");
    }
    CodeSessionLiveSmokeTokenStatus::Missing
}

fn oauth_token_file_status() -> OAuthTokenFileStatus {
    match kiana_services::oauth::load_oauth_tokens() {
        Ok(Some(tokens)) => {
            if tokens
                .refresh_token
                .as_deref()
                .map(str::trim)
                .is_some_and(|value| !value.is_empty())
            {
                OAuthTokenFileStatus::Refreshable
            } else {
                OAuthTokenFileStatus::AccessOnly
            }
        }
        Ok(None) => OAuthTokenFileStatus::Missing,
        Err(error) => OAuthTokenFileStatus::Invalid(error.to_string()),
    }
}

fn command_first_line(program: &str, args: &[&str]) -> Option<String> {
    let output = ProcessCommand::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

fn remote_settings_status() -> String {
    std::env::var("KIANA_REMOTE_SETTINGS_STATUS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "not_loaded".to_string())
}

fn remote_settings_file_label() -> String {
    let Some(path) = std::env::var_os("KIANA_REMOTE_SETTINGS_FILE") else {
        return "none".to_string();
    };
    let path = std::path::PathBuf::from(path);
    format!(
        "{} ({})",
        path.display(),
        if path.is_file() { "found" } else { "missing" }
    )
}

#[cfg(test)]
mod tests {
    use super::{commercial_security_report_for_values, DoctorCommand};
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use kiana_tools::bash_sandbox::{BashSandboxDiagnostic, BashSandboxStatus};
    use serde_json::json;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

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

    fn temp_path(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-doctor-{name}-{}-{unique}",
            std::process::id()
        ))
    }

    fn sandbox(enabled: bool, status: BashSandboxStatus) -> BashSandboxDiagnostic {
        BashSandboxDiagnostic {
            enabled,
            fail_if_unavailable: enabled,
            allow_unsandboxed_commands: !enabled,
            bwrap_path: None,
            status,
        }
    }

    #[test]
    fn commercial_security_linux_requires_strict_bwrap_controls() {
        let report = commercial_security_report_for_values(
            "linux",
            "commercial",
            "ask",
            &sandbox(false, BashSandboxStatus::Disabled),
        );

        assert!(!report.ready);
        assert_eq!(report.platform, "linux");
        assert_eq!(report.isolation, "linux_bwrap");
        assert!(report
            .controls
            .contains(&"bash_sandbox:fail_if_unavailable".to_string()));
        assert!(report
            .issues
            .contains(&"enable bash sandbox in config sandbox.enabled=true".to_string()));
        assert!(report
            .issues
            .contains(&"install/configure bubblewrap (bwrap) for bash sandbox".to_string()));
    }

    #[test]
    fn commercial_security_windows_uses_exec_policy_without_bwrap_requirement() {
        let report = commercial_security_report_for_values(
            "windows",
            "commercial",
            "ask",
            &sandbox(false, BashSandboxStatus::Disabled),
        );

        assert!(report.ready);
        assert_eq!(report.status, "ready");
        assert_eq!(report.platform, "windows");
        assert_eq!(report.isolation, "windows_exec_policy");
        assert!(report
            .controls
            .contains(&"platform_shell:windows-powershell-policy".to_string()));
        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.contains("bubblewrap") || issue.contains("bash sandbox")));
    }

    #[test]
    fn commercial_security_rejects_unknown_platform_model() {
        let report = commercial_security_report_for_values(
            "solaris",
            "commercial",
            "ask",
            &sandbox(false, BashSandboxStatus::Disabled),
        );

        assert!(!report.ready);
        assert_eq!(report.isolation, "unsupported_platform");
        assert!(report.issues.iter().any(|issue| issue.contains("solaris")));
    }

    #[tokio::test]
    async fn doctor_reports_wired_transports_without_stale_warning() {
        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("mcp_transport: stdio,http,sse,ws wired; surfaces: tools,resources,resource_templates,prompts"));
        assert!(result.value.contains("remote_bridge: start command wired"));
        assert!(result.value.contains("modifiers: platform="));
        assert!(result.value.contains("remote_settings:"));
        assert!(!result.value.contains("not wired yet"));
    }

    #[tokio::test]
    async fn doctor_reports_tui_permission_request_state() {
        let mut app_state = HashMap::new();
        app_state.insert("tui_permission_request_active".to_string(), json!(true));
        app_state.insert("tui_permission_request_queue_len".to_string(), json!(2));

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state,
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("tui_permission_request: active=yes queued=2"));
    }

    #[tokio::test]
    async fn doctor_json_reports_stable_contract() {
        let _lock = env_lock().lock().unwrap();
        let token_path = temp_path("missing-oauth-token-file");
        let token_path_str = token_path.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_REMOTE_ACCESS_TOKEN", None),
            ("KIANA_BRIDGE_ACCESS_TOKEN", None),
            ("CLAUDE_ACCESS_TOKEN", None),
            ("ANTHROPIC_AUTH_TOKEN", None),
            ("KIANA_OAUTH_TOKENS_FILE", Some(&token_path_str)),
        ]);
        let mut app_state = HashMap::new();
        app_state.insert("tui_permission_request_active".to_string(), json!(true));
        app_state.insert("tui_permission_request_queue_len".to_string(), json!(2));

        let result = DoctorCommand
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state,
            })
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(report["schema"], "kiana.doctor.v1");
        assert!(report["status"].is_string());
        assert!(report["cargo"]["available"].is_boolean());
        assert!(report["git_root"]["value"].is_string());
        assert_eq!(report["tui_permission_request"]["active"], true);
        assert_eq!(report["tui_permission_request"]["queued"], 2);
        assert_eq!(report["mcp_transport"]["wired"], true);
        assert!(report["remote_bridge"]["token_configured"].is_boolean());
        assert_eq!(report["remote_code_session"]["live_smoke_token"], "no");
        assert_eq!(report["oauth_token_file"]["status"], "missing");
        assert!(report["bash_sandbox"]["enabled"].is_boolean());
        assert!(report["commercial_security"]["platform"].is_string());
        assert!(report["commercial_security"]["isolation"].is_string());
        assert!(report["commercial_security"]["controls"].is_array());
        assert!(report["commercial_security"]["issues"].is_array());
        assert_eq!(report["tool_parity"]["schema"], "kiana.tool-parity.v1");
        assert_eq!(report["tool_parity"]["source"], "builtin-registry");
        assert_eq!(report["tool_parity"]["plugin_tools_included"], false);
        assert!(report["tool_parity"]["built_in_tools"].is_array());
        assert!(report["reference_capabilities"].is_array());
        assert!(report["reference_capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "provider-registry"
                && item["status"] == "local_ready_external_required"
                && item["surfaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|surface| surface == "model-smoke")));
        assert!(report["reference_capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["id"] == "remote-commercial-release"
                && item["status"] == "external_required"
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "stage-commercial-release-proofs")));
        assert!(report["warnings"].is_array());
    }

    #[tokio::test]
    async fn doctor_text_reports_reference_capability_matrix() {
        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("capability: runtime-session-core status=ready"));
        assert!(result
            .value
            .contains("capability: provider-registry status=local_ready_external_required"));
        assert!(result
            .value
            .contains("capability: remote-commercial-release status="));
        assert!(result
            .value
            .contains("capability: plugin-extension-contracts status=ready"));
        assert!(result.value.contains("mcp-prompt-lifecycle"));
        assert!(result.value.contains("plugin-component-json-preflight"));
        assert!(result.value.contains("tool_parity: "));
        assert!(result.value.contains("plugin_tools_included=no"));
        assert!(result
            .value
            .contains("capability_risk: provider-registry: production-like provider live catalog and smoke proof are external release blockers"));
    }

    #[tokio::test]
    async fn doctor_json_reports_reference_capability_matrix() {
        let result = DoctorCommand
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_str(&result.value).unwrap();
        let capabilities = report["reference_capabilities"].as_array().unwrap();

        assert!(capabilities
            .iter()
            .any(|item| item["id"] == "runtime-session-core"
                && item["status"] == "ready"
                && item["references"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|reference| reference == "codex")));
        assert!(capabilities
            .iter()
            .any(|item| item["id"] == "security-policy"
                && item["surfaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|surface| surface == "commercial-security")));
        assert!(capabilities
            .iter()
            .any(|item| item["id"] == "tool-lifecycle-mcp"
                && item["surfaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|surface| surface == "mcp-prompts")
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "mcp-prompt-lifecycle")));
        assert!(capabilities
            .iter()
            .any(|item| item["id"] == "plugin-extension-contracts"
                && item["status"] == "ready"
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "plugin-component-json-preflight")));
        assert!(capabilities
            .iter()
            .any(|item| item["id"] == "knowledge-agent-foundation"
                && item["status"] == "in_progress"
                && item["surfaces"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|surface| surface == "team-runtime")
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "deterministic-hash-vector-search")
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "durable-context-artifact-ingest")
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "deterministic-team-runtime-smoke")
                && item["evidence"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|evidence| evidence == "notebook-execution-isolation-smoke")
                && item["risks"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|risk| risk.as_str().unwrap().contains("richer role-runtime UX"))));
    }

    #[tokio::test]
    async fn doctor_json_reports_complete_local_coding_workflow_audit() {
        let result = DoctorCommand
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_str(&result.value).unwrap();
        let capabilities = report["reference_capabilities"].as_array().unwrap();
        let local = capabilities
            .iter()
            .find(|item| item["id"] == "local-coding-workflow")
            .expect("local-coding-workflow capability missing");

        assert_eq!(local["status"], "ready");
        let evidence = local["evidence"].as_array().unwrap();
        for expected in [
            "deterministic-repo-map-budget",
            "editable-read-only-file-sets",
            "write-edit-delete-file-changes",
            "checkpoint-create-restore-undo",
            "last-assistant-diff",
            "isolated-review",
            "isolated-checks",
            "repair-checks-nonstreaming",
            "repair-checks-streaming",
            "late-user-edit-conflict",
        ] {
            assert!(
                evidence.iter().any(|item| item == expected),
                "missing local coding audit evidence: {expected}"
            );
        }

        assert!(local["risks"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn doctor_json_reports_tool_parity_snapshot() {
        let result = DoctorCommand
            .execute(CommandContext {
                args: "--json".to_string(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();
        let report: serde_json::Value = serde_json::from_str(&result.value).unwrap();
        let parity = &report["tool_parity"];

        assert_eq!(parity["schema"], "kiana.tool-parity.v1");
        assert_eq!(parity["source"], "builtin-registry");
        assert_eq!(parity["plugin_tools_included"], false);
        assert!(parity["total_tools"].as_u64().unwrap() >= 40);
        assert!(parity["read_only_tools"].as_u64().unwrap() >= 3);
        assert!(parity["concurrency_safe_tools"].as_u64().unwrap() >= 3);

        let tools = parity["built_in_tools"].as_array().unwrap();
        for name in [
            "Read",
            "Write",
            "Edit",
            "Bash",
            "MCP",
            "TaskCreate",
            "NotebookEdit",
        ] {
            assert!(
                tools.iter().any(|tool| tool["name"] == name),
                "{name} missing from tool parity snapshot"
            );
        }

        let read = tools
            .iter()
            .find(|tool| tool["name"] == "Read")
            .expect("Read missing from tool parity snapshot");
        assert_eq!(read["source"], "builtin");
        assert_eq!(read["read_only"], true);
        assert_eq!(read["concurrency_safe"], true);
        assert!(read["workbench"].is_null());

        let mcp = tools
            .iter()
            .find(|tool| tool["name"] == "MCP")
            .expect("MCP missing from tool parity snapshot");
        assert_eq!(mcp["workbench"], "mcp");

        assert!(!tools
            .iter()
            .any(|tool| tool["name"] == "review-tools:code-audit"));
    }

    #[tokio::test]
    async fn doctor_reports_code_session_live_smoke_token_misuse() {
        let _lock = env_lock().lock().unwrap();
        let _guard = EnvGuard::set(&[
            ("KIANA_REMOTE_ACCESS_TOKEN", None),
            ("CLAUDE_ACCESS_TOKEN", None),
            ("ANTHROPIC_AUTH_TOKEN", Some("sk-test-api-key")),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains(
            "remote_code_session: live_smoke_token=invalid (ANTHROPIC_AUTH_TOKEN is sk-* API key)"
        ));
        assert!(result.value.contains(
            "warning: live smoke requires KIANA_REMOTE_ACCESS_TOKEN or CLAUDE_ACCESS_TOKEN; ANTHROPIC_AUTH_TOKEN=sk-* is an API key, not a remote access token"
        ));
    }

    #[tokio::test]
    async fn doctor_reports_oauth_file_as_remote_token_source() {
        let _lock = env_lock().lock().unwrap();
        let token_path = temp_path("oauth-token-file");
        let token_path_str = token_path.to_string_lossy().to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_REMOTE_ACCESS_TOKEN", None),
            ("KIANA_BRIDGE_ACCESS_TOKEN", None),
            ("CLAUDE_ACCESS_TOKEN", None),
            ("ANTHROPIC_AUTH_TOKEN", None),
            ("KIANA_OAUTH_TOKENS_FILE", Some(&token_path_str)),
        ]);
        kiana_services::oauth::save_oauth_tokens(&kiana_services::oauth::OAuthTokens {
            access_token: "oauth-access-token".to_string(),
            refresh_token: Some("oauth-refresh-token".to_string()),
            expires_at: None,
        })
        .unwrap();

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result
            .value
            .contains("remote_bridge: start command wired with SDK runner; token_configured: yes"));
        assert!(result
            .value
            .contains("remote_code_session: live_smoke_token=yes (oauth_file)"));
        assert!(result.value.contains("oauth_token_file: refreshable"));

        let _ = fs::remove_file(token_path);
    }

    #[tokio::test]
    async fn doctor_reports_ready_bash_sandbox_when_bwrap_is_available() {
        let _lock = env_lock().lock().unwrap();
        let bwrap = temp_path("fake-bwrap");
        fs::write(&bwrap, "#!/bin/sh\n").unwrap();
        let settings = json!({
            "sandbox": {
                "enabled": true,
                "failIfUnavailable": true,
                "allowUnsandboxedCommands": false,
                "bwrapPath": bwrap,
            }
        })
        .to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_SETTINGS_JSON", Some(&settings)),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains(
            "bash_sandbox: enabled=yes status=ready runtime=bwrap fail_if_unavailable=yes allow_unsandboxed_commands=no"
        ));
        assert!(result.value.contains(&format!("bwrap={}", bwrap.display())));
        assert!(!result
            .value
            .contains("warning: bash sandbox enabled with failIfUnavailable=true"));

        let _ = fs::remove_file(bwrap);
    }

    #[tokio::test]
    async fn doctor_warns_when_required_bash_sandbox_lacks_bwrap() {
        let _lock = env_lock().lock().unwrap();
        let path_dir = temp_path("empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let _guard = EnvGuard::set(&[
            (
                "KIANA_SETTINGS_JSON",
                Some(
                    r#"{"sandbox":{"enabled":true,"failIfUnavailable":true,"allowUnsandboxedCommands":false}}"#,
                ),
            ),
            ("PATH", Some(path_dir.to_str().unwrap())),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains(
            "bash_sandbox: enabled=yes status=unavailable runtime=missing fail_if_unavailable=yes allow_unsandboxed_commands=no bwrap=missing"
        ));
        assert!(result.value.contains(
            "warning: bash sandbox enabled with failIfUnavailable=true, but bubblewrap (bwrap) is not available"
        ));

        let _ = fs::remove_dir_all(path_dir);
    }

    #[tokio::test]
    async fn doctor_reports_commercial_security_ready() {
        let _lock = env_lock().lock().unwrap();
        let bwrap = temp_path("commercial-bwrap");
        fs::write(&bwrap, "#!/bin/sh\n").unwrap();
        let settings = json!({
            "sandbox": {
                "enabled": true,
                "failIfUnavailable": true,
                "allowUnsandboxedCommands": false,
                "bwrapPath": bwrap,
            }
        })
        .to_string();
        let _guard = EnvGuard::set(&[
            ("KIANA_SETTINGS_JSON", Some(&settings)),
            ("KIANA_PERMISSION_PROFILE", Some("commercial")),
            ("KIANA_PERMISSION_MODE", None),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("commercial_security: ready"));
        assert!(!result.value.contains("commercial_security_issue:"));

        let _ = fs::remove_file(bwrap);
    }

    #[tokio::test]
    async fn doctor_reports_commercial_security_gaps() {
        let _lock = env_lock().lock().unwrap();
        let path_dir = temp_path("commercial-empty-path");
        fs::create_dir_all(&path_dir).unwrap();
        let _guard = EnvGuard::set(&[
            (
                "KIANA_SETTINGS_JSON",
                Some(r#"{"sandbox":{"enabled":false}}"#),
            ),
            ("KIANA_PERMISSION_PROFILE", Some("workspace")),
            ("KIANA_PERMISSION_MODE", None),
            ("PATH", Some(path_dir.to_str().unwrap())),
            ("KIANA_BASH_SANDBOX", None),
            ("KIANA_BASH_SANDBOX_FAIL_IF_UNAVAILABLE", None),
            ("KIANA_BASH_SANDBOX_ALLOW_UNSANDBOXED", None),
            ("KIANA_BWRAP_PATH", None),
        ]);

        let result = DoctorCommand
            .execute(CommandContext {
                args: String::new(),
                app_state: HashMap::new(),
            })
            .await
            .unwrap();

        assert!(result.value.contains("commercial_security: not_ready"));
        assert!(result
            .value
            .contains(&format!("platform={}", std::env::consts::OS)));
        assert!(result
            .value
            .contains("commercial_security_issue: set `kiana permissions profile commercial`"));
        if std::env::consts::OS == "linux" {
            assert!(result
                .value
                .contains("commercial_security_issue: enable bash sandbox"));
        } else {
            assert!(!result
                .value
                .contains("commercial_security_issue: enable bash sandbox"));
        }

        let _ = fs::remove_dir_all(path_dir);
    }
}
