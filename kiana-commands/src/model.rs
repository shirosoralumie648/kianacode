use crate::local_state::{config_path, load_user_config, save_user_config};
use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::anyhow;
use async_trait::async_trait;
use kiana_services::api::{
    messages::{Message, MessagesRequest},
    provider::{
        provider_registry_entry, AnthropicProvider, FakeProvider, FakeProviderStep, OllamaProvider,
        OpenAiCompatibleProvider, Provider, ProviderRegistryEntry, ANTHROPIC_PROVIDER_ID,
        FAKE_MODEL_ID, FAKE_PROVIDER_ID, OLLAMA_PROVIDER_ID, OPENAI_COMPATIBLE_PROVIDER_ID,
    },
};
use serde::Serialize;
use serde_json::{json, Value};
use std::time::Duration;

pub struct ModelCommand;

#[async_trait]
impl Command for ModelCommand {
    fn name(&self) -> &str {
        "model"
    }

    fn description(&self) -> &str {
        "Configure model"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let arg = context.args.trim();
        let mut parts = arg.splitn(2, char::is_whitespace);
        let head = parts.next().unwrap_or_default();
        let rest = parts.next().unwrap_or_default().trim();

        match head {
            "" => return Ok(CommandResult::text(model_status())),
            "status" if rest.is_empty() => return Ok(CommandResult::text(model_status())),
            "status" => return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage())),
            "list" if rest.is_empty() => return Ok(CommandResult::text(model_list_text())),
            "list" if rest == "--json" => return Ok(CommandResult::text(model_list_json()?)),
            "list" => return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage())),
            "smoke" => return model_smoke_command(rest).await,
            "help" | "--help" | "-h" if rest.is_empty() => return Ok(CommandResult::text(usage())),
            "help" | "--help" | "-h" => {
                return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage()))
            }
            "reset" | "default" if rest.is_empty() => {
                return save_model(kiana_bootstrap::config::Config::default().model)
            }
            "reset" | "default" => {
                return Err(anyhow!("unknown model command '{}'\n\n{}", arg, usage()))
            }
            other if other.starts_with('-') => {
                return Err(anyhow!("unknown model option '{}'\n\n{}", other, usage()))
            }
            _ if !rest.is_empty() => {
                return Err(anyhow!(
                    "model name cannot contain whitespace\n\n{}",
                    usage()
                ))
            }
            _ => {}
        }

        save_model(arg.to_string())
    }
}

fn model_status() -> String {
    let config = kiana_bootstrap::config::load_config();
    format!(
        "Model status\nmodel: {}\nconfig_file: {}\nusage: kiana model <model-name>\nlist: kiana model list --json",
        config.model,
        config_path().display()
    )
}

fn model_list_json() -> anyhow::Result<String> {
    Ok(serde_json::to_string_pretty(
        &kiana_services::api::provider::built_in_model_profiles(),
    )?)
}

fn model_list_text() -> String {
    let mut lines = vec![
        "Available model profiles".to_string(),
        "provider\tprovider_name\tmodel\ttools\tstreaming\tstreaming_mode\tnative_streaming\tvision\tstructured_output\tcontext_window".to_string(),
    ];
    for profile in kiana_services::api::provider::built_in_model_profiles() {
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            profile.provider_id,
            profile.provider_display_name,
            profile.model_id,
            profile.supports_tools,
            profile.supports_streaming,
            profile.streaming_mode.as_str(),
            profile.native_streaming,
            profile.supports_vision,
            profile.supports_structured_output,
            profile.context_window
        ));
    }
    lines.join("\n")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ModelSmokeOptions {
    json: bool,
    live: bool,
    tools: bool,
}

#[derive(Debug, Serialize)]
struct ModelSmokeReport {
    schema: &'static str,
    live: bool,
    tools: bool,
    summary: ModelSmokeSummary,
    results: Vec<ModelSmokeResult>,
}

#[derive(Debug, Serialize)]
struct ModelSmokeSummary {
    passed: usize,
    skipped: usize,
    failed: usize,
}

