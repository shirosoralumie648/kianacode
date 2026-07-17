use kiana_types::{has_explicit_project_trust, project_trust_from_app_state, ProjectTrust};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionMode {
    #[serde(rename = "default")]
    Default,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "ask")]
    Ask,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionRule {
    pub pattern: String,
    pub action: PermissionAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionAction {
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
    #[serde(rename = "ask")]
    Ask,
}

pub struct PermissionContext {
    pub mode: PermissionMode,
    pub allow_rules: Vec<PermissionRule>,
    pub deny_rules: Vec<PermissionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ToolPermissionFile {
    #[serde(alias = "permissionProfile")]
    pub profile: Option<String>,
    pub mode: Option<String>,
    #[serde(alias = "allowedTools")]
    pub allowed_tools: Vec<String>,
    #[serde(alias = "disallowedTools")]
    pub disallowed_tools: Vec<String>,
    #[serde(alias = "askTools")]
    pub ask_tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EffectiveToolPermissions {
    pub profile: String,
    pub mode: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub ask_tools: Vec<String>,
    pub managed_allowed_tools: Vec<String>,
    pub managed_disallowed_tools: Vec<String>,
    pub managed_ask_tools: Vec<String>,
    pub file_path: PathBuf,
    pub file_loaded: bool,
    pub file_error: Option<String>,
    pub managed_policy_path: Option<PathBuf>,
    pub managed_policy_loaded: bool,
    pub managed_policy_error: Option<String>,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPermissionCheck {
    Allow,
    Deny(String),
    Ask(String),
}

impl PermissionContext {
    pub fn new() -> Self {
        Self {
            mode: PermissionMode::Default,
            allow_rules: Vec::new(),
            deny_rules: Vec::new(),
        }
    }

    pub fn check_path(&self, path: &str) -> bool {
        // Check deny rules first
        for rule in &self.deny_rules {
            if self.matches_pattern(&rule.pattern, path) {
                return false;
            }
        }

        // Check allow rules
        for rule in &self.allow_rules {
            if self.matches_pattern(&rule.pattern, path) {
                return true;
            }
        }

        // Default behavior based on mode
        matches!(self.mode, PermissionMode::Auto)
    }

    fn matches_pattern(&self, pattern: &str, path: &str) -> bool {
        // Simple wildcard matching
        if pattern.contains('*') {
            if let Ok(glob_pattern) = glob::Pattern::new(pattern) {
                return glob_pattern.matches(path);
            }
        }

        path.contains(pattern)
    }
}

impl Default for PermissionContext {
    fn default() -> Self {
        Self::new()
    }
}

pub fn tool_permissions_file_path() -> PathBuf {
    if let Some(path) = std::env::var_os("KIANA_PERMISSIONS_FILE") {
        return PathBuf::from(path);
    }

    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kiana")
        .join("permissions.json")
}

pub fn managed_policy_file_path() -> Option<PathBuf> {
    std::env::var_os("KIANA_MANAGED_PERMISSIONS_FILE")
        .or_else(|| std::env::var_os("KIANA_MANAGED_POLICY_FILE"))
        .map(PathBuf::from)
}

pub fn read_tool_permissions_file() -> Result<ToolPermissionFile, String> {
    let path = tool_permissions_file_path();
    if !path.exists() {
        return Ok(ToolPermissionFile::default());
    }

    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))
}

fn read_managed_policy_file(path: &PathBuf) -> Result<ToolPermissionFile, String> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        format!(
            "failed to read managed policy {}: {}",
            path.display(),
            error
        )
    })?;
    let policy: ManagedPolicyFile = serde_json::from_str(&content).map_err(|error| {
        format!(
            "failed to parse managed policy {}: {}",
            path.display(),
            error
        )
    })?;
    Ok(policy.into_permissions())
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct ManagedPolicyFile {
    #[serde(flatten)]
    flat_permissions: ToolPermissionFile,
    permissions: Option<ToolPermissionFile>,
}

impl ManagedPolicyFile {
    fn into_permissions(self) -> ToolPermissionFile {
        self.permissions.unwrap_or(self.flat_permissions)
    }
}

pub fn write_tool_permissions_file(file: &ToolPermissionFile) -> Result<PathBuf, String> {
    let path = tool_permissions_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {}", parent.display(), error))?;
    }
    let content = serde_json::to_string_pretty(file)
        .map_err(|error| format!("failed to serialize permissions: {}", error))?;
    std::fs::write(&path, format!("{content}\n"))
        .map_err(|error| format!("failed to write {}: {}", path.display(), error))?;
    Ok(path)
}

