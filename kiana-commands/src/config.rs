use crate::local_state::{bool_label, config_path, load_user_config, on_off, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_bootstrap::config::Config;
use serde_json::{json, Value};

pub struct ConfigCommand;

#[async_trait]
impl Command for ConfigCommand {
    fn name(&self) -> &str {
        "config"
    }

    fn description(&self) -> &str {
        "Configure settings"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = parse_args(&context.args);
        match args.first().map(String::as_str).unwrap_or("status") {
            "" | "status" | "list" => {
                reject_extra_args(
                    args.first().map(String::as_str).unwrap_or("status"),
                    args.get(1..).unwrap_or_default(),
                )?;
                Ok(CommandResult::text(config_status()))
            }
            "init" | "setup" => {
                reject_extra_args(
                    args.first().map(String::as_str).unwrap_or("init"),
                    args.get(1..).unwrap_or_default(),
                )?;
                init_config()
            }
            "path" => {
                reject_extra_args("path", args.get(1..).unwrap_or_default())?;
                Ok(CommandResult::text(config_path().display().to_string()))
            }
            "json" => {
                reject_extra_args("json", args.get(1..).unwrap_or_default())?;
                Ok(CommandResult::text(serde_json::to_string_pretty(
                    &redacted_config_json(&kiana_bootstrap::config::load_config()),
                )?))
            }
            "resolved" => resolved_config(args.get(1..).unwrap_or_default()),
            "get" => get_config_value(args.get(1), args.get(2..).unwrap_or_default()),
            "set" => set_config_value(args.get(1), args.get(2..).unwrap_or_default()),
            "unset" | "reset" => unset_config_value(
                args.first().map(String::as_str).unwrap_or("unset"),
                args.get(1),
                args.get(2..).unwrap_or_default(),
            ),
            "help" | "--help" | "-h" => {
                reject_extra_args(
                    args.first().map(String::as_str).unwrap_or("help"),
                    args.get(1..).unwrap_or_default(),
                )?;
                Ok(CommandResult::text(usage()))
            }
            other => Err(anyhow!("unknown config command '{}'\n\n{}", other, usage())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigKey {
    ApiKey,
    BaseUrl,
    Model,
    Verbose,
    Brief,
    VimMode,
    Theme,
    AdvisorModel,
    OutputStyle,
    Sandbox,
}

impl ConfigKey {
    fn name(self) -> &'static str {
        match self {
            ConfigKey::ApiKey => "api_key",
            ConfigKey::BaseUrl => "base_url",
            ConfigKey::Model => "model",
            ConfigKey::Verbose => "verbose",
            ConfigKey::Brief => "brief",
            ConfigKey::VimMode => "vim_mode",
            ConfigKey::Theme => "theme",
            ConfigKey::AdvisorModel => "advisor_model",
            ConfigKey::OutputStyle => "output_style",
            ConfigKey::Sandbox => "sandbox",
        }
    }
}

fn config_status() -> String {
    let effective = kiana_bootstrap::config::load_config();
    let file = load_user_config();
    let path = config_path();
    let env_api_key = std::env::var("ANTHROPIC_API_KEY")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let env_base_url = std::env::var("ANTHROPIC_BASE_URL")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    let env_model = std::env::var("ANTHROPIC_MODEL")
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);

    vec![
        "Config status".to_string(),
        format!(
            "file: {} ({})",
            path.display(),
            if path.is_file() { "found" } else { "missing" }
        ),
        format!(
            "api_key: {} ({})",
            if effective
                .api_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                "set"
            } else {
                "missing"
            },
            if env_api_key {
                "ANTHROPIC_API_KEY"
            } else if file
                .api_key
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty())
            {
                "file"
            } else {
                "none"
            }
        ),
        format!(
            "base_url: {} ({})",
            effective
                .base_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com"),
            if env_base_url {
                "ANTHROPIC_BASE_URL"
            } else if file.base_url.is_some() {
                "file"
            } else {
                "default"
            }
        ),
        format!(
            "model: {} ({})",
            effective.model,
            if env_model {
                "ANTHROPIC_MODEL"
            } else {
                "file/default"
            }
        ),
        format!("verbose: {}", on_off(effective.settings.verbose)),
        format!("brief: {}", on_off(effective.settings.brief)),
        format!("vim_mode: {}", on_off(effective.settings.vim_mode)),
        format!(
            "theme: {}",
            effective.settings.theme.as_deref().unwrap_or("system")
        ),
        format!(
            "advisor_model: {}",
            effective.settings.advisor_model.as_deref().unwrap_or("off")
        ),
        format!(
            "output_style: {}",
            effective
                .settings
                .output_style
                .as_deref()
                .unwrap_or("default")
        ),
        format!("sandbox: {}", bool_label(effective.sandbox.is_some())),
        "usage: kiana config init | get <key> | set <key> <value> | unset <key>".to_string(),
    ]
    .join("\n")
}

fn init_config() -> Result<CommandResult> {
    let path = config_path();
    let config = load_user_config();
    let existed = path.is_file();
    let path = save_user_config(&config)?;
    let status = if existed {
        "Config already exists"
    } else {
        "Config initialized"
    };
    Ok(CommandResult::text(format!(
        "{status}\nfile: {}\nmodel: {}\napi_key: {}\nnext: kiana login <api-key>\nor: kiana config set api_key <api-key>\ncheck: kiana config status",
        path.display(),
        config.model,
        format_config_value(&config, ConfigKey::ApiKey),
    )))
}

fn get_config_value(key: Option<&String>, extra: &[String]) -> Result<CommandResult> {
    reject_extra_args("get", extra)?;
    let key = parse_key_arg(key)?;
    let config = kiana_bootstrap::config::load_config();
    Ok(CommandResult::text(format!(
        "{}: {}",
        key.name(),
        format_config_value(&config, key)
    )))
}

fn set_config_value(key: Option<&String>, value_parts: &[String]) -> Result<CommandResult> {
    let key = parse_key_arg(key)?;
    let value = value_parts.join(" ");
    if value.trim().is_empty() {
        return Err(anyhow!("missing value for '{}'\n\n{}", key.name(), usage()));
    }

    let mut config = load_user_config();
    apply_set(&mut config, key, value.trim())?;
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Config updated\n{}: {}\nfile: {}",
        key.name(),
        format_config_value(&config, key),
        path.display()
    )))
}