#[derive(Debug, Serialize)]
struct ModelSmokeResult {
    provider_id: String,
    model_id: String,
    status: String,
    live: bool,
    capability: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_preview: Option<String>,
}

async fn model_smoke_command(args: &str) -> anyhow::Result<CommandResult> {
    let options = parse_model_smoke_options(args)?;
    let report = model_smoke_report(options.live, options.tools).await;
    if options.json {
        return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
    }
    Ok(CommandResult::text(model_smoke_text(&report)))
}

fn parse_model_smoke_options(args: &str) -> anyhow::Result<ModelSmokeOptions> {
    let mut options = ModelSmokeOptions {
        json: false,
        live: env_flag("KIANA_PROVIDER_SMOKE_LIVE"),
        tools: env_flag("KIANA_PROVIDER_SMOKE_TOOLS"),
    };
    for token in args.split_whitespace() {
        match token {
            "--json" => options.json = true,
            "--live" => options.live = true,
            "--tools" => options.tools = true,
            "" => {}
            other => {
                return Err(anyhow!(
                    "unknown model smoke option '{}'\n\n{}",
                    other,
                    usage()
                ))
            }
        }
    }
    Ok(options)
}

async fn model_smoke_report(live: bool, tools: bool) -> ModelSmokeReport {
    let mut results = Vec::new();
    results.push(smoke_fake_provider().await);
    results.push(smoke_anthropic_provider(live).await);
    results.push(smoke_openai_compatible_provider(live).await);
    results.push(smoke_ollama_provider(live).await);
    if tools {
        results.push(smoke_fake_provider_tools().await);
        results.push(smoke_anthropic_provider_tools(live).await);
        results.push(smoke_openai_compatible_provider_tools(live).await);
        results.push(smoke_ollama_provider_tools(live).await);
    }
    let summary = ModelSmokeSummary {
        passed: results
            .iter()
            .filter(|result| result.status == "passed")
            .count(),
        skipped: results
            .iter()
            .filter(|result| result.status == "skipped")
            .count(),
        failed: results
            .iter()
            .filter(|result| result.status == "failed")
            .count(),
    };
    ModelSmokeReport {
        schema: "kiana.model-smoke.v1",
        live,
        tools,
        summary,
        results,
    }
}

async fn smoke_fake_provider() -> ModelSmokeResult {
    let provider = FakeProvider::new(
        FAKE_MODEL_ID.to_string(),
        vec![FakeProviderStep::AssistantText {
            text: "kiana-provider-smoke".to_string(),
        }],
    );
    run_provider_smoke(FAKE_PROVIDER_ID, FAKE_MODEL_ID, false, Box::new(provider)).await
}

async fn smoke_fake_provider_tools() -> ModelSmokeResult {
    let provider = FakeProvider::new(
        FAKE_MODEL_ID.to_string(),
        vec![FakeProviderStep::ToolCall {
            id: Some("toolu_provider_smoke".to_string()),
            name: "ProviderSmoke".to_string(),
            input: json!({ "status": "ok" }),
            text: None,
        }],
    );
    run_provider_tool_smoke(FAKE_PROVIDER_ID, FAKE_MODEL_ID, false, Box::new(provider)).await
}

async fn smoke_anthropic_provider(live: bool) -> ModelSmokeResult {
    let model_id = provider_model_id(ANTHROPIC_PROVIDER_ID);
    if !live {
        return skipped_provider_smoke(
            ANTHROPIC_PROVIDER_ID,
            &model_id,
            false,
            "text",
            "live provider smoke disabled; pass --live or set KIANA_PROVIDER_SMOKE_LIVE=1",
        );
    }
    let Some(api_key) = provider_api_key(ANTHROPIC_PROVIDER_ID) else {
        return skipped_provider_smoke(
            ANTHROPIC_PROVIDER_ID,
            &model_id,
            true,
            "text",
            &missing_api_key_message(ANTHROPIC_PROVIDER_ID),
        );
    };
    let base_url = provider_base_url(ANTHROPIC_PROVIDER_ID).unwrap_or_default();
    run_provider_smoke(
        ANTHROPIC_PROVIDER_ID,
        &model_id,
        true,
        Box::new(AnthropicProvider::new(
            api_key,
            base_url,
            Duration::from_secs(30),
        )),
    )
    .await
}