pub fn effective_tool_permissions(app_state: &HashMap<String, Value>) -> EffectiveToolPermissions {
    let file_path = tool_permissions_file_path();
    let mut profile = "workspace".to_string();
    let mut mode = "default".to_string();
    let mut allowed_tools = Vec::new();
    let mut disallowed_tools = Vec::new();
    let mut ask_tools = Vec::new();
    let mut managed_allowed_tools = Vec::new();
    let mut managed_disallowed_tools = Vec::new();
    let mut managed_ask_tools = Vec::new();
    let mut sources = Vec::new();
    let mut file_loaded = false;
    let mut file_error = None;
    let managed_policy_path = managed_policy_file_path();
    let mut managed_policy_loaded = false;
    let mut managed_policy_error = None;

    match read_tool_permissions_file() {
        Ok(file) => {
            if file_path.exists() {
                file_loaded = true;
                sources.push("file".to_string());
            }
            if let Some(file_profile) = normalized_profile(file.profile.as_deref()) {
                profile = file_profile.clone();
                mode = profile_default_mode(&profile);
                sources.push("file_profile".to_string());
            }
            if let Some(file_mode) = normalized_mode(file.mode.as_deref()) {
                mode = file_mode;
                sources.push("file_mode".to_string());
            }
            extend_unique(&mut allowed_tools, file.allowed_tools);
            extend_unique(&mut disallowed_tools, file.disallowed_tools);
            extend_unique(&mut ask_tools, file.ask_tools);
        }
        Err(error) => {
            file_error = Some(error);
            sources.push("file_error".to_string());
        }
    }

    if let Ok(value) = std::env::var("KIANA_PERMISSION_PROFILE") {
        if let Some(env_profile) = normalized_profile(Some(value.as_str())) {
            profile = env_profile.clone();
            mode = profile_default_mode(&profile);
            sources.push("env_profile".to_string());
        }
    }
    if let Ok(env_mode) = std::env::var("KIANA_PERMISSION_MODE") {
        if let Some(env_mode) = normalized_mode(Some(env_mode.as_str())) {
            mode = env_mode;
            sources.push("env".to_string());
        }
    }
    if let Ok(value) = std::env::var("KIANA_ALLOWED_TOOLS") {
        let rules = parse_rule_list(&value);
        if !rules.is_empty() {
            sources.push("env_allowed".to_string());
            extend_unique(&mut allowed_tools, rules);
        }
    }
    if let Ok(value) = std::env::var("KIANA_DISALLOWED_TOOLS") {
        let rules = parse_rule_list(&value);
        if !rules.is_empty() {
            sources.push("env_disallowed".to_string());
            extend_unique(&mut disallowed_tools, rules);
        }
    }
    if let Ok(value) = std::env::var("KIANA_ASK_TOOLS") {
        let rules = parse_rule_list(&value);
        if !rules.is_empty() {
            sources.push("env_ask".to_string());
            extend_unique(&mut ask_tools, rules);
        }
    }

    if let Some(app_profile) = app_state_permission_profile(app_state) {
        profile = app_profile.clone();
        mode = profile_default_mode(&profile);
        sources.push("session_profile".to_string());
    }
    if let Some(app_mode) = app_state_permission_mode(app_state) {
        mode = app_mode;
        sources.push("session".to_string());
    }
    let app_allowed = app_state_rule_array(app_state, "allowed_tools");
    if !app_allowed.is_empty() {
        sources.push("session_allowed".to_string());
        extend_unique(&mut allowed_tools, app_allowed);
    }
    let app_disallowed = app_state_rule_array(app_state, "disallowed_tools");
    if !app_disallowed.is_empty() {
        sources.push("session_disallowed".to_string());
        extend_unique(&mut disallowed_tools, app_disallowed);
    }
    let app_ask = app_state_rule_array(app_state, "ask_tools");
    if !app_ask.is_empty() {
        sources.push("session_ask".to_string());
        extend_unique(&mut ask_tools, app_ask);
    }

    if let Some(path) = &managed_policy_path {
        match read_managed_policy_file(path) {
            Ok(policy) => {
                managed_policy_loaded = true;
                sources.push("managed_policy".to_string());
                if let Some(managed_profile) = normalized_profile(policy.profile.as_deref()) {
                    profile = managed_profile.clone();
                    mode = profile_default_mode(&managed_profile);
                    sources.push("managed_profile".to_string());
                }
                if let Some(managed_mode) = normalized_mode(policy.mode.as_deref()) {
                    mode = managed_mode;
                    sources.push("managed_mode".to_string());
                }
                if !policy.allowed_tools.is_empty() {
                    sources.push("managed_allowed".to_string());
                    extend_unique(&mut managed_allowed_tools, policy.allowed_tools);
                }
                if !policy.disallowed_tools.is_empty() {
                    sources.push("managed_disallowed".to_string());
                    extend_unique(&mut managed_disallowed_tools, policy.disallowed_tools);
                }
                if !policy.ask_tools.is_empty() {
                    sources.push("managed_ask".to_string());
                    extend_unique(&mut managed_ask_tools, policy.ask_tools);
                }
            }
            Err(error) => {
                managed_policy_error = Some(error);
                sources.push("managed_policy_error".to_string());
            }
        }
    }

    EffectiveToolPermissions {
        profile,
        mode,
        allowed_tools,
        disallowed_tools,
        ask_tools,
        managed_allowed_tools,
        managed_disallowed_tools,
        managed_ask_tools,
        file_path,
        file_loaded,
        file_error,
        managed_policy_path,
        managed_policy_loaded,
        managed_policy_error,
        sources,
    }
}