fn unset_config_value(
    command: &str,
    key: Option<&String>,
    extra: &[String],
) -> Result<CommandResult> {
    reject_extra_args(command, extra)?;
    let key = parse_key_arg(key)?;
    let mut config = load_user_config();
    apply_unset(&mut config, key);
    let path = save_user_config(&config)?;
    Ok(CommandResult::text(format!(
        "Config reset\n{}: {}\nfile: {}",
        key.name(),
        format_config_value(&config, key),
        path.display()
    )))
}

fn reject_extra_args(command: &str, extra: &[String]) -> Result<()> {
    if extra.is_empty() {
        return Ok(());
    }
    Err(anyhow!(
        "unknown config {} arguments '{}'\n\n{}",
        command,
        extra.join(" "),
        usage()
    ))
}

fn parse_args(args: &str) -> Vec<String> {
    args.split_whitespace().map(str::to_string).collect()
}

fn parse_key_arg(key: Option<&String>) -> Result<ConfigKey> {
    let key = key
        .map(String::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| anyhow!("missing config key\n\n{}", usage()))?;
    parse_key(key)
}

fn parse_key(key: &str) -> Result<ConfigKey> {
    match key {
        "api_key" | "apiKey" | "api-key" | "anthropic_api_key" => Ok(ConfigKey::ApiKey),
        "base_url" | "baseUrl" | "base-url" | "anthropic_base_url" => Ok(ConfigKey::BaseUrl),
        "model" | "anthropic_model" => Ok(ConfigKey::Model),
        "verbose" | "settings.verbose" => Ok(ConfigKey::Verbose),
        "brief" | "settings.brief" => Ok(ConfigKey::Brief),
        "vim_mode" | "vimMode" | "vim-mode" | "settings.vim_mode" | "settings.vimMode" => {
            Ok(ConfigKey::VimMode)
        }
        "theme" | "settings.theme" => Ok(ConfigKey::Theme),
        "advisor_model"
        | "advisorModel"
        | "advisor-model"
        | "settings.advisor_model"
        | "settings.advisorModel" => Ok(ConfigKey::AdvisorModel),
        "output_style"
        | "outputStyle"
        | "output-style"
        | "settings.output_style"
        | "settings.outputStyle" => Ok(ConfigKey::OutputStyle),
        "sandbox" => Ok(ConfigKey::Sandbox),
        _ => Err(anyhow!("unknown config key '{}'\n\n{}", key, usage())),
    }
}