async fn smoke_anthropic_provider_tools(live: bool) -> ModelSmokeResult {
    let model_id = provider_model_id(ANTHROPIC_PROVIDER_ID);
    if !live {
        return skipped_provider_smoke(
            ANTHROPIC_PROVIDER_ID,
            &model_id,
            false,
            "tools",
            "live provider tool smoke disabled; pass --live --tools or set KIANA_PROVIDER_SMOKE_LIVE=1 and KIANA_PROVIDER_SMOKE_TOOLS=1",
        );
    }
    let Some(api_key) = provider_api_key(ANTHROPIC_PROVIDER_ID) else {
        return skipped_provider_smoke(
            ANTHROPIC_PROVIDER_ID,
            &model_id,
            true,
            "tools",
            &missing_api_key_message(ANTHROPIC_PROVIDER_ID),
        );
    };
    let base_url = provider_base_url(ANTHROPIC_PROVIDER_ID).unwrap_or_default();
    run_provider_tool_smoke(
        ANTHROPIC_PROVIDER_ID,
        &model_id,
        true,
        Box::new(AnthropicProvider::new(
            api_key,
            base_url,
            Duration::from_secs(30),
        )),
    )
    .await
}

async fn smoke_openai_compatible_provider(live: bool) -> ModelSmokeResult {
    let model_id = provider_model_id(OPENAI_COMPATIBLE_PROVIDER_ID);
    if !live {
        return skipped_provider_smoke(
            OPENAI_COMPATIBLE_PROVIDER_ID,
            &model_id,
            false,
            "text",
            "live provider smoke disabled; pass --live or set KIANA_PROVIDER_SMOKE_LIVE=1",
        );
    }
    let Some(api_key) = provider_api_key(OPENAI_COMPATIBLE_PROVIDER_ID) else {
        return skipped_provider_smoke(
            OPENAI_COMPATIBLE_PROVIDER_ID,
            &model_id,
            true,
            "text",
            &missing_api_key_message(OPENAI_COMPATIBLE_PROVIDER_ID),
        );
    };
    let base_url = provider_base_url(OPENAI_COMPATIBLE_PROVIDER_ID).unwrap_or_default();
    let provider = match OpenAiCompatibleProvider::new(api_key, base_url, Duration::from_secs(30)) {
        Ok(provider) => provider,
        Err(error) => {
            return failed_provider_smoke(
                OPENAI_COMPATIBLE_PROVIDER_ID,
                &model_id,
                true,
                "text",
                error,
            )
        }
    };
    run_provider_smoke(
        OPENAI_COMPATIBLE_PROVIDER_ID,
        &model_id,
        true,
        Box::new(provider),
    )
    .await
}

async fn smoke_openai_compatible_provider_tools(live: bool) -> ModelSmokeResult {
    let model_id = provider_model_id(OPENAI_COMPATIBLE_PROVIDER_ID);
    if !live {
        return skipped_provider_smoke(
            OPENAI_COMPATIBLE_PROVIDER_ID,
            &model_id,
            false,
            "tools",
            "live provider tool smoke disabled; pass --live --tools or set KIANA_PROVIDER_SMOKE_LIVE=1 and KIANA_PROVIDER_SMOKE_TOOLS=1",
        );
    }
    let Some(api_key) = provider_api_key(OPENAI_COMPATIBLE_PROVIDER_ID) else {
        return skipped_provider_smoke(
            OPENAI_COMPATIBLE_PROVIDER_ID,
            &model_id,
            true,
            "tools",
            &missing_api_key_message(OPENAI_COMPATIBLE_PROVIDER_ID),
        );
    };
    let base_url = provider_base_url(OPENAI_COMPATIBLE_PROVIDER_ID).unwrap_or_default();
    let provider = match OpenAiCompatibleProvider::new(api_key, base_url, Duration::from_secs(30)) {
        Ok(provider) => provider,
        Err(error) => {
            return failed_provider_smoke(
                OPENAI_COMPATIBLE_PROVIDER_ID,
                &model_id,
                true,
                "tools",
                error,
            )
        }
    };
    run_provider_tool_smoke(
        OPENAI_COMPATIBLE_PROVIDER_ID,
        &model_id,
        true,
        Box::new(provider),
    )
    .await
}