pub fn permission_denial_for_tool(
    tool_name: &str,
    is_read_only: bool,
    input: &Value,
    app_state: &HashMap<String, Value>,
) -> Option<String> {
    match permission_check_for_tool(tool_name, is_read_only, input, app_state) {
        ToolPermissionCheck::Allow => None,
        ToolPermissionCheck::Deny(reason) => Some(reason),
        ToolPermissionCheck::Ask(reason) => Some(format!(
            "{reason} Run from an interactive terminal to approve once, add an allow rule with `kiana permissions allow {tool_name}`, or set KIANA_ALLOWED_TOOLS."
        )),
    }
}

pub fn permission_check_for_tool(
    tool_name: &str,
    is_read_only: bool,
    input: &Value,
    app_state: &HashMap<String, Value>,
) -> ToolPermissionCheck {
    let is_read_only = (is_read_only || known_read_only_tool(tool_name))
        && !mcp_query_uses_inline_command(tool_name, input);

    let project_trust = project_trust_from_app_state(app_state);
    if !is_read_only && !project_trust.allows_project_resources() {
        let trust_reason = match project_trust {
            ProjectTrust::Unknown => "because project trust is unknown",
            ProjectTrust::Untrusted => "in an untrusted project",
            ProjectTrust::Trusted => unreachable!("trusted projects allow project resources"),
        };
        return ToolPermissionCheck::Deny(format!(
            "Tool {tool_name} is denied {trust_reason}. Run `kiana trust trust` from this project to allow mutating tools."
        ));
    }

    let settings = effective_tool_permissions(app_state);
    if !is_read_only && settings.profile == "commercial" && !has_explicit_project_trust(app_state) {
        return ToolPermissionCheck::Deny(format!(
            "Tool {tool_name} is denied in commercial profile because project trust is not explicitly set. Run `kiana trust trust` from this project before allowing mutating tools."
        ));
    }

    if let Some(rule) = settings
        .managed_disallowed_tools
        .iter()
        .find(|rule| rule_matches_tool(rule, tool_name, input))
    {
        return ToolPermissionCheck::Deny(format!(
            "Tool {tool_name} is denied by managed policy rule \"{rule}\"."
        ));
    }

    if let Some(rule) = settings
        .disallowed_tools
        .iter()
        .find(|rule| rule_matches_tool(rule, tool_name, input))
    {
        return ToolPermissionCheck::Deny(format!(
            "Tool {tool_name} is denied by permission rule \"{rule}\"."
        ));
    }

    if let Some(rule) = settings
        .managed_ask_tools
        .iter()
        .find(|rule| rule_matches_tool(rule, tool_name, input))
    {
        return ToolPermissionCheck::Ask(format!(
            "Tool {tool_name} requires permission by managed policy ask rule \"{rule}\"."
        ));
    }

    if settings
        .managed_allowed_tools
        .iter()
        .any(|rule| rule_matches_tool(rule, tool_name, input))
    {
        return ToolPermissionCheck::Allow;
    }

    if let Some(rule) = settings
        .allowed_tools
        .iter()
        .find(|rule| rule_matches_tool(rule, tool_name, input))
    {
        if settings.profile == "commercial" && !is_read_only {
            return ToolPermissionCheck::Ask(format!(
                "Tool {tool_name} requires permission in commercial profile; normal allow rule \"{rule}\" cannot bypass managed approval."
            ));
        }
        return ToolPermissionCheck::Allow;
    }

    if let Some(rule) = settings
        .ask_tools
        .iter()
        .find(|rule| rule_matches_tool(rule, tool_name, input))
    {
        return ToolPermissionCheck::Ask(format!(
            "Tool {tool_name} requires permission by ask rule \"{rule}\"."
        ));
    }

    if settings.profile == "commercial" && !is_read_only {
        return ToolPermissionCheck::Ask(format!(
            "Tool {tool_name} requires permission in commercial profile."
        ));
    }

    match settings.mode.as_str() {
        "bypassPermissions" | "dontAsk" | "acceptEdits" | "auto" => ToolPermissionCheck::Allow,
        "plan" if !is_read_only => ToolPermissionCheck::Deny(format!(
            "Tool {tool_name} is denied in plan permission mode because it may modify state."
        )),
        "ask" if !is_read_only => {
            ToolPermissionCheck::Ask(format!("Tool {tool_name} requires permission in ask mode."))
        }
        _ => ToolPermissionCheck::Allow,
    }
}

pub fn prompt_for_tool_permission(tool_name: &str, input: &Value) -> Result<bool, String> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err(format!(
            "Tool {tool_name} requires permission in ask mode, but this process is not attached to an interactive terminal. Add an allow rule with `kiana permissions allow {tool_name}` or set KIANA_ALLOWED_TOOLS."
        ));
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stderr = io::stderr();
    let mut writer = stderr.lock();
    prompt_for_tool_permission_with_io(tool_name, input, &mut reader, &mut writer)
        .map_err(|error| format!("failed to read permission response: {error}"))
}