fn apply_set(config: &mut Config, key: ConfigKey, value: &str) -> Result<()> {
    match key {
        ConfigKey::ApiKey => config.api_key = Some(value.to_string()),
        ConfigKey::BaseUrl => config.base_url = Some(value.to_string()),
        ConfigKey::Model => config.model = value.to_string(),
        ConfigKey::Verbose => config.settings.verbose = parse_bool(value)?,
        ConfigKey::Brief => config.settings.brief = parse_bool(value)?,
        ConfigKey::VimMode => config.settings.vim_mode = parse_bool(value)?,
        ConfigKey::Theme => {
            config.settings.theme = match value.to_ascii_lowercase().as_str() {
                "system" | "auto" | "default" => None,
                "dark" | "light" => Some(value.to_ascii_lowercase()),
                _ => Some(value.to_string()),
            };
        }
        ConfigKey::AdvisorModel => {
            config.settings.advisor_model = if matches!(
                value.to_ascii_lowercase().as_str(),
                "off" | "none" | "disable" | "disabled"
            ) {
                None
            } else {
                Some(value.to_string())
            };
        }
        ConfigKey::OutputStyle => {
            config.settings.output_style = if matches!(
                value.to_ascii_lowercase().as_str(),
                "default" | "none" | "off" | "reset"
            ) {
                None
            } else {
                Some(value.to_string())
            };
        }
        ConfigKey::Sandbox => {
            let parsed: Value = serde_json::from_str(value)
                .map_err(|error| anyhow!("sandbox must be valid JSON: {}", error))?;
            if !parsed.is_object() {
                return Err(anyhow!("sandbox must be a JSON object"));
            }
            config.sandbox = Some(parsed);
        }
    }
    Ok(())
}

fn apply_unset(config: &mut Config, key: ConfigKey) {
    match key {
        ConfigKey::ApiKey => config.api_key = None,
        ConfigKey::BaseUrl => config.base_url = None,
        ConfigKey::Model => config.model = Config::default().model,
        ConfigKey::Verbose => config.settings.verbose = false,
        ConfigKey::Brief => config.settings.brief = false,
        ConfigKey::VimMode => config.settings.vim_mode = false,
        ConfigKey::Theme => config.settings.theme = None,
        ConfigKey::AdvisorModel => config.settings.advisor_model = None,
        ConfigKey::OutputStyle => config.settings.output_style = None,
        ConfigKey::Sandbox => config.sandbox = None,
    }
}

fn parse_bool(value: &str) -> Result<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Ok(true),
        "false" | "off" | "no" | "0" => Ok(false),
        _ => Err(anyhow!(
            "expected boolean value: on/off, true/false, yes/no, or 1/0"
        )),
    }
}

fn format_config_value(config: &Config, key: ConfigKey) -> String {
    match key {
        ConfigKey::ApiKey => config
            .api_key
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(mask_secret)
            .unwrap_or_else(|| "missing".to_string()),
        ConfigKey::BaseUrl => config
            .base_url
            .as_deref()
            .unwrap_or("https://api.anthropic.com")
            .to_string(),
        ConfigKey::Model => config.model.clone(),
        ConfigKey::Verbose => on_off(config.settings.verbose).to_string(),
        ConfigKey::Brief => on_off(config.settings.brief).to_string(),
        ConfigKey::VimMode => on_off(config.settings.vim_mode).to_string(),
        ConfigKey::Theme => config
            .settings
            .theme
            .as_deref()
            .unwrap_or("system")
            .to_string(),
        ConfigKey::AdvisorModel => config
            .settings
            .advisor_model
            .as_deref()
            .unwrap_or("off")
            .to_string(),
        ConfigKey::OutputStyle => config
            .settings
            .output_style
            .as_deref()
            .unwrap_or("default")
            .to_string(),
        ConfigKey::Sandbox => config
            .sandbox
            .as_ref()
            .map(Value::to_string)
            .unwrap_or_else(|| "unset".to_string()),
    }
}

fn redacted_config_json(config: &Config) -> Value {
    json!({
        "api_key": config.api_key.as_deref().map(mask_secret),
        "base_url": config.base_url.clone(),
        "model": config.model.clone(),
        "settings": {
            "verbose": config.settings.verbose,
            "brief": config.settings.brief,
            "vim_mode": config.settings.vim_mode,
            "theme": config.settings.theme.clone(),
            "advisor_model": config.settings.advisor_model.clone(),
            "output_style": config.settings.output_style.clone(),
        },
        "sandbox": config.sandbox.clone(),
    })
}