async fn smoke_ollama_provider(live: bool) -> ModelSmokeResult {
    let model_id = provider_model_id(OLLAMA_PROVIDER_ID);
    if !live {
        return skipped_provider_smoke(
            OLLAMA_PROVIDER_ID,
            &model_id,
            false,
            "text",
            "live provider smoke disabled; pass --live or set KIANA_PROVIDER_SMOKE_LIVE=1",
        );
    }
    let base_url = provider_base_url(OLLAMA_PROVIDER_ID).unwrap_or_default();
    let provider = match OllamaProvider::new(base_url, Duration::from_secs(30)) {
        Ok(provider) => provider,
        Err(error) => {
            return failed_provider_smoke(OLLAMA_PROVIDER_ID, &model_id, true, "text", error)
        }
    };
    run_provider_smoke(OLLAMA_PROVIDER_ID, &model_id, true, Box::new(provider)).await
}

async fn smoke_ollama_provider_tools(live: bool) -> ModelSmokeResult {
    let model_id = provider_model_id(OLLAMA_PROVIDER_ID);
    if !live {
        return skipped_provider_smoke(
            OLLAMA_PROVIDER_ID,
            &model_id,
            false,
            "tools",
            "live provider tool smoke disabled; pass --live --tools or set KIANA_PROVIDER_SMOKE_LIVE=1 and KIANA_PROVIDER_SMOKE_TOOLS=1",
        );
    }
    let base_url = provider_base_url(OLLAMA_PROVIDER_ID).unwrap_or_default();
    let provider = match OllamaProvider::new(base_url, Duration::from_secs(30)) {
        Ok(provider) => provider,
        Err(error) => {
            return failed_provider_smoke(OLLAMA_PROVIDER_ID, &model_id, true, "tools", error)
        }
    };
    run_provider_tool_smoke(OLLAMA_PROVIDER_ID, &model_id, true, Box::new(provider)).await
}

async fn run_provider_smoke(
    provider_id: &str,
    model_id: &str,
    live: bool,
    provider: Box<dyn Provider>,
) -> ModelSmokeResult {
    let response = provider
        .create_message(MessagesRequest {
            model: model_id.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: json!("Reply with a short provider smoke acknowledgement."),
            }],
            max_tokens: 32,
            system: None,
            temperature: None,
            tools: None,
            thinking: None,
            stream: None,
        })
        .await;
    match response {
        Ok(response) => {
            let text = response
                .content
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("");
            if text.trim().is_empty() {
                ModelSmokeResult {
                    provider_id: provider_id.to_string(),
                    model_id: model_id.to_string(),
                    status: "failed".to_string(),
                    live,
                    capability: "text",
                    message: "provider returned an empty text response".to_string(),
                    output_preview: None,
                }
            } else {
                ModelSmokeResult {
                    provider_id: provider_id.to_string(),
                    model_id: response.model,
                    status: "passed".to_string(),
                    live,
                    capability: "text",
                    message: "provider returned text".to_string(),
                    output_preview: Some(truncate_smoke_preview(&text)),
                }
            }
        }
        Err(error) => failed_provider_smoke(provider_id, model_id, live, "text", error),
    }
}

