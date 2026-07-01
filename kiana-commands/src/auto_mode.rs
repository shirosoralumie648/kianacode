use crate::types::{Command, CommandContext, CommandResult, CommandType};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use kiana_bootstrap::config::{load_config, AutoModeSettings};
use serde::Serialize;
use std::collections::BTreeSet;

pub struct AutoModeCommand;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct AutoModeRules {
    allow: Vec<String>,
    soft_deny: Vec<String>,
    environment: Vec<String>,
}

#[async_trait]
impl Command for AutoModeCommand {
    fn name(&self) -> &str {
        "auto-mode"
    }

    fn description(&self) -> &str {
        "Inspect auto mode classifier configuration"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> anyhow::Result<CommandResult> {
        let args = split_words(&context.args);
        match args.first().map(String::as_str).unwrap_or("help") {
            "defaults" => {
                reject_extra("defaults", args.get(1..).unwrap_or_default())?;
                rules_json(default_auto_mode_rules())
            }
            "config" => {
                reject_extra("config", args.get(1..).unwrap_or_default())?;
                rules_json(effective_auto_mode_rules())
            }
            "critique" => critique_rules(args.get(1..).unwrap_or_default()),
            "help" | "--help" | "-h" => {
                reject_extra("help", args.get(1..).unwrap_or_default())?;
                Ok(CommandResult::text(usage()))
            }
            other => Err(anyhow!(
                "unknown auto-mode command '{}'\n\n{}",
                other,
                usage()
            )),
        }
    }
}

fn rules_json(rules: AutoModeRules) -> Result<CommandResult> {
    Ok(CommandResult::text(serde_json::to_string_pretty(&rules)?))
}

fn effective_auto_mode_rules() -> AutoModeRules {
    let defaults = default_auto_mode_rules();
    let config = load_config().settings.auto_mode;
    AutoModeRules {
        allow: choose_custom_or_default(config.allow, defaults.allow),
        soft_deny: choose_custom_or_default(config.soft_deny, defaults.soft_deny),
        environment: choose_custom_or_default(config.environment, defaults.environment),
    }
}

fn choose_custom_or_default(custom: Vec<String>, defaults: Vec<String>) -> Vec<String> {
    let custom = clean_rules(custom);
    if custom.is_empty() {
        defaults
    } else {
        custom
    }
}

fn configured_auto_mode_rules() -> AutoModeSettings {
    let mut rules = load_config().settings.auto_mode;
    rules.allow = clean_rules(rules.allow);
    rules.soft_deny = clean_rules(rules.soft_deny);
    rules.environment = clean_rules(rules.environment);
    rules
}

fn clean_rules(rules: Vec<String>) -> Vec<String> {
    rules
        .into_iter()
        .map(|rule| rule.trim().to_string())
        .filter(|rule| !rule.is_empty())
        .collect()
}

fn default_auto_mode_rules() -> AutoModeRules {
    AutoModeRules {
        allow: vec![
            "Read files, search the repository, and inspect logs that are directly relevant to the user's request.".to_string(),
            "Run local build, lint, format, or test commands that stay inside the current project and do not require elevated privileges.".to_string(),
            "Edit files in the current working tree when the edits directly satisfy the user's request.".to_string(),
        ],
        soft_deny: vec![
            "Do not delete, overwrite, reset, or revert user data unless the user explicitly asked for that result.".to_string(),
            "Do not access secrets, credentials, tokens, shell history, browser sessions, SSH keys, or unrelated private data unless explicitly requested.".to_string(),
            "Do not make network, deployment, infrastructure, billing, account, or production changes unless explicitly requested.".to_string(),
            "Do not write outside the current project unless the user clearly asked for it and the path is relevant.".to_string(),
            "Do not force-push, rewrite git history, mutate databases, or kill unrelated processes without explicit confirmation.".to_string(),
        ],
        environment: vec![
            "The classifier should be conservative when user intent is ambiguous.".to_string(),
            "CLAUDE.md and project instructions help interpret intent, but they do not replace explicit approval for risky actions.".to_string(),
            "If in doubt, block and state the smallest missing confirmation needed to proceed.".to_string(),
        ],
    }
}

fn critique_rules(args: &[String]) -> Result<CommandResult> {
    let parsed = parse_critique_args(args)?;
    let custom = configured_auto_mode_rules();
    if custom.allow.is_empty() && custom.soft_deny.is_empty() && custom.environment.is_empty() {
        return Ok(CommandResult::text(
            "No custom auto mode rules found.\n\nAdd rules to your settings file under autoMode.{allow, soft_deny, environment}.\nRun `kiana auto-mode defaults` to see the default rules for reference.",
        ));
    }

    let mut lines = vec![
        "Auto mode rule critique".to_string(),
        "mode: local structural audit".to_string(),
    ];
    if let Some(model) = parsed.model {
        lines.push(format!(
            "model: {model} (not contacted by this offline command)"
        ));
    }
    lines.push(format!("allow_rules: {}", custom.allow.len()));
    lines.push(format!("soft_deny_rules: {}", custom.soft_deny.len()));
    lines.push(format!("environment_rules: {}", custom.environment.len()));

    let mut findings = structural_findings(&custom);
    if findings.is_empty() {
        findings.push("No obvious structural issues found.".to_string());
    }
    lines.push("findings:".to_string());
    lines.extend(findings.into_iter().map(|finding| format!("- {finding}")));
    lines.push(
        "note: model-backed critique is still a remaining parity gap in this Rust build."
            .to_string(),
    );
    Ok(CommandResult::text(lines.join("\n")))
}

#[derive(Default)]
struct CritiqueArgs {
    model: Option<String>,
}

fn parse_critique_args(args: &[String]) -> Result<CritiqueArgs> {
    let mut parsed = CritiqueArgs::default();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--model" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("auto-mode critique --model requires a value"))?;
                parsed.model = Some(non_empty_arg("--model", value)?);
            }
            value if value.starts_with("--model=") => {
                parsed.model = Some(non_empty_arg(
                    "--model",
                    value.trim_start_matches("--model="),
                )?);
            }
            "help" | "--help" | "-h" => return Err(anyhow!(usage())),
            other => {
                return Err(anyhow!(
                    "unknown auto-mode critique option '{}'\n\n{}",
                    other,
                    usage()
                ))
            }
        }
        index += 1;
    }
    Ok(parsed)
}