#[derive(Debug, Clone)]
struct ResolvedField {
    value: Value,
    source: &'static str,
}

impl ResolvedField {
    fn new(value: Value, source: &'static str) -> Self {
        Self { value, source }
    }

    fn json(self) -> Value {
        json!({
            "value": self.value,
            "source": self.source,
        })
    }
}

#[derive(Debug, Clone)]
struct ResolvedConfigSummary {
    api_key: ResolvedField,
    base_url: ResolvedField,
    model: ResolvedField,
    verbose: ResolvedField,
    brief: ResolvedField,
    vim_mode: ResolvedField,
    theme: ResolvedField,
    advisor_model: ResolvedField,
    output_style: ResolvedField,
    sandbox: ResolvedField,
}

fn resolved_config(extra: &[String]) -> Result<CommandResult> {
    if !extra.is_empty() && extra != ["--json"] {
        return Err(anyhow!(
            "unknown config resolved arguments '{}'\n\n{}",
            extra.join(" "),
            usage()
        ));
    }
    let summary = resolved_config_summary();
    Ok(CommandResult::text(serde_json::to_string_pretty(
        &resolved_config_json(summary),
    )?))
}

fn resolved_config_json(summary: ResolvedConfigSummary) -> Value {
    let path = config_path();
    json!({
        "schema": "kiana.config-resolved.v1",
        "config_file": {
            "path": path.to_string_lossy(),
            "status": if path.is_file() { "found" } else { "missing" },
        },
        "sources": resolved_config_sources(),
        "values": {
            "api_key": summary.api_key.json(),
            "base_url": summary.base_url.json(),
            "model": summary.model.json(),
            "settings": {
                "verbose": summary.verbose.json(),
                "brief": summary.brief.json(),
                "vim_mode": summary.vim_mode.json(),
                "theme": summary.theme.json(),
                "advisor_model": summary.advisor_model.json(),
                "output_style": summary.output_style.json(),
            },
            "sandbox": summary.sandbox.json(),
        }
    })
}

fn resolved_config_sources() -> Value {
    let path = config_path();
    json!({
        "config_file": {
            "path": path.to_string_lossy(),
            "status": if path.is_file() { "found" } else { "missing" },
        },
        "remote_settings_file": env_path_source("KIANA_REMOTE_SETTINGS_FILE"),
        "settings_file": env_path_source("KIANA_SETTINGS_FILE"),
        "settings_json": {
            "status": if std::env::var("KIANA_SETTINGS_JSON").ok().is_some_and(|value| !value.trim().is_empty()) {
                "set"
            } else {
                "missing"
            },
        },
        "env": {
            "anthropic_api_key": env_value_status("ANTHROPIC_API_KEY"),
            "anthropic_base_url": env_value_status("ANTHROPIC_BASE_URL"),
            "anthropic_model": env_value_status("ANTHROPIC_MODEL"),
        },
        "managed_settings_file": managed_settings_source(),
    })
}

fn env_path_source(name: &str) -> Value {
    match std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        Some(path) => json!({
            "path": path,
            "status": if std::path::Path::new(&path).is_file() { "found" } else { "missing" },
        }),
        None => json!({
            "path": Value::Null,
            "status": "missing",
        }),
    }
}

fn env_value_status(name: &str) -> Value {
    json!({
        "status": if std::env::var(name).ok().is_some_and(|value| !value.trim().is_empty()) {
            "set"
        } else {
            "missing"
        }
    })
}

fn managed_settings_source() -> Value {
    let explicit = std::env::var("KIANA_MANAGED_SETTINGS_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let shared = std::env::var("KIANA_MANAGED_POLICY_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty());
    match explicit.or(shared) {
        Some(path) => json!({
            "path": path,
            "status": if std::path::Path::new(&path).is_file() { "found" } else { "missing" },
        }),
        None => json!({
            "path": Value::Null,
            "status": "missing",
        }),
    }
}