async fn run_provider_tool_smoke(
    provider_id: &str,
    model_id: &str,
    live: bool,
    provider: Box<dyn Provider>,
) -> ModelSmokeResult {
    let response = provider
        .create_message(MessagesRequest {
            model: model_id.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: json!(
                    "Call the ProviderSmoke tool with status ok. Do not answer in plain text."
                ),
            }],
            max_tokens: 128,
            system: None,
            temperature: Some(0.0),
            tools: Some(vec![provider_smoke_tool_definition()]),
            thinking: None,
            stream: None,
        })
        .await;
    match response {
        Ok(response) => {
            let tool_use = response.content.iter().find(|block| {
                block.get("type").and_then(Value::as_str) == Some("tool_use")
                    && block.get("name").and_then(Value::as_str) == Some("ProviderSmoke")
            });
            if let Some(tool_use) = tool_use {
                ModelSmokeResult {
                    provider_id: provider_id.to_string(),
                    model_id: response.model,
                    status: "passed".to_string(),
                    live,
                    capability: "tools",
                    message: "provider returned ProviderSmoke tool_use".to_string(),
                    output_preview: tool_use
                        .get("input")
                        .map(|input| truncate_smoke_preview(&input.to_string())),
                }
            } else {
                ModelSmokeResult {
                    provider_id: provider_id.to_string(),
                    model_id: response.model,
                    status: "failed".to_string(),
                    live,
                    capability: "tools",
                    message: "provider did not return ProviderSmoke tool_use".to_string(),
                    output_preview: Some(truncate_smoke_preview(
                        &serde_json::to_string(&response.content).unwrap_or_default(),
                    )),
                }
            }
        }
        Err(error) => failed_provider_smoke(provider_id, model_id, live, "tools", error),
    }
}

fn provider_smoke_tool_definition() -> Value {
    json!({
        "name": "ProviderSmoke",
        "description": "Report that the provider can emit a tool call.",
        "input_schema": {
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["ok"]
                }
            },
            "required": ["status"]
        }
    })
}

fn skipped_provider_smoke(
    provider_id: &str,
    model_id: &str,
    live: bool,
    capability: &'static str,
    message: &str,
) -> ModelSmokeResult {
    ModelSmokeResult {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        status: "skipped".to_string(),
        live,
        capability,
        message: message.to_string(),
        output_preview: None,
    }
}

fn failed_provider_smoke(
    provider_id: &str,
    model_id: &str,
    live: bool,
    capability: &'static str,
    error: impl std::fmt::Display,
) -> ModelSmokeResult {
    ModelSmokeResult {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
        status: "failed".to_string(),
        live,
        capability,
        message: error.to_string(),
        output_preview: None,
    }
}

fn truncate_smoke_preview(text: &str) -> String {
    const MAX_CHARS: usize = 120;
    let mut output = String::new();
    for (index, ch) in text.chars().enumerate() {
        if index >= MAX_CHARS {
            output.push_str("...");
            return output;
        }
        output.push(ch);
    }
    output
}

fn model_smoke_text(report: &ModelSmokeReport) -> String {
    let mut lines = vec![
        "Model provider smoke".to_string(),
        format!(
            "summary: passed={} skipped={} failed={} live={} tools={}",
            report.summary.passed,
            report.summary.skipped,
            report.summary.failed,
            report.live,
            report.tools
        ),
        "provider\tmodel\tcapability\tstatus\tmessage".to_string(),
    ];
    for result in &report.results {
        lines.push(format!(
            "{}\t{}\t{}\t{}\t{}",
            result.provider_id, result.model_id, result.capability, result.status, result.message
        ));
    }
    lines.join("\n")
}