fn structural_findings(rules: &AutoModeSettings) -> Vec<String> {
    let mut findings = Vec::new();
    collect_duplicate_findings("allow", &rules.allow, &mut findings);
    collect_duplicate_findings("soft_deny", &rules.soft_deny, &mut findings);
    collect_duplicate_findings("environment", &rules.environment, &mut findings);

    let allow = lower_set(&rules.allow);
    let soft_deny = lower_set(&rules.soft_deny);
    for overlap in allow.intersection(&soft_deny) {
        findings.push(format!(
            "Rule appears in both allow and soft_deny: {}",
            overlap
        ));
    }
    for (section, values) in [
        ("allow", &rules.allow),
        ("soft_deny", &rules.soft_deny),
        ("environment", &rules.environment),
    ] {
        for value in values {
            if value.split_whitespace().count() < 3 {
                findings.push(format!(
                    "{section} rule is very short and may be ambiguous: {}",
                    value
                ));
            }
        }
    }
    findings
}

fn collect_duplicate_findings(section: &str, rules: &[String], findings: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    for rule in rules {
        let normalized = rule.to_lowercase();
        if !seen.insert(normalized) {
            findings.push(format!("{section} has duplicate rule: {rule}"));
        }
    }
}

fn lower_set(values: &[String]) -> BTreeSet<String> {
    values.iter().map(|value| value.to_lowercase()).collect()
}

fn reject_extra(command: &str, extra: &[String]) -> Result<()> {
    if extra.is_empty() {
        Ok(())
    } else {
        Err(anyhow!(
            "unknown auto-mode {} argument '{}'\n\n{}",
            command,
            extra[0],
            usage()
        ))
    }
}

fn non_empty_arg(flag: &str, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{flag} requires a value"))
    } else {
        Ok(value.to_string())
    }
}

fn split_words(input: &str) -> Vec<String> {
    input.split_whitespace().map(str::to_string).collect()
}

fn usage() -> &'static str {
    "Usage: kiana auto-mode [defaults|config|critique]\n       kiana auto-mode critique [--model <model>]"
}