fn resolved_config_summary() -> ResolvedConfigSummary {
    let effective = kiana_bootstrap::config::load_config();
    let file = load_user_config();

    ResolvedConfigSummary {
        api_key: ResolvedField::new(
            effective
                .api_key
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .map(mask_secret)
                .map(Value::String)
                .unwrap_or(Value::Null),
            resolved_api_key_source(&file),
        ),
        base_url: ResolvedField::new(
            effective
                .base_url
                .as_deref()
                .unwrap_or("https://api.anthropic.com")
                .to_string()
                .into(),
            resolved_base_url_source(&file),
        ),
        model: ResolvedField::new(effective.model.clone().into(), resolved_model_source(&file)),
        verbose: ResolvedField::new(
            effective.settings.verbose.into(),
            resolved_settings_bool_source(
                "verbose",
                file.settings.verbose,
                Config::default().settings.verbose,
            ),
        ),
        brief: ResolvedField::new(
            effective.settings.brief.into(),
            resolved_settings_bool_source(
                "brief",
                file.settings.brief,
                Config::default().settings.brief,
            ),
        ),
        vim_mode: ResolvedField::new(
            effective.settings.vim_mode.into(),
            resolved_settings_bool_source(
                "vim_mode",
                file.settings.vim_mode,
                Config::default().settings.vim_mode,
            ),
        ),
        theme: ResolvedField::new(
            effective
                .settings
                .theme
                .as_deref()
                .unwrap_or("system")
                .to_string()
                .into(),
            resolved_settings_string_source("theme", file.settings.theme.as_deref(), None),
        ),
        advisor_model: ResolvedField::new(
            effective
                .settings
                .advisor_model
                .as_deref()
                .unwrap_or("off")
                .to_string()
                .into(),
            resolved_settings_string_source(
                "advisor_model",
                file.settings.advisor_model.as_deref(),
                None,
            ),
        ),
        output_style: ResolvedField::new(
            effective
                .settings
                .output_style
                .as_deref()
                .unwrap_or("default")
                .to_string()
                .into(),
            resolved_settings_string_source(
                "output_style",
                file.settings.output_style.as_deref(),
                None,
            ),
        ),
        sandbox: ResolvedField::new(
            effective.sandbox.clone().unwrap_or(Value::Null),
            resolved_sandbox_source(file.sandbox.as_ref()),
        ),
    }
}

fn resolved_api_key_source(file: &Config) -> &'static str {
    if overlay_has_non_empty_string(managed_settings_path().as_ref(), &["apiKey", "api_key"]) {
        "managed_settings_file"
    } else if env_non_empty("ANTHROPIC_API_KEY") {
        "env"
    } else if overlay_has_non_empty_string(settings_json_value().as_ref(), &["apiKey", "api_key"]) {
        "settings_json"
    } else if overlay_has_non_empty_string(settings_file_path().as_ref(), &["apiKey", "api_key"]) {
        "settings_file"
    } else if overlay_has_non_empty_string(remote_settings_path().as_ref(), &["apiKey", "api_key"])
    {
        "remote_settings_file"
    } else if file
        .api_key
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        "config_file"
    } else {
        "default"
    }
}

fn resolved_base_url_source(file: &Config) -> &'static str {
    if overlay_has_non_empty_string(managed_settings_path().as_ref(), &["baseUrl", "base_url"]) {
        "managed_settings_file"
    } else if env_non_empty("ANTHROPIC_BASE_URL") {
        "env"
    } else if overlay_has_non_empty_string(settings_json_value().as_ref(), &["baseUrl", "base_url"])
    {
        "settings_json"
    } else if overlay_has_non_empty_string(settings_file_path().as_ref(), &["baseUrl", "base_url"])
    {
        "settings_file"
    } else if overlay_has_non_empty_string(
        remote_settings_path().as_ref(),
        &["baseUrl", "base_url"],
    ) {
        "remote_settings_file"
    } else if file.base_url.is_some() {
        "config_file"
    } else {
        "default"
    }
}

fn resolved_model_source(file: &Config) -> &'static str {
    if overlay_has_non_empty_string(managed_settings_path().as_ref(), &["model"]) {
        "managed_settings_file"
    } else if env_non_empty("ANTHROPIC_MODEL") {
        "env"
    } else if overlay_has_non_empty_string(settings_json_value().as_ref(), &["model"]) {
        "settings_json"
    } else if overlay_has_non_empty_string(settings_file_path().as_ref(), &["model"]) {
        "settings_file"
    } else if overlay_has_non_empty_string(remote_settings_path().as_ref(), &["model"]) {
        "remote_settings_file"
    } else if file.model != Config::default().model {
        "config_file"
    } else {
        "default"
    }
}