fn prompt_for_tool_permission_with_io<R, W>(
    tool_name: &str,
    input: &Value,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<bool>
where
    R: BufRead,
    W: Write,
{
    writeln!(writer, "Kiana permission request")?;
    writeln!(writer, "Tool: {tool_name}")?;
    if let Some(summary) = permission_input_summary(tool_name, input) {
        writeln!(writer, "{summary}")?;
    }
    write!(writer, "Allow once? [y/N]: ")?;
    writer.flush()?;

    let mut answer = String::new();
    reader.read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

fn permission_input_summary(tool_name: &str, input: &Value) -> Option<String> {
    if is_command_tool(tool_name) {
        return input
            .get("command")
            .and_then(Value::as_str)
            .map(|command| format!("Command: {}", shorten_for_prompt(command)));
    }

    for key in ["file_path", "path", "uri", "title"] {
        if let Some(value) = input.get(key).and_then(Value::as_str) {
            return Some(format!("{key}: {}", shorten_for_prompt(value)));
        }
    }

    None
}

fn shorten_for_prompt(value: &str) -> String {
    const MAX_CHARS: usize = 220;
    let mut chars = value.chars();
    let shortened: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{shortened}...")
    } else {
        shortened
    }
}

fn known_read_only_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        "Read"
            | "Grep"
            | "Glob"
            | "WebFetch"
            | "WebSearch"
            | "LSP"
            | "AskUserQuestion"
            | "SendUserMessage"
            | "send_user_file"
            | "Sleep"
            | "TaskGet"
            | "TaskList"
            | "TaskOutput"
            | "ListMcpResourcesTool"
            | "ListMcpResourceTemplatesTool"
            | "ListMcpPromptsTool"
            | "ReadMcpResourceTool"
            | "GetMcpPromptTool"
            | "StructuredOutput"
            | "discover_skills"
            | "ToolSearch"
            | "CronList"
            | "Monitor"
    )
}

fn mcp_query_uses_inline_command(tool_name: &str, input: &Value) -> bool {
    let is_mcp_query = matches!(
        tool_name,
        "ListMcpResourcesTool"
            | "ListMcpResourceTemplatesTool"
            | "ListMcpPromptsTool"
            | "ReadMcpResourceTool"
            | "GetMcpPromptTool"
    );
    is_mcp_query
        && [
            input.get("command").and_then(Value::as_str),
            input.pointer("/config/command").and_then(Value::as_str),
        ]
        .into_iter()
        .flatten()
        .any(|command| !command.trim().is_empty())
}

fn normalized_mode(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let normalized = match value {
        "acceptEdits" | "accept-edits" | "accept_edits" => "acceptEdits",
        "bypassPermissions" | "bypass-permissions" | "bypass_permissions" => "bypassPermissions",
        "default" => "default",
        "dontAsk" | "dont-ask" | "dont_ask" => "dontAsk",
        "plan" => "plan",
        "auto" => "auto",
        "ask" => "ask",
        other => other,
    };
    Some(normalized.to_string())
}

fn normalized_profile(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    let normalized = match value {
        "read-only" | "readonly" | "read_only" => "read-only",
        "workspace" | "default" => "workspace",
        "commercial" | "commercial-security" | "commercial_security" => "commercial",
        "full" | "bypassPermissions" | "bypass-permissions" | "bypass_permissions" => "full",
        "ask" => "ask",
        "plan" => "plan",
        other => other,
    };
    Some(normalized.to_string())
}

fn profile_default_mode(profile: &str) -> String {
    match profile {
        "read-only" | "plan" => "plan".to_string(),
        "full" => "bypassPermissions".to_string(),
        "commercial" => "ask".to_string(),
        "ask" => "ask".to_string(),
        _ => "default".to_string(),
    }
}

fn app_state_permission_mode(app_state: &HashMap<String, Value>) -> Option<String> {
    app_state
        .get("permission_mode")
        .and_then(Value::as_str)
        .and_then(|mode| normalized_mode(Some(mode)))
        .or_else(|| {
            app_state
                .get("permissions")
                .and_then(|value| value.get("mode"))
                .and_then(Value::as_str)
                .and_then(|mode| normalized_mode(Some(mode)))
        })
        .or_else(|| {
            app_state
                .get("config")
                .and_then(|value| value.get("permission_mode"))
                .and_then(Value::as_str)
                .and_then(|mode| normalized_mode(Some(mode)))
        })
}

fn app_state_permission_profile(app_state: &HashMap<String, Value>) -> Option<String> {
    app_state
        .get("permission_profile")
        .and_then(Value::as_str)
        .and_then(|profile| normalized_profile(Some(profile)))
        .or_else(|| {
            app_state
                .get("permissionProfile")
                .and_then(Value::as_str)
                .and_then(|profile| normalized_profile(Some(profile)))
        })
        .or_else(|| {
            app_state
                .get("permissions")
                .and_then(|value| value.get("profile"))
                .and_then(Value::as_str)
                .and_then(|profile| normalized_profile(Some(profile)))
        })
        .or_else(|| {
            app_state
                .get("permissions")
                .and_then(|value| value.get("permissionProfile"))
                .and_then(Value::as_str)
                .and_then(|profile| normalized_profile(Some(profile)))
        })
        .or_else(|| {
            app_state
                .get("config")
                .and_then(|value| value.get("permission_profile"))
                .and_then(Value::as_str)
                .and_then(|profile| normalized_profile(Some(profile)))
        })
        .or_else(|| {
            app_state
                .get("config")
                .and_then(|value| value.get("permissionProfile"))
                .and_then(Value::as_str)
                .and_then(|profile| normalized_profile(Some(profile)))
        })
}