fn env_string(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn provider_model_id(provider_id: &str) -> String {
    let entry = provider_entry(provider_id);
    first_env_value(&entry.model_env_vars).unwrap_or(entry.default_model_id)
}

fn provider_api_key(provider_id: &str) -> Option<String> {
    let entry = provider_entry(provider_id);
    first_env_value(&entry.api_key_env_vars)
}

fn provider_base_url(provider_id: &str) -> Option<String> {
    let entry = provider_entry(provider_id);
    first_env_value(&entry.base_url_env_vars).or(entry.default_base_url)
}

fn missing_api_key_message(provider_id: &str) -> String {
    let entry = provider_entry(provider_id);
    format!("{} is not set", entry.api_key_env_vars.join(" or "))
}

fn first_env_value(keys: &[String]) -> Option<String> {
    keys.iter().find_map(|key| env_string(key))
}

fn provider_entry(provider_id: &str) -> ProviderRegistryEntry {
    provider_registry_entry(provider_id).expect("built-in provider registry entry should exist")
}

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

fn save_model(model: String) -> anyhow::Result<CommandResult> {
    let mut config = load_user_config();
    config.model = model.clone();
    let path = save_user_config(&config)?;

    Ok(CommandResult::text(format!(
        "Model updated\nmodel: {}\nfile: {}",
        model,
        path.display()
    )))
}

fn usage() -> &'static str {
    "Usage: kiana model <model-name>\n       kiana model status\n       kiana model list [--json]\n       kiana model smoke [--json] [--live] [--tools]\n       kiana model reset"
}

#[cfg(test)]
mod tests {
    use super::ModelCommand;
    use crate::local_state::{env_lock, save_user_config};
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
            "kiana-model-command-{}-{unique}.toml",
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

    fn write_config_with_model(path: &std::path::Path) {
        std::env::set_var("KIANA_CONFIG_FILE", path);
        let mut config = kiana_bootstrap::config::Config::default();
        config.model = "claude-existing-model".to_string();
        save_user_config(&config).unwrap();
    }

    fn clear_model_smoke_env() {
        for key in [
            "KIANA_PROVIDER_SMOKE_LIVE",
            "KIANA_PROVIDER_SMOKE_TOOLS",
            "ANTHROPIC_API_KEY",
            "ANTHROPIC_MODEL",
            "KIANA_OPENAI_API_KEY",
            "OPENAI_API_KEY",
            "KIANA_OPENAI_MODEL",
            "OPENAI_MODEL",
            "KIANA_OLLAMA_MODEL",
            "OLLAMA_MODEL",
        ] {
            std::env::remove_var(key);
        }
    }

    fn file_contains(path: &std::path::Path, needle: &str) -> bool {
        fs::read_to_string(path).unwrap().contains(needle)
    }

    #[tokio::test]
    async fn model_help_does_not_persist_help_as_model() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_model(&path);

        let result = ModelCommand.execute(context("--help")).await.unwrap();

        assert!(result.value.contains("Usage: kiana model <model-name>"));
        assert!(file_contains(&path, "claude-existing-model"));
        assert!(!file_contains(&path, "model = \"--help\""));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn model_status_with_extra_words_is_not_saved_as_model() {
        let _guard = lock_env();
        let path = temp_config_path();
        write_config_with_model(&path);

        let error = ModelCommand
            .execute(context("status please"))
            .await
            .unwrap_err()
            .to_string();

        assert!(error.contains("unknown model command"));
        assert!(file_contains(&path, "claude-existing-model"));
        assert!(!file_contains(&path, "status please"));

        let _ = fs::remove_file(&path);
        std::env::remove_var("KIANA_CONFIG_FILE");
    }

    #[tokio::test]
    async fn model_list_json_outputs_provider_capabilities() {
        let result = ModelCommand.execute(context("list --json")).await.unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();
        let profiles = value.as_array().unwrap();

        assert!(profiles.iter().any(|profile| {
            profile["provider_id"].as_str() == Some("anthropic")
                && profile["model_id"].as_str() == Some("claude-sonnet-4-6")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
                && profile["streaming_mode"].as_str() == Some("native")
                && profile["native_streaming"].as_bool() == Some(true)
        }));
        assert!(profiles.iter().any(|profile| {
            profile["provider_id"].as_str() == Some("fake")
                && profile["model_id"].as_str() == Some("fake-model")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["context_window"].as_u64().unwrap() > 0
        }));
        assert!(profiles.iter().any(|profile| {
            profile["provider_id"].as_str() == Some("openai-compatible")
                && profile["model_id"].as_str() == Some("gpt-4.1")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
                && profile["streaming_mode"].as_str() == Some("synthetic")
                && profile["native_streaming"].as_bool() == Some(false)
        }));
        assert!(profiles.iter().any(|profile| {
            profile["provider_id"].as_str() == Some("ollama")
                && profile["model_id"].as_str() == Some("llama3.1")
                && profile["supports_tools"].as_bool() == Some(true)
                && profile["supports_streaming"].as_bool() == Some(true)
                && profile["streaming_mode"].as_str() == Some("synthetic")
                && profile["native_streaming"].as_bool() == Some(false)
        }));
    }