fn resolved_settings_bool_source(key: &str, file_value: bool, default_value: bool) -> &'static str {
    if overlay_settings_has_key(managed_settings_path().as_ref(), key) {
        "managed_settings_file"
    } else if overlay_settings_has_key(settings_json_value().as_ref(), key) {
        "settings_json"
    } else if overlay_settings_has_key(settings_file_path().as_ref(), key) {
        "settings_file"
    } else if overlay_settings_has_key(remote_settings_path().as_ref(), key) {
        "remote_settings_file"
    } else if file_value != default_value {
        "config_file"
    } else {
        "default"
    }
}

fn resolved_settings_string_source(
    key: &str,
    file_value: Option<&str>,
    default_value: Option<&str>,
) -> &'static str {
    if overlay_settings_has_key(managed_settings_path().as_ref(), key) {
        "managed_settings_file"
    } else if overlay_settings_has_key(settings_json_value().as_ref(), key) {
        "settings_json"
    } else if overlay_settings_has_key(settings_file_path().as_ref(), key) {
        "settings_file"
    } else if overlay_settings_has_key(remote_settings_path().as_ref(), key) {
        "remote_settings_file"
    } else if file_value != default_value {
        "config_file"
    } else {
        "default"
    }
}

fn resolved_sandbox_source(file_sandbox: Option<&Value>) -> &'static str {
    if overlay_has_key(managed_settings_path().as_ref(), &["sandbox"]) {
        "managed_settings_file"
    } else if overlay_has_key(settings_json_value().as_ref(), &["sandbox"]) {
        "settings_json"
    } else if overlay_has_key(settings_file_path().as_ref(), &["sandbox"]) {
        "settings_file"
    } else if overlay_has_key(remote_settings_path().as_ref(), &["sandbox"]) {
        "remote_settings_file"
    } else if file_sandbox.is_some() {
        "config_file"
    } else {
        "default"
    }
}

fn env_non_empty(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .is_some_and(|value| !value.trim().is_empty())
}

fn remote_settings_path() -> Option<Value> {
    env_json_or_toml_path("KIANA_REMOTE_SETTINGS_FILE")
}

fn settings_file_path() -> Option<Value> {
    env_json_or_toml_path("KIANA_SETTINGS_FILE")
}

fn managed_settings_path() -> Option<Value> {
    std::env::var("KIANA_MANAGED_SETTINGS_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("KIANA_MANAGED_POLICY_FILE")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .and_then(|path| config_overlay_value_from_file(std::path::Path::new(&path)))
}

fn env_json_or_toml_path(name: &str) -> Option<Value> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .and_then(|path| config_overlay_value_from_file(std::path::Path::new(&path)))
}

fn settings_json_value() -> Option<Value> {
    std::env::var("KIANA_SETTINGS_JSON")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .and_then(|value| serde_json::from_str(&value).ok())
}

fn config_overlay_value_from_file(path: &std::path::Path) -> Option<Value> {
    let contents = std::fs::read_to_string(path).ok()?;
    if path.extension().and_then(|ext| ext.to_str()) == Some("json")
        || contents.trim_start().starts_with('{')
    {
        serde_json::from_str(&contents).ok()
    } else {
        let toml_value: toml::Value = toml::from_str(&contents).ok()?;
        serde_json::to_value(toml_value).ok()
    }
}

fn overlay_has_non_empty_string(value: Option<&Value>, aliases: &[&str]) -> bool {
    aliases.iter().any(|alias| {
        value
            .and_then(|value| value.get(*alias))
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty())
    })
}

fn overlay_has_key(value: Option<&Value>, aliases: &[&str]) -> bool {
    aliases
        .iter()
        .any(|alias| value.and_then(|value| value.get(*alias)).is_some())
}

fn overlay_settings_has_key(value: Option<&Value>, key: &str) -> bool {
    let aliases = settings_aliases(key);
    aliases.iter().any(|alias| {
        value
            .and_then(|value| value.get("settings"))
            .and_then(|settings| settings.get(*alias))
            .is_some()
    })
}

fn settings_aliases(key: &str) -> &'static [&'static str] {
    match key {
        "vim_mode" => &["vimMode", "vim_mode"],
        "advisor_model" => &["advisorModel", "advisor_model"],
        "output_style" => &["outputStyle", "output_style"],
        other => {
            if other == "verbose" {
                &["verbose"]
            } else if other == "brief" {
                &["brief"]
            } else if other == "theme" {
                &["theme"]
            } else {
                &[]
            }
        }
    }
}