fn app_state_rule_array(app_state: &HashMap<String, Value>, key: &str) -> Vec<String> {
    let camel_key = match key {
        "allowed_tools" => "allowedTools",
        "disallowed_tools" => "disallowedTools",
        "ask_tools" => "askTools",
        other => other,
    };
    let mut values = value_string_array(app_state.get(key));
    values.extend(value_string_array(app_state.get(camel_key)));
    values.extend(
        app_state
            .get("permissions")
            .map(|value| {
                let mut nested = value_string_array(value.get(key));
                nested.extend(value_string_array(value.get(camel_key)));
                nested
            })
            .unwrap_or_default(),
    );
    values
}

fn value_string_array(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect(),
        Some(Value::String(value)) => parse_rule_list(value),
        _ => Vec::new(),
    }
}

fn parse_rule_list(value: &str) -> Vec<String> {
    value
        .split(|ch| matches!(ch, ',' | '\n' | ';'))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect()
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&value))
        {
            target.push(value);
        }
    }
}

fn rule_matches_tool(rule: &str, tool_name: &str, input: &Value) -> bool {
    let rule = rule.trim();
    if rule.is_empty() {
        return false;
    }

    let (rule_tool, rule_content) = split_tool_rule(rule);
    if !name_matches(rule_tool, tool_name) {
        return false;
    }

    let Some(rule_content) = rule_content else {
        return true;
    };

    if is_command_tool(tool_name) {
        let command = input
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return content_matches(rule_content, command);
    }

    true
}

fn is_command_tool(tool_name: &str) -> bool {
    matches!(
        tool_name.to_ascii_lowercase().as_str(),
        "bash" | "powershell"
    )
}

fn split_tool_rule(rule: &str) -> (&str, Option<&str>) {
    if let Some(open) = rule.find('(') {
        if rule.ends_with(')') && open < rule.len() - 1 {
            return (
                rule[..open].trim(),
                Some(rule[open + 1..rule.len() - 1].trim()),
            );
        }
    }
    (rule, None)
}

fn name_matches(pattern: &str, tool_name: &str) -> bool {
    if pattern == "*" || pattern.eq_ignore_ascii_case(tool_name) {
        return true;
    }
    let pattern_lower = pattern.to_ascii_lowercase();
    let tool_lower = tool_name.to_ascii_lowercase();
    glob::Pattern::new(&pattern_lower)
        .map(|pattern| pattern.matches(&tool_lower))
        .unwrap_or(false)
}