    #[tokio::test]
    async fn model_smoke_json_reports_fake_pass_and_live_skips_by_default() {
        let _guard = lock_env();
        clear_model_smoke_env();

        let result = ModelCommand.execute(context("smoke --json")).await.unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();
        let results = value["results"].as_array().unwrap();

        assert_eq!(value["schema"], "kiana.model-smoke.v1");
        assert_eq!(value["live"], false);
        assert_eq!(value["tools"], false);
        assert_eq!(value["summary"]["passed"], 1);
        assert_eq!(value["summary"]["skipped"], 3);
        assert_eq!(value["summary"]["failed"], 0);
        assert!(results.iter().any(|result| {
            result["provider_id"].as_str() == Some("fake")
                && result["status"].as_str() == Some("passed")
                && result["output_preview"].as_str() == Some("kiana-provider-smoke")
        }));
        assert!(results.iter().any(|result| {
            result["provider_id"].as_str() == Some("openai-compatible")
                && result["status"].as_str() == Some("skipped")
                && result["message"]
                    .as_str()
                    .unwrap()
                    .contains("live provider smoke disabled")
        }));
        assert!(results.iter().any(|result| {
            result["provider_id"].as_str() == Some("ollama")
                && result["status"].as_str() == Some("skipped")
        }));
    }

    #[tokio::test]
    async fn model_smoke_tools_json_reports_fake_tool_pass_and_live_tool_skips() {
        let _guard = lock_env();
        clear_model_smoke_env();

        let result = ModelCommand
            .execute(context("smoke --json --tools"))
            .await
            .unwrap();
        let value: Value = serde_json::from_str(&result.value).unwrap();
        let results = value["results"].as_array().unwrap();

        assert_eq!(value["schema"], "kiana.model-smoke.v1");
        assert_eq!(value["live"], false);
        assert_eq!(value["tools"], true);
        assert_eq!(value["summary"]["passed"], 2);
        assert_eq!(value["summary"]["skipped"], 6);
        assert_eq!(value["summary"]["failed"], 0);
        assert!(results.iter().any(|result| {
            result["provider_id"].as_str() == Some("fake")
                && result["capability"].as_str() == Some("tools")
                && result["status"].as_str() == Some("passed")
                && result["message"]
                    .as_str()
                    .unwrap()
                    .contains("ProviderSmoke tool_use")
        }));
        assert!(results.iter().any(|result| {
            result["provider_id"].as_str() == Some("openai-compatible")
                && result["capability"].as_str() == Some("tools")
                && result["status"].as_str() == Some("skipped")
                && result["message"]
                    .as_str()
                    .unwrap()
                    .contains("live provider tool smoke disabled")
        }));
        assert!(results.iter().any(|result| {
            result["provider_id"].as_str() == Some("ollama")
                && result["capability"].as_str() == Some("tools")
                && result["status"].as_str() == Some("skipped")
        }));
    }

    #[tokio::test]
    async fn model_smoke_text_reports_summary() {
        let _guard = lock_env();
        clear_model_smoke_env();

        let result = ModelCommand.execute(context("smoke")).await.unwrap();

        assert!(result.value.contains("Model provider smoke"));
        assert!(result
            .value
            .contains("summary: passed=1 skipped=3 failed=0 live=false tools=false"));
        assert!(result.value.contains("fake\tfake-model\ttext\tpassed"));
        assert!(result.value.contains("ollama\tllama3.1\ttext\tskipped"));
    }
}