fn mask_secret(secret: &str) -> String {
    let chars: Vec<char> = secret.chars().collect();
    if chars.len() <= 8 {
        return "****".to_string();
    }
    let start: String = chars.iter().take(4).collect();
    let end: String = chars
        .iter()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("{start}...{end}")
}

fn usage() -> &'static str {
    "Usage:\n  kiana config [status|list]\n  kiana config init\n  kiana config path\n  kiana config json\n  kiana config resolved --json\n  kiana config get <api_key|base_url|model|verbose|brief|vim_mode|theme|advisor_model|output_style|sandbox>\n  kiana config set <key> <value>\n  kiana config unset <key>"
}

#[cfg(test)]
mod tests {
    use super::ConfigCommand;
    use crate::local_state::env_lock;
    use crate::{Command, CommandContext};
    use serde_json::Value;
    use std::collections::HashMap;
    use std::fs;
    use std::sync::{MutexGuard, PoisonError};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path() -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "kiana-config-command-{}-{unique}.toml",
            std::process::id()
        ))
    }

    fn context(args: &str) -> CommandContext {
        CommandContext {
            args: args.to_string(),
            app_state: HashMap::<String, Value>::new(),
        }
    }

    fn lock_env() -> MutexGuard<'static, ()> {
        env_lock().lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[tokio::test]
    async fn config_set_get_and_unset_persist_values() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        std::env::remove_var("ANTHROPIC_MODEL");

        let command = ConfigCommand;
        let set = command
            .execute(context("set model claude-opus-4-1"))
            .await
            .unwrap();
        assert!(set.value.contains("model: claude-opus-4-1"));

        let get = command.execute(context("get model")).await.unwrap();
        assert!(get.value.contains("model: claude-opus-4-1"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("model = \"claude-opus-4-1\""));

        let unset = command.execute(context("unset model")).await.unwrap();
        assert!(unset.value.contains("model: claude-sonnet-4-6"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_init_creates_starter_config_with_next_steps() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ANTHROPIC_BASE_URL");
        std::env::remove_var("ANTHROPIC_MODEL");
        let _ = fs::remove_file(&path);

        let result = ConfigCommand.execute(context("init")).await.unwrap();

        assert!(path.is_file());
        let contents = fs::read_to_string(&path).unwrap();
        assert!(contents.contains("model = \"claude-sonnet-4-6\""));
        assert!(result.value.contains("Config initialized"));
        assert!(result.value.contains(&format!("file: {}", path.display())));
        assert!(result.value.contains("next: kiana login <api-key>"));
        assert!(result
            .value
            .contains("or: kiana config set api_key <api-key>"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_set_boolean_and_redacts_api_key_json() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        std::env::remove_var("ANTHROPIC_API_KEY");

        let command = ConfigCommand;
        command.execute(context("set brief on")).await.unwrap();
        command
            .execute(context("set api_key sk-ant-1234567890"))
            .await
            .unwrap();

        let status = command.execute(context("status")).await.unwrap();
        assert!(status.value.contains("brief: on"));
        assert!(status.value.contains("api_key: set (file)"));

        let json = command.execute(context("json")).await.unwrap();
        assert!(json.value.contains("sk-a...7890"));
        assert!(!json.value.contains("sk-ant-1234567890"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_rejects_invalid_boolean_and_sandbox_json() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);

        let command = ConfigCommand;
        let bool_error = command
            .execute(context("set brief maybe"))
            .await
            .unwrap_err()
            .to_string();
        assert!(bool_error.contains("expected boolean value"));

        let sandbox_error = command
            .execute(context("set sandbox true"))
            .await
            .unwrap_err()
            .to_string();
        assert!(sandbox_error.contains("sandbox must be a JSON object"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_unset_rejects_extra_args_without_resetting_value() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        std::env::remove_var("ANTHROPIC_MODEL");

        let command = ConfigCommand;
        command
            .execute(context("set model claude-opus-4-1"))
            .await
            .unwrap();

        let error = command
            .execute(context("unset model please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown config unset arguments"));
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("model = \"claude-opus-4-1\""));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_init_rejects_extra_args_without_creating_file() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        let _ = fs::remove_file(&path);

        let error = ConfigCommand
            .execute(context("init extra"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown config init arguments"));
        assert!(!path.exists());

        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_read_only_subcommands_reject_extra_args() {
        let _guard = lock_env();
        let path = temp_config_path();
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        let _ = fs::remove_file(&path);

        for args in [
            "status now",
            "list now",
            "path extra",
            "json api_key",
            "help now",
            "get model please",
        ] {
            let error = ConfigCommand
                .execute(context(args))
                .await
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("unknown config"),
                "{args} returned wrong error: {error}"
            );
        }
        assert!(!path.exists());

        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn config_resolved_json_reports_effective_values_sources_and_redacts_secrets() {
        let _guard = lock_env();
        let path = temp_config_path();
        let remote = temp_config_path().with_extension("remote.json");
        let settings = temp_config_path().with_extension("settings.json");
        let managed = temp_config_path().with_extension("managed.json");
        std::env::set_var("KIANA_CONFIG_FILE", &path);
        std::env::set_var("KIANA_REMOTE_SETTINGS_FILE", &remote);
        std::env::set_var("KIANA_SETTINGS_FILE", &settings);
        std::env::set_var("KIANA_SETTINGS_JSON", r#"{"settings":{"brief":true}}"#);
        std::env::set_var("ANTHROPIC_API_KEY", "env-secret-1234567890");
        std::env::set_var("ANTHROPIC_BASE_URL", "https://env.example.test");
        std::env::set_var("ANTHROPIC_MODEL", "env-model");
        std::env::set_var("KIANA_MANAGED_SETTINGS_FILE", &managed);

        fs::write(
            &path,
            r#"
api_key = "file-secret-0000"
base_url = "https://file.example.test"
model = "file-model"

[settings]
theme = "file-theme"
"#,
        )
        .unwrap();
        fs::write(
            &remote,
            r#"{"model":"remote-model","settings":{"theme":"remote-theme"}}"#,
        )
        .unwrap();
        fs::write(
            &settings,
            r#"{"baseUrl":"settings.example.test","settings":{"outputStyle":"settings-style"}}"#,
        )
        .unwrap();
        fs::write(
            &managed,
            r#"{"apiKey":"managed-secret-abcdef","model":"managed-model","settings":{"theme":"managed-theme"}}"#,
        )
        .unwrap();

        let result = ConfigCommand
            .execute(context("resolved --json"))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();

        assert_eq!(value["schema"], "kiana.config-resolved.v1");
        assert_eq!(
            value["config_file"]["path"].as_str(),
            Some(path.to_string_lossy().as_ref())
        );
        assert_eq!(value["config_file"]["status"], "found");
        assert_eq!(value["values"]["api_key"]["value"], "mana...cdef");
        assert_eq!(
            value["values"]["api_key"]["source"],
            "managed_settings_file"
        );
        assert_eq!(value["values"]["model"]["value"], "managed-model");
        assert_eq!(value["values"]["model"]["source"], "managed_settings_file");
        assert_eq!(
            value["values"]["base_url"]["value"],
            "https://env.example.test"
        );
        assert_eq!(value["values"]["base_url"]["source"], "env");
        assert_eq!(
            value["values"]["settings"]["theme"]["value"],
            "managed-theme"
        );
        assert_eq!(
            value["values"]["settings"]["theme"]["source"],
            "managed_settings_file"
        );
        assert_eq!(value["values"]["settings"]["brief"]["value"], true);
        assert_eq!(
            value["values"]["settings"]["brief"]["source"],
            "settings_json"
        );
        assert_eq!(
            value["sources"]["managed_settings_file"]["path"].as_str(),
            Some(managed.to_string_lossy().as_ref())
        );
        assert!(!result.value.contains("managed-secret-abcdef"));
        assert!(!result.value.contains("env-secret-1234567890"));
        assert!(!result.value.contains("file-secret-0000"));

        let _ = fs::remove_file(path);
        let _ = fs::remove_file(remote);
        let _ = fs::remove_file(settings);
        let _ = fs::remove_file(managed);
        std::env::remove_var("KIANA_CONFIG_FILE");
        std::env::remove_var("KIANA_REMOTE_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_FILE");
        std::env::remove_var("KIANA_SETTINGS_JSON");
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::remove_var("ANTHROPIC_BASE_URL");
        std::env::remove_var("ANTHROPIC_MODEL");
        std::env::remove_var("KIANA_MANAGED_SETTINGS_FILE");
    }
}