fn content_matches(pattern: &str, value: &str) -> bool {
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    let candidates = [pattern.to_string(), pattern.replace(':', " ")];
    candidates.iter().any(|candidate| {
        glob::Pattern::new(candidate)
            .map(|pattern| pattern.matches(value))
            .unwrap_or(false)
            || candidate
                .strip_suffix('*')
                .is_some_and(|prefix| value.starts_with(prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        effective_tool_permissions, permission_check_for_tool, permission_denial_for_tool,
        prompt_for_tool_permission_with_io, write_tool_permissions_file, ToolPermissionCheck,
        ToolPermissionFile,
    };
    use serde_json::json;
    use std::collections::HashMap;
    use std::io::Cursor;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    static PERMISSION_ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn with_isolated_permission_env<T>(label: &str, test: impl FnOnce(PathBuf) -> T) -> T {
        let _guard = PERMISSION_ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap();
        let keys = [
            "KIANA_PERMISSIONS_FILE",
            "KIANA_PERMISSION_PROFILE",
            "KIANA_PERMISSION_MODE",
            "KIANA_ALLOWED_TOOLS",
            "KIANA_DISALLOWED_TOOLS",
            "KIANA_ASK_TOOLS",
            "KIANA_MANAGED_POLICY_FILE",
            "KIANA_MANAGED_PERMISSIONS_FILE",
        ];
        let saved: Vec<(&str, Option<String>)> = keys
            .iter()
            .map(|key| (*key, std::env::var(key).ok()))
            .collect();
        for key in keys {
            std::env::remove_var(key);
        }

        let path = std::env::temp_dir().join(format!(
            "kiana-permissions-test-{}-{}.json",
            label,
            uuid::Uuid::new_v4()
        ));
        std::env::set_var("KIANA_PERMISSIONS_FILE", &path);
        let result = test(path.clone());
        let _ = std::fs::remove_file(path);

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        result
    }

    #[test]
    fn denies_explicit_session_disallow_rule() {
        with_isolated_permission_env("session-disallow", |_| {
            let app_state = HashMap::from([
                ("disallowed_tools".to_string(), json!(["Bash"])),
                ("project_trusted".to_string(), json!(true)),
            ]);
            let denial =
                permission_denial_for_tool("Bash", false, &json!({"command": "pwd"}), &app_state);
            assert!(denial.unwrap().contains("denied by permission rule"));
        });
    }

    #[test]
    fn ask_mode_denies_mutating_tools_without_allow_rule() {
        with_isolated_permission_env("ask-mode", |_| {
            let app_state = HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                ("project_trusted".to_string(), json!(true)),
            ]);
            let denial =
                permission_denial_for_tool("Bash", false, &json!({"command": "pwd"}), &app_state);
            assert!(denial.unwrap().contains("requires permission in ask mode"));
            assert!(!permission_denial_for_tool(
                "Bash",
                false,
                &json!({"command": "pwd"}),
                &app_state
            )
            .unwrap()
            .contains("not wired"));
            assert!(matches!(
                permission_check_for_tool("Bash", false, &json!({"command": "pwd"}), &app_state),
                ToolPermissionCheck::Ask(_)
            ));

            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn read_only_profile_denies_mutating_tools_but_allows_reads() {
        with_isolated_permission_env("read-only-profile", |_| {
            let app_state = HashMap::from([
                ("permission_profile".to_string(), json!("read-only")),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let settings = effective_tool_permissions(&app_state);
            assert_eq!(settings.profile, "read-only");
            assert_eq!(settings.mode, "plan");

            assert!(matches!(
                permission_check_for_tool("Bash", false, &json!({"command": "pwd"}), &app_state),
                ToolPermissionCheck::Deny(_)
            ));
            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn untrusted_project_denies_mutating_tools_but_allows_reads() {
        with_isolated_permission_env("untrusted-project", |_| {
            let app_state = HashMap::from([("project_trusted".to_string(), json!(false))]);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(
                matches!(bash, ToolPermissionCheck::Deny(reason) if reason.contains("untrusted project"))
            );

            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn unknown_project_denies_mutating_tools_but_allows_reads() {
        with_isolated_permission_env("unknown-project", |_| {
            let app_state = HashMap::new();

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(
                matches!(&bash, ToolPermissionCheck::Deny(reason) if reason.contains("project trust is unknown")),
                "{bash:?}"
            );
            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn unknown_project_user_allow_rule_cannot_bypass_trust_gate() {
        with_isolated_permission_env("unknown-user-allow", |_| {
            let app_state = HashMap::from([("allowed_tools".to_string(), json!(["Bash"]))]);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(
                matches!(&bash, ToolPermissionCheck::Deny(reason) if reason.contains("project trust is unknown")),
                "{bash:?}"
            );
        });
    }

    #[test]
    fn unknown_project_bypass_permissions_cannot_bypass_trust_gate() {
        with_isolated_permission_env("unknown-bypass", |_| {
            let app_state =
                HashMap::from([("permission_mode".to_string(), json!("bypassPermissions"))]);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(
                matches!(&bash, ToolPermissionCheck::Deny(reason) if reason.contains("project trust is unknown")),
                "{bash:?}"
            );
        });
    }

    #[test]
    fn unknown_project_managed_allow_rule_cannot_bypass_trust_gate() {
        with_isolated_permission_env("unknown-managed-allow", |_| {
            let managed_path = std::env::temp_dir().join(format!(
                "kiana-unknown-managed-permissions-{}.json",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(
                &managed_path,
                r#"{
                  "permissions": {
                    "allowedTools": ["Bash"]
                  }
                }"#,
            )
            .unwrap();
            std::env::set_var("KIANA_MANAGED_POLICY_FILE", &managed_path);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &HashMap::new(),
            );
            assert!(
                matches!(&bash, ToolPermissionCheck::Deny(reason) if reason.contains("project trust is unknown")),
                "{bash:?}"
            );

            let _ = std::fs::remove_file(managed_path);
        });
    }

    #[test]
    fn inline_mcp_query_commands_require_trusted_project() {
        with_isolated_permission_env("inline-mcp-query", |_| {
            let tools = [
                "ListMcpResourcesTool",
                "ListMcpResourceTemplatesTool",
                "ListMcpPromptsTool",
                "ReadMcpResourceTool",
                "GetMcpPromptTool",
            ];
            let inline_inputs = [
                json!({"server": "inline", "command": "untrusted-mcp"}),
                json!({
                    "server": "inline",
                    "config": {"transport": "stdio", "command": "untrusted-mcp"}
                }),
            ];
            let trust_states = [
                ("unknown", HashMap::new()),
                (
                    "untrusted",
                    HashMap::from([("project_trusted".to_string(), json!(false))]),
                ),
            ];

            for (trust_state, app_state) in trust_states {
                for tool_name in tools {
                    assert!(matches!(
                        permission_check_for_tool(
                            tool_name,
                            true,
                            &json!({"server": "user-configured"}),
                            &app_state,
                        ),
                        ToolPermissionCheck::Allow
                    ));

                    for input in &inline_inputs {
                        let decision =
                            permission_check_for_tool(tool_name, true, input, &app_state);
                        assert!(
                            matches!(&decision, ToolPermissionCheck::Deny(reason) if reason.contains(trust_state)),
                            "{tool_name} with {input} was not denied for {trust_state}: {decision:?}"
                        );
                    }
                }
            }
        });
    }

    #[test]
    fn commercial_profile_requires_permission_for_mutating_tools() {
        with_isolated_permission_env("commercial-profile", |_| {
            let app_state = HashMap::from([
                ("permission_profile".to_string(), json!("commercial")),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let settings = effective_tool_permissions(&app_state);
            assert_eq!(settings.profile, "commercial");
            assert_eq!(settings.mode, "ask");

            assert!(matches!(
                permission_check_for_tool("Bash", false, &json!({"command": "pwd"}), &app_state),
                ToolPermissionCheck::Ask(_)
            ));
            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn commercial_profile_cannot_bypass_unknown_project_trust() {
        with_isolated_permission_env("commercial-explicit-trust", |_| {
            let app_state =
                HashMap::from([("permission_profile".to_string(), json!("commercial"))]);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(
                matches!(bash, ToolPermissionCheck::Deny(reason) if reason.contains("project trust is unknown"))
            );
            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn commercial_profile_does_not_let_normal_allow_rules_bypass_mutating_tools() {
        with_isolated_permission_env("commercial-normal-allow", |_| {
            let app_state = HashMap::from([
                ("permission_profile".to_string(), json!("commercial")),
                ("permission_mode".to_string(), json!("bypassPermissions")),
                ("allowed_tools".to_string(), json!(["Bash", "Read"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(
                matches!(bash, ToolPermissionCheck::Ask(reason) if reason.contains("normal allow rule"))
            );
            assert!(permission_denial_for_tool("Read", true, &json!({}), &app_state).is_none());
        });
    }

    #[test]
    fn commercial_profile_allows_mutating_tools_from_managed_allow_rules() {
        with_isolated_permission_env("commercial-managed-allow", |_| {
            let managed_path = std::env::temp_dir().join(format!(
                "kiana-commercial-managed-permissions-{}.json",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(
                &managed_path,
                r#"{
                  "permissions": {
                    "profile": "commercial",
                    "allowedTools": ["Bash"]
                  }
                }"#,
            )
            .unwrap();
            std::env::set_var("KIANA_MANAGED_POLICY_FILE", &managed_path);

            let app_state = HashMap::from([
                ("permission_profile".to_string(), json!("commercial")),
                ("allowed_tools".to_string(), json!(["Bash"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(matches!(bash, ToolPermissionCheck::Allow));

            let _ = std::fs::remove_file(managed_path);
        });
    }

    #[test]
    fn deny_rules_take_precedence_over_allow_ask_and_mode() {
        with_isolated_permission_env("deny-precedence", |_| {
            let app_state = HashMap::from([
                ("permission_mode".to_string(), json!("auto")),
                ("allowed_tools".to_string(), json!(["Bash"])),
                ("ask_tools".to_string(), json!(["Bash"])),
                ("disallowed_tools".to_string(), json!(["Bash"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let decision = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );

            assert!(matches!(decision, ToolPermissionCheck::Deny(_)));
        });
    }

    #[test]
    fn ask_rules_take_precedence_over_permissive_mode_after_allow_miss() {
        with_isolated_permission_env("ask-precedence", |_| {
            let app_state = HashMap::from([
                ("permission_mode".to_string(), json!("bypassPermissions")),
                ("allowed_tools".to_string(), json!(["Bash(git:*)"])),
                ("ask_tools".to_string(), json!(["Bash(rm:*)"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            assert!(matches!(
                permission_check_for_tool(
                    "Bash",
                    false,
                    &json!({"command": "git status"}),
                    &app_state
                ),
                ToolPermissionCheck::Allow
            ));
            assert!(matches!(
                permission_check_for_tool(
                    "Bash",
                    false,
                    &json!({"command": "rm -rf ."}),
                    &app_state
                ),
                ToolPermissionCheck::Ask(_)
            ));
            assert!(matches!(
                permission_check_for_tool(
                    "Bash",
                    false,
                    &json!({"command": "cargo test"}),
                    &app_state
                ),
                ToolPermissionCheck::Allow
            ));
        });
    }

    #[test]
    fn session_mode_overrides_env_profile_and_file_mode() {
        with_isolated_permission_env("session-precedence", |_| {
            write_tool_permissions_file(&ToolPermissionFile {
                profile: Some("full".to_string()),
                mode: Some("auto".to_string()),
                allowed_tools: Vec::new(),
                disallowed_tools: Vec::new(),
                ask_tools: Vec::new(),
            })
            .unwrap();
            std::env::set_var("KIANA_PERMISSION_PROFILE", "full");
            std::env::set_var("KIANA_PERMISSION_MODE", "bypassPermissions");

            let app_state = HashMap::from([
                ("permission_profile".to_string(), json!("read-only")),
                ("permission_mode".to_string(), json!("ask")),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let settings = effective_tool_permissions(&app_state);
            assert_eq!(settings.profile, "read-only");
            assert_eq!(settings.mode, "ask");
            assert!(settings.sources.contains(&"file_profile".to_string()));
            assert!(settings.sources.contains(&"env_profile".to_string()));
            assert!(settings.sources.contains(&"session_profile".to_string()));
            assert!(settings.sources.contains(&"session".to_string()));

            assert!(matches!(
                permission_check_for_tool("Bash", false, &json!({"command": "pwd"}), &app_state),
                ToolPermissionCheck::Ask(_)
            ));
        });
    }

    #[test]
    fn managed_policy_overrides_session_env_and_user_permissions() {
        with_isolated_permission_env("managed-policy", |_| {
            write_tool_permissions_file(&ToolPermissionFile {
                profile: Some("full".to_string()),
                mode: Some("bypassPermissions".to_string()),
                allowed_tools: vec!["Bash".to_string()],
                disallowed_tools: Vec::new(),
                ask_tools: Vec::new(),
            })
            .unwrap();

            let managed_path = std::env::temp_dir().join(format!(
                "kiana-managed-permissions-{}.json",
                uuid::Uuid::new_v4()
            ));
            std::fs::write(
                &managed_path,
                r#"{
                  "permissions": {
                    "profile": "read-only",
                    "disallowedTools": ["Bash"],
                    "askTools": ["Write"]
                  }
                }"#,
            )
            .unwrap();
            std::env::set_var("KIANA_MANAGED_POLICY_FILE", &managed_path);
            std::env::set_var("KIANA_PERMISSION_PROFILE", "full");
            std::env::set_var("KIANA_ALLOWED_TOOLS", "Bash,Write");

            let app_state = HashMap::from([
                ("permission_profile".to_string(), json!("full")),
                ("permission_mode".to_string(), json!("bypassPermissions")),
                ("allowed_tools".to_string(), json!(["Bash", "Write"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            let settings = effective_tool_permissions(&app_state);
            assert_eq!(settings.profile, "read-only");
            assert_eq!(settings.mode, "plan");
            assert!(settings.sources.contains(&"managed_policy".to_string()));
            assert!(settings.sources.contains(&"managed_disallowed".to_string()));
            assert!(settings.sources.contains(&"managed_ask".to_string()));

            let bash = permission_check_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            );
            assert!(matches!(bash, ToolPermissionCheck::Deny(_)));

            let write = permission_check_for_tool("Write", false, &json!({}), &app_state);
            assert!(matches!(write, ToolPermissionCheck::Ask(_)));

            let _ = std::fs::remove_file(managed_path);
        });
    }

    #[test]
    fn allow_rule_overrides_ask_mode_for_matching_bash_command() {
        with_isolated_permission_env("allow-bash-rule", |_| {
            let app_state = HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                ("allowed_tools".to_string(), json!(["Bash(git:*)"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            assert!(permission_denial_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            )
            .is_none());
            assert!(permission_denial_for_tool(
                "Bash",
                false,
                &json!({"command": "rm -rf ."}),
                &app_state,
            )
            .is_some());
        });
    }

    #[test]
    fn allow_rule_matches_powershell_command_content() {
        with_isolated_permission_env("allow-powershell-rule", |_| {
            let app_state = HashMap::from([
                ("permission_mode".to_string(), json!("ask")),
                ("allowed_tools".to_string(), json!(["PowerShell(Get-*)"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            assert!(permission_denial_for_tool(
                "PowerShell",
                false,
                &json!({"command": "Get-ChildItem ."}),
                &app_state,
            )
            .is_none());
            assert!(permission_denial_for_tool(
                "PowerShell",
                false,
                &json!({"command": "Set-Content file.txt value"}),
                &app_state,
            )
            .is_some());
        });
    }

    #[test]
    fn ask_rule_requires_permission_in_default_mode() {
        with_isolated_permission_env("ask-rule-default", |_| {
            let app_state = HashMap::from([
                ("ask_tools".to_string(), json!(["Bash(cargo:*)"])),
                ("project_trusted".to_string(), json!(true)),
            ]);

            assert!(matches!(
                permission_check_for_tool(
                    "Bash",
                    false,
                    &json!({"command": "cargo test"}),
                    &app_state
                ),
                ToolPermissionCheck::Ask(_)
            ));
            assert!(permission_denial_for_tool(
                "Bash",
                false,
                &json!({"command": "git status"}),
                &app_state,
            )
            .is_none());
        });
    }

    #[test]
    fn permission_prompt_accepts_yes_and_rejects_default() {
        let mut reader = Cursor::new("yes\n");
        let mut writer = Vec::new();
        assert!(prompt_for_tool_permission_with_io(
            "Bash",
            &json!({"command": "git status"}),
            &mut reader,
            &mut writer,
        )
        .unwrap());
        let prompt = String::from_utf8(writer).unwrap();
        assert!(prompt.contains("Tool: Bash"));
        assert!(prompt.contains("Command: git status"));

        let mut reader = Cursor::new("\n");
        let mut writer = Vec::new();
        assert!(!prompt_for_tool_permission_with_io(
            "Bash",
            &json!({"command": "git status"}),
            &mut reader,
            &mut writer,
        )
        .unwrap());
    }
}
