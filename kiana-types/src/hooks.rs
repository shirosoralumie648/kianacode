use serde_json::Value;
use std::collections::BTreeMap;

pub type HookConfig = BTreeMap<String, Vec<String>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookConfigError {
    pub location: String,
    pub message: String,
}

impl HookConfigError {
    fn new(location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            location: location.into(),
            message: message.into(),
        }
    }
}

pub fn parse_hook_config(value: Value) -> Result<HookConfig, HookConfigError> {
    let mut config = HookConfig::new();
    if value.is_array() || value.is_string() {
        let commands = parse_hook_commands_value(&value, "$")?;
        if !commands.is_empty() {
            config.insert("hooks".to_string(), commands);
        }
        return Ok(config);
    }

    let Some(object) = value.as_object() else {
        return Err(HookConfigError::new(
            "$",
            "expected object, string command, or array of string commands",
        ));
    };

    for (key, value) in object {
        let slot = normalized_hook_key(key).ok_or_else(|| {
            HookConfigError::new(
                key,
                format!("unknown hook event '{key}'; expected a supported hook event"),
            )
        })?;
        let commands = parse_hook_commands_value(value, key)?;
        if !commands.is_empty() {
            config.insert(slot.to_string(), commands);
        }
    }
    Ok(config)
}

pub fn parse_hook_commands(value: &str) -> Result<Vec<String>, HookConfigError> {
    if let Ok(commands) = serde_json::from_str::<Vec<String>>(value) {
        return Ok(clean_commands(commands));
    }
    if let Ok(command) = serde_json::from_str::<String>(value) {
        return Ok(clean_commands(vec![command]));
    }
    if let Ok(value) = serde_json::from_str::<Value>(value) {
        return parse_hook_commands_value(&value, "$");
    }
    Ok(clean_commands(value.lines().map(str::to_string).collect()))
}

pub fn hook_commands_for_event(config: &HookConfig, hook_name: &str) -> Vec<String> {
    for key in hook_file_keys(hook_name) {
        if let Some(commands) = config.get(*key) {
            return commands.clone();
        }
    }
    for key in ["hooks", "fallback", "all", "KIANA_HOOKS"] {
        if let Some(commands) = config.get(key) {
            return commands.clone();
        }
    }
    Vec::new()
}

fn parse_hook_commands_value(
    value: &Value,
    location: &str,
) -> Result<Vec<String>, HookConfigError> {
    if let Some(command) = value.as_str() {
        return Ok(clean_commands(vec![command.to_string()]));
    }
    if let Some(commands) = value.as_array() {
        let mut parsed = Vec::new();
        for (index, item) in commands.iter().enumerate() {
            let Some(command) = item.as_str() else {
                return Err(HookConfigError::new(
                    format!("{location}[{index}]"),
                    "expected hook command to be a string",
                ));
            };
            parsed.push(command.to_string());
        }
        return Ok(clean_commands(parsed));
    }
    Err(HookConfigError::new(
        location,
        "expected hook command string or array of strings",
    ))
}

fn normalized_hook_key(key: &str) -> Option<&'static str> {
    match key {
        "Stop" | "stop" | "stop_hooks" | "stopHooks" | "KIANA_STOP_HOOKS" => Some("Stop"),
        "SessionStart"
        | "sessionStart"
        | "session_start"
        | "session_start_hooks"
        | "sessionStartHooks"
        | "KIANA_SESSION_START_HOOKS" => Some("SessionStart"),
        "UserPromptSubmit"
        | "userPromptSubmit"
        | "user_prompt_submit"
        | "user_prompt_submit_hooks"
        | "userPromptSubmitHooks"
        | "KIANA_USER_PROMPT_SUBMIT_HOOKS" => Some("UserPromptSubmit"),
        "PreToolUse"
        | "preToolUse"
        | "pre_tool_use"
        | "pre_tool_use_hooks"
        | "preToolUseHooks"
        | "KIANA_PRE_TOOL_USE_HOOKS" => Some("PreToolUse"),
        "PostToolUse"
        | "postToolUse"
        | "post_tool_use"
        | "post_tool_use_hooks"
        | "postToolUseHooks"
        | "KIANA_POST_TOOL_USE_HOOKS" => Some("PostToolUse"),
        "TaskCompleted"
        | "taskCompleted"
        | "task_completed"
        | "task_completed_hooks"
        | "taskCompletedHooks"
        | "KIANA_TASK_COMPLETED_HOOKS" => Some("TaskCompleted"),
        "TeammateIdle"
        | "teammateIdle"
        | "teammate_idle"
        | "teammate_idle_hooks"
        | "teammateIdleHooks"
        | "KIANA_TEAMMATE_IDLE_HOOKS" => Some("TeammateIdle"),
        "hooks" | "fallback" | "all" | "KIANA_HOOKS" => Some("hooks"),
        _ => None,
    }
}

fn hook_file_keys(hook_name: &str) -> &'static [&'static str] {
    match hook_name {
        "Stop" => &[
            "Stop",
            "stop",
            "stop_hooks",
            "stopHooks",
            "KIANA_STOP_HOOKS",
        ],
        "SessionStart" => &[
            "SessionStart",
            "sessionStart",
            "session_start",
            "session_start_hooks",
            "sessionStartHooks",
            "KIANA_SESSION_START_HOOKS",
        ],
        "UserPromptSubmit" => &[
            "UserPromptSubmit",
            "userPromptSubmit",
            "user_prompt_submit",
            "user_prompt_submit_hooks",
            "userPromptSubmitHooks",
            "KIANA_USER_PROMPT_SUBMIT_HOOKS",
        ],
        "PreToolUse" => &[
            "PreToolUse",
            "preToolUse",
            "pre_tool_use",
            "pre_tool_use_hooks",
            "preToolUseHooks",
            "KIANA_PRE_TOOL_USE_HOOKS",
        ],
        "PostToolUse" => &[
            "PostToolUse",
            "postToolUse",
            "post_tool_use",
            "post_tool_use_hooks",
            "postToolUseHooks",
            "KIANA_POST_TOOL_USE_HOOKS",
        ],
        "TaskCompleted" => &[
            "TaskCompleted",
            "taskCompleted",
            "task_completed",
            "task_completed_hooks",
            "taskCompletedHooks",
            "KIANA_TASK_COMPLETED_HOOKS",
        ],
        "TeammateIdle" => &[
            "TeammateIdle",
            "teammateIdle",
            "teammate_idle",
            "teammate_idle_hooks",
            "teammateIdleHooks",
            "KIANA_TEAMMATE_IDLE_HOOKS",
        ],
        _ => &["hooks", "fallback", "all", "KIANA_HOOKS"],
    }
}

fn clean_commands(commands: Vec<String>) -> Vec<String> {
    commands
        .into_iter()
        .map(|command| command.trim().to_string())
        .filter(|command| !command.is_empty())
        .collect()
}
