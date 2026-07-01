use crate::{
    mcp_tool::{MCP_SERVERS_APP_STATE_KEY, MCP_SERVERS_ENV},
    shell::preferred_bash_program,
    tool::*,
};
use async_trait::async_trait;
use kiana_types::{project_trust_from_app_state, ProjectTrust};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::env;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::process::Command;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
struct AgentInput {
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    context: Option<String>,
    #[serde(default, alias = "agent_type", alias = "agentType")]
    subagent_type: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default, alias = "agent_name", alias = "agentName")]
    name: Option<String>,
    #[serde(default, alias = "teamName")]
    team_name: Option<String>,
    #[serde(default, alias = "planModeRequired")]
    plan_mode_required: bool,
    #[serde(default)]
    run_in_background: bool,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    isolation: Option<String>,
    #[serde(default)]
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct AgentDefinition {
    agent_type: String,
    description: String,
    system_prompt: String,
    tools: Option<Vec<String>>,
    disallowed_tools: Vec<String>,
    skills: Vec<String>,
    hooks: Vec<AgentLifecycleHook>,
    memory: Option<AgentMemoryScope>,
    permission_mode: Option<String>,
    max_turns: Option<u64>,
    mcp_servers: Option<Value>,
    isolation: Option<String>,
    model: Option<String>,
    initial_prompt: Option<String>,
    background: bool,
    path: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentMemoryScope {
    User,
    Project,
    Local,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
enum AgentLifecycleEvent {
    SubagentStart,
    SubagentStop,
    Stop,
}

#[derive(Debug, Clone, Serialize)]
struct AgentLifecycleHook {
    event: AgentLifecycleEvent,
    matcher: Option<String>,
    command: String,
    timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct AgentLifecycleHookResult {
    event: String,
    command: String,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
    additional_context: Option<String>,
    blocking_error: Option<String>,
    duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct AgentLifecycleHookOutcome {
    results: Vec<AgentLifecycleHookResult>,
    additional_contexts: Vec<String>,
    blocking_errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct PreloadedSkill {
    name: String,
    path: String,
    content: String,
}

#[derive(Debug, Clone, Serialize)]
struct AgentIsolation {
    mode: String,
    path: PathBuf,
    strategy: String,
    branch: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct AgentFrontmatter {
    name: Option<String>,
    #[serde(default, alias = "when-to-use", alias = "when_to_use")]
    description: Option<String>,
    #[serde(default)]
    tools: Option<serde_yaml::Value>,
    #[serde(
        default,
        rename = "disallowedTools",
        alias = "disallowed_tools",
        alias = "disallowed-tools"
    )]
    disallowed_tools: Option<serde_yaml::Value>,
    #[serde(default)]
    skills: Option<serde_yaml::Value>,
    #[serde(default)]
    hooks: Option<serde_yaml::Value>,
    #[serde(default)]
    memory: Option<String>,
    #[serde(
        default,
        rename = "permissionMode",
        alias = "permission_mode",
        alias = "permission-mode"
    )]
    permission_mode: Option<String>,
    #[serde(default, rename = "maxTurns", alias = "max_turns", alias = "max-turns")]
    max_turns: Option<serde_yaml::Value>,
    #[serde(
        default,
        rename = "mcpServers",
        alias = "mcp_servers",
        alias = "mcp-servers"
    )]
    mcp_servers: Option<serde_yaml::Value>,
    #[serde(default)]
    isolation: Option<String>,
    model: Option<String>,
    #[serde(
        default,
        rename = "initialPrompt",
        alias = "initial_prompt",
        alias = "initial-prompt"
    )]
    initial_prompt: Option<String>,
    #[serde(default)]
    background: Option<serde_yaml::Value>,
}

pub struct AgentTool;

impl AgentTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "Agent"
    }

    fn description(&self) -> &str {
        "Launch a new agent"
    }

    fn search_hint(&self) -> Option<&str> {
        Some("delegate task to sub-agent")
    }

    fn is_concurrency_safe(&self) -> bool {
        true
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "A short description of the delegated task"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "task": {
                    "type": "string",
                    "description": "Backward-compatible alias for prompt"
                },
                "context": {
                    "type": "string",
                    "description": "Additional context for the sub-agent"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Optional specialized agent type label"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Reference-compatible alias for subagent_type"
                },
                "model": {
                    "type": "string",
                    "description": "Optional model override for the spawned agent"
                },
                "name": {
                    "type": "string",
                    "description": "Optional teammate name when joining a team"
                },
                "agent_name": {
                    "type": "string",
                    "description": "Reference-compatible alias for name"
                },
                "team_name": {
                    "type": "string",
                    "description": "Team to join as a teammate"
                },
                "teamName": {
                    "type": "string",
                    "description": "Reference-compatible alias for team_name"
                },
                "planModeRequired": {
                    "type": "boolean",
                    "description": "Require plan mode before implementation for this teammate"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run the agent in the background and read output later with TaskOutput"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory for the spawned agent"
                },
                "isolation": {
                    "type": "string",
                    "enum": ["worktree"],
                    "description": "Run the agent in an isolated worktree/snapshot copy"
                },
                "timeout_ms": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 600000
                }
            },
            "required": ["prompt"]
        })
    }

    fn output_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": { "type": "string" },
                "message": { "type": "string" },
                "agentId": { "type": "string" },
                "task_id": { "type": "string" },
                "description": { "type": "string" },
                "prompt": { "type": "string" },
                "stdout": { "type": "string" },
                "stderr": { "type": "string" },
                "exit_code": { "type": "integer" },
                "cwd": { "type": "string" },
                "team_name": { "type": "string" },
                "teamName": { "type": "string" },
                "teammate_id": { "type": "string" },
                "teammateId": { "type": "string" },
                "agent_name": { "type": "string" },
                "agentName": { "type": "string" },
                "task_list_id": { "type": "string" },
                "taskListId": { "type": "string" },
                "team_file_path": { "type": "string" },
                "mailbox_path": { "type": "string" },
                "plan_mode_required": { "type": "boolean" },
                "planModeRequired": { "type": "boolean" },
                "isolation": { "type": "string" },
                "worktree_path": { "type": "string" },
                "worktree_strategy": { "type": "string" },
                "outputFile": { "type": "string" },
                "canReadOutputFile": { "type": "boolean" }
            }
        })
    }

    async fn validate_input(&self, input: &Value, context: &ToolContext) -> ValidationResult {
        let input: AgentInput = match serde_json::from_value(input.clone()) {
            Ok(input) => input,
            Err(e) => return ValidationResult::err(format!("Invalid input: {}", e), 1),
        };

        if normalized_prompt(&input).is_none() {
            return ValidationResult::err("prompt cannot be empty".to_string(), 2);
        }
        if input
            .timeout_ms
            .is_some_and(|timeout_ms| timeout_ms == 0 || timeout_ms > MAX_AGENT_TIMEOUT_MS)
        {
            return ValidationResult::err(
                format!("timeout_ms must be between 1 and {MAX_AGENT_TIMEOUT_MS}"),
                3,
            );
        }
        if input
            .isolation
            .as_deref()
            .and_then(|isolation| parse_isolation(Some(isolation)))
            .is_none()
            && input
                .isolation
                .as_deref()
                .map(str::trim)
                .is_some_and(|isolation| !isolation.is_empty())
        {
            return ValidationResult::err(
                "isolation must be \"worktree\" when provided".to_string(),
                6,
            );
        }
        if input
            .isolation
            .as_deref()
            .and_then(|isolation| parse_isolation(Some(isolation)))
            .is_some()
            && input
                .cwd
                .as_deref()
                .map(str::trim)
                .is_some_and(|cwd| !cwd.is_empty())
        {
            return ValidationResult::err("cwd cannot be combined with isolation".to_string(), 7);
        }
        if let Some(cwd) = input
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|cwd| !cwd.is_empty())
        {
            let path = context.resolve_path(cwd);
            if !path.is_dir() {
                return ValidationResult::err(
                    format!("cwd '{}' is not a directory", path.display()),
                    4,
                );
            }
        }
        if let Some(agent_type) = normalized_optional(input.subagent_type.as_deref()) {
            let cwd = input
                .cwd
                .as_deref()
                .map(str::trim)
                .filter(|cwd| !cwd.is_empty())
                .map(|cwd| context.resolve_path(cwd))
                .unwrap_or_else(|| PathBuf::from(&context.cwd));
            if let Err(error) = resolve_agent_definition_with_trust(
                &agent_type,
                &cwd,
                project_trust_from_app_state(&context.app_state),
            ) {
                return ValidationResult::err(error, 5);
            }
        }

        ValidationResult::ok()
    }

    async fn call(&self, input: &Value, context: &mut ToolContext) -> ToolResult<ToolOutput> {
        let input: AgentInput = serde_json::from_value(input.clone())?;
        let prompt = normalized_prompt(&input)
            .ok_or_else(|| ToolError::ValidationError("prompt cannot be empty".to_string()))?;
        let base_cwd = agent_cwd(&input, context)?;
        let project_trust = project_trust_from_app_state(&context.app_state);
        let mut agent_definition =
            if let Some(agent_type) = normalized_optional(input.subagent_type.as_deref()) {
                Some(
                    resolve_agent_definition_with_trust(&agent_type, &base_cwd, project_trust)
                        .map_err(ToolError::ValidationError)?,
                )
            } else {
                None
            };
        enrich_claude_code_guide_agent(agent_definition.as_mut(), &base_cwd, context).await;
        let isolation = prepare_agent_isolation(&base_cwd, &input, agent_definition.as_ref())?;
        let run_cwd = isolation
            .as_ref()
            .map(|isolation| isolation.path.clone())
            .unwrap_or_else(|| base_cwd.clone());
        let description = normalized_description(&input, agent_definition.as_ref(), &prompt);
        let preloaded_skills =
            preload_agent_skills(agent_definition.as_ref(), &base_cwd, &context.app_state).await;
        let agent_mcp_servers = resolve_agent_mcp_servers(agent_definition.as_ref(), context);
        let runs_in_background = input.run_in_background
            || agent_definition
                .as_ref()
                .is_some_and(|agent| agent.background);
        let team_agent = prepare_team_agent_runtime(
            &input,
            agent_definition.as_ref(),
            context,
            &prompt,
            &description,
            &run_cwd,
            runs_in_background,
        )?;
        let agent_type = agent_definition
            .as_ref()
            .map(|agent| agent.agent_type.clone())
            .or_else(|| normalized_optional(input.subagent_type.as_deref()))
            .unwrap_or_else(|| "general-purpose".to_string());
        let agent_run_id = team_agent
            .as_ref()
            .map(|team| team.agent_id.clone())
            .unwrap_or_else(|| format!("agent-{}", Uuid::new_v4()));
        let start_hooks = run_agent_lifecycle_hooks(
            agent_definition.as_ref(),
            AgentLifecycleEvent::SubagentStart,
            AgentLifecycleContext {
                agent_id: &agent_run_id,
                agent_type: &agent_type,
                prompt: &prompt,
                cwd: &run_cwd,
                status: None,
                exit_code: None,
                stdout: None,
                stderr: None,
                project_trust,
            },
        )
        .await?;
        let prompt_with_hooks = prompt_with_hook_context(&prompt, &start_hooks.additional_contexts);
        let runner_prompt = prompt_with_preloaded_skills(&prompt_with_hooks, &preloaded_skills);
        let start_hook_results = start_hooks.results.clone();

        if runs_in_background {
            return spawn_background_agent(
                input,
                agent_definition,
                context,
                prompt,
                runner_prompt,
                description,
                run_cwd,
                preloaded_skills,
                agent_mcp_servers,
                isolation,
                team_agent,
            );
        }

        let timeout_ms = input.timeout_ms.unwrap_or(DEFAULT_AGENT_TIMEOUT_MS);
        let mut command = agent_command(
            &input,
            agent_definition.as_ref(),
            &runner_prompt,
            &preloaded_skills,
            agent_mcp_servers.as_ref(),
            isolation.as_ref(),
            team_agent.as_ref(),
        )?;
        command.current_dir(&run_cwd).kill_on_drop(true);
        let output = match tokio::time::timeout(Duration::from_millis(timeout_ms), command.output())
            .await
        {
            Ok(output) => output?,
            Err(_) => {
                let hook_results = stop_agent_lifecycle_hooks_for_output(
                    agent_definition.as_ref(),
                    &agent_run_id,
                    &agent_type,
                    &prompt,
                    &run_cwd,
                    "failed",
                    -1,
                    "",
                    &format!("Agent timed out after {timeout_ms}ms"),
                    start_hook_results,
                    project_trust,
                )
                .await;
                return Ok(ToolOutput {
                    data: json!({
                        "status": "failed",
                        "agentId": agent_run_id,
                        "prompt": prompt,
                        "description": description,
                        "stdout": "",
                        "stderr": format!("Agent timed out after {timeout_ms}ms"),
                        "exit_code": -1,
                        "cwd": run_cwd.to_string_lossy(),
                        "team_name": team_agent.as_ref().map(|team| team.team_name.clone()),
                        "teamName": team_agent.as_ref().map(|team| team.team_name.clone()),
                        "teammate_id": team_agent.as_ref().map(|team| team.agent_id.clone()),
                        "teammateId": team_agent.as_ref().map(|team| team.agent_id.clone()),
                        "agent_name": team_agent.as_ref().map(|team| team.agent_name.clone()),
                        "agentName": team_agent.as_ref().map(|team| team.agent_name.clone()),
                        "task_list_id": team_agent.as_ref().map(|team| team.task_list_id.clone()),
                        "taskListId": team_agent.as_ref().map(|team| team.task_list_id.clone()),
                        "team_file_path": team_agent.as_ref().map(|team| team.team_file_path.to_string_lossy().to_string()),
                        "mailbox_path": team_agent.as_ref().map(|team| team.mailbox_path.to_string_lossy().to_string()),
                        "plan_mode_required": team_agent.as_ref().map(|team| team.plan_mode_required),
                        "planModeRequired": team_agent.as_ref().map(|team| team.plan_mode_required),
                        "isolation": isolation.as_ref().map(|isolation| isolation.mode.clone()),
                        "worktree_path": isolation.as_ref().map(|isolation| isolation.path.to_string_lossy().to_string()),
                        "worktree_strategy": isolation.as_ref().map(|isolation| isolation.strategy.clone()),
                        "hook_results": hook_results.results,
                        "hook_errors": hook_results.blocking_errors,
                        "message": format!("Agent timed out after {timeout_ms}ms")
                    }),
                    metadata: None,
                });
            }
        };

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let exit_code = output.status.code().unwrap_or(-1);
        let status = if output.status.success() {
            "completed"
        } else {
            "failed"
        };
        let hook_results = stop_agent_lifecycle_hooks_for_output(
            agent_definition.as_ref(),
            &agent_run_id,
            &agent_type,
            &prompt,
            &run_cwd,
            status,
            exit_code,
            &stdout,
            &stderr,
            start_hook_results,
            project_trust,
        )
        .await;
        let status = if hook_results.blocking_errors.is_empty() {
            status
        } else {
            "failed"
        };
        let stderr = if hook_results.blocking_errors.is_empty() {
            stderr
        } else {
            let mut combined = stderr;
            if !combined.trim().is_empty() {
                combined.push('\n');
            }
            combined.push_str(&hook_results.blocking_errors.join("\n"));
            combined
        };

        Ok(ToolOutput {
            data: json!({
                "status": status,
                "agentId": agent_run_id,
                "prompt": prompt,
                "description": description,
                "stdout": stdout,
                "stderr": stderr,
                "exit_code": exit_code,
                "cwd": run_cwd.to_string_lossy(),
                "team_name": team_agent.as_ref().map(|team| team.team_name.clone()),
                "teamName": team_agent.as_ref().map(|team| team.team_name.clone()),
                "teammate_id": team_agent.as_ref().map(|team| team.agent_id.clone()),
                "teammateId": team_agent.as_ref().map(|team| team.agent_id.clone()),
                "agent_name": team_agent.as_ref().map(|team| team.agent_name.clone()),
                "agentName": team_agent.as_ref().map(|team| team.agent_name.clone()),
                "task_list_id": team_agent.as_ref().map(|team| team.task_list_id.clone()),
                "taskListId": team_agent.as_ref().map(|team| team.task_list_id.clone()),
                "team_file_path": team_agent.as_ref().map(|team| team.team_file_path.to_string_lossy().to_string()),
                "mailbox_path": team_agent.as_ref().map(|team| team.mailbox_path.to_string_lossy().to_string()),
                "plan_mode_required": team_agent.as_ref().map(|team| team.plan_mode_required),
                "planModeRequired": team_agent.as_ref().map(|team| team.plan_mode_required),
                "isolation": isolation.as_ref().map(|isolation| isolation.mode.clone()),
                "worktree_path": isolation.as_ref().map(|isolation| isolation.path.to_string_lossy().to_string()),
                "worktree_strategy": isolation.as_ref().map(|isolation| isolation.strategy.clone()),
                "worktree_branch": isolation.as_ref().and_then(|isolation| isolation.branch.clone()),
                "hook_results": hook_results.results,
                "hook_errors": hook_results.blocking_errors,
                "message": if output.status.success() {
                    "Agent completed."
                } else {
                    "Agent failed."
                }
            }),
            metadata: None,
        })
    }

    fn map_to_api_result(&self, output: &ToolOutput, tool_use_id: &str) -> Value {
        let message = match output.data.get("status").and_then(Value::as_str) {
            Some("async_launched") => {
                let agent_id = output
                    .data
                    .get("agentId")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let output_file = output
                    .data
                    .get("outputFile")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                format!(
                    "Async agent launched successfully.\nagentId: {agent_id}\nThe agent is working in the background. Read progress with TaskOutput or inspect output_file: {output_file}"
                )
            }
            Some("completed") => output
                .data
                .get("stdout")
                .and_then(Value::as_str)
                .filter(|stdout| !stdout.trim().is_empty())
                .unwrap_or("Agent completed.")
                .to_string(),
            Some("failed") => output
                .data
                .get("stderr")
                .and_then(Value::as_str)
                .filter(|stderr| !stderr.trim().is_empty())
                .unwrap_or("Agent failed.")
                .to_string(),
            _ => output
                .data
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("Agent finished.")
                .to_string(),
        };

        json!({
            "tool_use_id": tool_use_id,
            "type": "tool_result",
            "content": message
        })
    }
}

const DEFAULT_AGENT_TIMEOUT_MS: u64 = 600_000;
const MAX_AGENT_TIMEOUT_MS: u64 = 600_000;
const BACKGROUND_ROOT_KEY: &str = "background_root";
const AGENT_COMMAND_ENV: &str = "KIANA_AGENT_COMMAND";
const TEAMS_ROOT_KEY: &str = "teams_root";
const TEAMS_ROOT_ENV: &str = "KIANA_TEAMS_ROOT";
const TASKS_ROOT_KEY: &str = "tasks_root";
const TASKS_ROOT_ENV: &str = "KIANA_TASKS_ROOT";
const TEAM_CONTEXT_KEY: &str = "team_context";

#[derive(Debug, Clone, Serialize)]
struct TeamAgentRuntime {
    team_name: String,
    agent_name: String,
    agent_id: String,
    task_list_id: String,
    team_file_path: PathBuf,
    mailbox_path: PathBuf,
    teams_root: PathBuf,
    tasks_root: PathBuf,
    color: String,
    plan_mode_required: bool,
}

fn normalized_prompt(input: &AgentInput) -> Option<String> {
    input
        .prompt
        .as_deref()
        .or(input.task.as_deref())
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
        .map(|prompt| {
            let Some(context) = input
                .context
                .as_deref()
                .map(str::trim)
                .filter(|c| !c.is_empty())
            else {
                return prompt.to_string();
            };
            format!("Context:\n{context}\n\nTask:\n{prompt}")
        })
}

fn normalized_description(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    prompt: &str,
) -> String {
    input
        .description
        .as_deref()
        .or(input.context.as_deref())
        .or(agent.map(|agent| agent.description.as_str()))
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            let mut description = prompt
                .lines()
                .next()
                .unwrap_or("Agent task")
                .trim()
                .to_string();
            description.truncate(80);
            if description.is_empty() {
                "Agent task".to_string()
            } else {
                description
            }
        })
}

fn wants_team_agent(input: &AgentInput) -> bool {
    input
        .name
        .as_deref()
        .map(str::trim)
        .is_some_and(|name| !name.is_empty())
        || input
            .team_name
            .as_deref()
            .map(str::trim)
            .is_some_and(|team_name| !team_name.is_empty())
        || input.plan_mode_required
}

fn sanitize_team_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "default".to_string()
    } else {
        sanitized
    }
}

fn sanitize_agent_name(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
}

fn active_team_name(context: &ToolContext) -> Option<String> {
    context
        .app_state
        .get(TEAM_CONTEXT_KEY)
        .or_else(|| context.app_state.get("teamContext"))
        .and_then(Value::as_object)
        .and_then(|team| {
            team.get("team_name")
                .or_else(|| team.get("teamName"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            std::env::var("KIANA_TEAM_NAME")
                .ok()
                .or_else(|| std::env::var("CLAUDE_CODE_TEAM_NAME").ok())
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
        })
}

fn teams_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get(TEAMS_ROOT_KEY)
        .or_else(|| context.app_state.get("teamsRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return context.resolve_path(path);
    }
    if let Some(path) = std::env::var_os(TEAMS_ROOT_ENV) {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            PathBuf::from(&context.cwd).join(path)
        };
    }
    PathBuf::from(&context.cwd).join(".kiana").join("teams")
}

fn tasks_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get(TASKS_ROOT_KEY)
        .or_else(|| context.app_state.get("tasksRoot"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        return context.resolve_path(path);
    }
    if let Some(path) = std::env::var_os(TASKS_ROOT_ENV) {
        let path = PathBuf::from(path);
        return if path.is_absolute() {
            path
        } else {
            PathBuf::from(&context.cwd).join(path)
        };
    }
    PathBuf::from(&context.cwd).join(".kiana").join("tasks")
}

fn team_file_path(context: &ToolContext, team_name: &str) -> PathBuf {
    teams_root(context)
        .join(sanitize_team_name(team_name))
        .join("config.json")
}

fn team_inbox_path(context: &ToolContext, team_name: &str, agent_name: &str) -> PathBuf {
    teams_root(context)
        .join(sanitize_team_name(team_name))
        .join("inboxes")
        .join(format!("{}.json", sanitize_agent_name(agent_name)))
}

fn team_task_list_dir(context: &ToolContext, team_name: &str) -> PathBuf {
    tasks_root(context).join(sanitize_team_name(team_name))
}

fn read_json_file(path: &Path) -> ToolResult<Option<Value>> {
    if !path.is_file() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

fn write_json_file(path: &Path, value: &Value) -> ToolResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(value)?)?;
    Ok(())
}

fn team_agent_id(agent_name: &str, team_name: &str) -> String {
    format!(
        "{}@{}",
        sanitize_agent_name(agent_name),
        sanitize_agent_name(team_name)
    )
}

fn member_is_active(member: &Value) -> bool {
    member
        .get("isActive")
        .or_else(|| member.get("is_active"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn reusable_or_unique_agent_name(
    team_file: &Value,
    requested_name: &str,
) -> (String, Option<usize>) {
    let base = sanitize_agent_name(requested_name);
    let Some(members) = team_file.get("members").and_then(Value::as_array) else {
        return (base, None);
    };

    for (index, member) in members.iter().enumerate() {
        let Some(existing_name) = member.get("name").and_then(Value::as_str) else {
            continue;
        };
        if existing_name.eq_ignore_ascii_case(&base)
            && !existing_name.eq_ignore_ascii_case("team-lead")
            && !member_is_active(member)
        {
            return (sanitize_agent_name(existing_name), Some(index));
        }
    }

    let existing_names = members
        .iter()
        .filter_map(|member| member.get("name").and_then(Value::as_str))
        .map(|name| name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if !existing_names.contains(&base.to_ascii_lowercase()) {
        return (base, None);
    }

    for suffix in 2..1000 {
        let candidate = format!("{base}-{suffix}");
        if !existing_names.contains(&candidate.to_ascii_lowercase()) {
            return (candidate, None);
        }
    }

    (format!("{base}-{}", Uuid::new_v4().simple()), None)
}

fn team_agent_base_name(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    description: &str,
) -> String {
    normalized_optional(input.name.as_deref())
        .or_else(|| agent.map(|agent| agent.agent_type.clone()))
        .or_else(|| normalized_optional(input.subagent_type.as_deref()))
        .or_else(|| {
            description
                .lines()
                .next()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
        })
        .unwrap_or_else(|| "agent".to_string())
}

fn team_agent_color(team_file: &Value, agent_name: &str) -> String {
    if let Some(color) = team_file
        .get("members")
        .and_then(Value::as_array)
        .and_then(|members| {
            members.iter().find_map(|member| {
                let name = member.get("name").and_then(Value::as_str)?;
                if name.eq_ignore_ascii_case(agent_name) {
                    member.get("color").and_then(Value::as_str)
                } else {
                    None
                }
            })
        })
    {
        return color.to_string();
    }

    const COLORS: &[&str] = &[
        "blue", "green", "yellow", "magenta", "cyan", "red", "orange", "purple",
    ];
    let index = team_file
        .get("members")
        .and_then(Value::as_array)
        .map(|members| members.len())
        .unwrap_or_default();
    COLORS[index % COLORS.len()].to_string()
}

fn append_initial_team_mailbox(runtime: &TeamAgentRuntime, prompt: &str) -> ToolResult<()> {
    let mut inbox = read_json_file(&runtime.mailbox_path)?
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    inbox.push(json!({
        "from": "team-lead",
        "text": prompt,
        "summary": "initial_prompt",
        "timestamp": now_unix_seconds().to_string(),
        "read": false
    }));
    write_json_file(&runtime.mailbox_path, &Value::Array(inbox))
}

fn upsert_team_context(
    context: &mut ToolContext,
    runtime: &TeamAgentRuntime,
    agent_type: &str,
    run_cwd: &Path,
    backend_type: &str,
    lead_agent_id: Option<String>,
) {
    let mut team_context = context
        .app_state
        .get(TEAM_CONTEXT_KEY)
        .or_else(|| context.app_state.get("teamContext"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    team_context.insert("team_name".to_string(), json!(runtime.team_name));
    team_context.insert("teamName".to_string(), json!(runtime.team_name));
    team_context.insert(
        "team_file_path".to_string(),
        json!(runtime.team_file_path.to_string_lossy().to_string()),
    );
    team_context.insert(
        "teamFilePath".to_string(),
        json!(runtime.team_file_path.to_string_lossy().to_string()),
    );
    if let Some(lead_agent_id) = lead_agent_id {
        team_context.insert("lead_agent_id".to_string(), json!(lead_agent_id));
        team_context.insert("leadAgentId".to_string(), json!(lead_agent_id));
    }

    let mut teammates = team_context
        .remove("teammates")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    teammates.insert(
        runtime.agent_id.clone(),
        json!({
            "agent_id": runtime.agent_id,
            "agentId": runtime.agent_id,
            "name": runtime.agent_name,
            "agent_type": agent_type,
            "agentType": agent_type,
            "color": runtime.color,
            "tmuxSessionName": backend_type,
            "tmuxPaneId": backend_type,
            "cwd": run_cwd.to_string_lossy().to_string(),
            "spawned_at": now_unix_seconds(),
            "spawnedAt": now_unix_seconds(),
            "planModeRequired": runtime.plan_mode_required,
            "plan_mode_required": runtime.plan_mode_required
        }),
    );
    team_context.insert("teammates".to_string(), Value::Object(teammates));

    let value = Value::Object(team_context);
    context
        .app_state
        .insert(TEAM_CONTEXT_KEY.to_string(), value.clone());
    context.app_state.insert("teamContext".to_string(), value);
}

fn prepare_team_agent_runtime(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    context: &mut ToolContext,
    prompt: &str,
    description: &str,
    run_cwd: &Path,
    background: bool,
) -> ToolResult<Option<TeamAgentRuntime>> {
    if !wants_team_agent(input) {
        return Ok(None);
    }

    let team_name = normalized_optional(input.team_name.as_deref())
        .or_else(|| active_team_name(context))
        .ok_or_else(|| {
            ToolError::ValidationError(
                "team_name is required when launching a named teammate".to_string(),
            )
        })?;
    let team_file_path = team_file_path(context, &team_name);
    let Some(mut team_file) = read_json_file(&team_file_path)? else {
        return Err(ToolError::ValidationError(format!(
            "Team \"{}\" does not exist. Call TeamCreate first.",
            team_name
        )));
    };
    let base_name = team_agent_base_name(input, agent, description);
    let (agent_name, reusable_index) = reusable_or_unique_agent_name(&team_file, &base_name);
    let agent_id = team_agent_id(&agent_name, &team_name);
    let task_list_id = sanitize_team_name(&team_name);
    let runtime = TeamAgentRuntime {
        team_name: team_name.clone(),
        agent_name: agent_name.clone(),
        agent_id: agent_id.clone(),
        task_list_id: task_list_id.clone(),
        team_file_path: team_file_path.clone(),
        mailbox_path: team_inbox_path(context, &team_name, &agent_name),
        teams_root: teams_root(context),
        tasks_root: tasks_root(context),
        color: team_agent_color(&team_file, &agent_name),
        plan_mode_required: input.plan_mode_required,
    };

    fs::create_dir_all(team_task_list_dir(context, &team_name))?;
    append_initial_team_mailbox(&runtime, prompt)?;

    let now = now_unix_seconds();
    let agent_type = agent
        .map(|agent| agent.agent_type.clone())
        .or_else(|| normalized_optional(input.subagent_type.as_deref()))
        .unwrap_or_else(|| "teammate".to_string());
    let model = normalized_optional(input.model.as_deref()).or_else(|| agent_model(agent));
    let backend_type = if background {
        "background-agent"
    } else {
        "foreground-agent"
    };
    let member = json!({
        "agentId": agent_id,
        "agent_id": agent_id,
        "name": agent_name,
        "agentType": agent_type,
        "agent_type": agent_type,
        "model": model,
        "prompt": prompt,
        "color": runtime.color,
        "planModeRequired": runtime.plan_mode_required,
        "plan_mode_required": runtime.plan_mode_required,
        "joinedAt": now,
        "joined_at": now,
        "tmuxPaneId": backend_type,
        "tmux_pane_id": backend_type,
        "cwd": run_cwd.to_string_lossy().to_string(),
        "subscriptions": [],
        "backendType": backend_type,
        "backend_type": backend_type,
        "isActive": true,
        "is_active": true
    });

    let lead_agent_id = team_file
        .get("leadAgentId")
        .or_else(|| team_file.get("lead_agent_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let team_object = team_file.as_object_mut().ok_or_else(|| {
        ToolError::ValidationError(format!("Team \"{}\" config is not an object", team_name))
    })?;
    let members = team_object
        .entry("members".to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| {
            ToolError::ValidationError(format!("Team \"{}\" members is not an array", team_name))
        })?;
    if let Some(index) = reusable_index {
        if let Some(slot) = members.get_mut(index) {
            *slot = member;
        } else {
            members.push(member);
        }
    } else {
        members.push(member);
    }
    write_json_file(&team_file_path, &team_file)?;
    upsert_team_context(
        context,
        &runtime,
        &agent_type,
        run_cwd,
        backend_type,
        lead_agent_id,
    );
    Ok(Some(runtime))
}

fn agent_cwd(input: &AgentInput, context: &ToolContext) -> ToolResult<PathBuf> {
    let cwd = input
        .cwd
        .as_deref()
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .map(|cwd| context.resolve_path(cwd))
        .unwrap_or_else(|| PathBuf::from(&context.cwd));
    if !cwd.is_dir() {
        return Err(ToolError::ValidationError(format!(
            "cwd '{}' is not a directory",
            cwd.display()
        )));
    }
    Ok(cwd)
}

fn agent_command(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    prompt: &str,
    preloaded_skills: &[PreloadedSkill],
    agent_mcp_servers: Option<&Value>,
    isolation: Option<&AgentIsolation>,
    team_agent: Option<&TeamAgentRuntime>,
) -> ToolResult<Command> {
    if let Ok(command) = std::env::var(AGENT_COMMAND_ENV) {
        let mut process = Command::new(preferred_bash_program());
        process
            .arg("-lc")
            .arg(command)
            .env("KIANA_AGENT_PROMPT", prompt)
            .env(
                "KIANA_AGENT_DESCRIPTION",
                normalized_description(input, agent, prompt),
            );
        if let Some(agent_type) = agent
            .map(|agent| agent.agent_type.clone())
            .or_else(|| normalized_optional(input.subagent_type.as_deref()))
        {
            process.env("KIANA_AGENT_TYPE", agent_type);
        }
        if let Some(system_prompt) = agent.map(|agent| agent.system_prompt.clone()) {
            process.env("KIANA_AGENT_SYSTEM_PROMPT", system_prompt);
        }
        if let Some(initial_prompt) = agent.and_then(|agent| agent.initial_prompt.clone()) {
            process.env("KIANA_AGENT_INITIAL_PROMPT", initial_prompt);
        }
        if let Some(agent) = agent {
            process.env("KIANA_AGENT_TOOLS", json_string(&agent.tools)?);
            process.env(
                "KIANA_AGENT_DISALLOWED_TOOLS",
                json_string(&agent.disallowed_tools)?,
            );
            process.env("KIANA_AGENT_SKILLS", json_string(&agent.skills)?);
            if let Some(memory) = agent.memory {
                process.env("KIANA_AGENT_MEMORY_SCOPE", agent_memory_scope_name(memory));
            }
            if let Some(permission_mode) = &agent.permission_mode {
                process.env("KIANA_AGENT_PERMISSION_MODE", permission_mode);
            }
            if let Some(max_turns) = agent.max_turns {
                process.env("KIANA_AGENT_MAX_TURNS", max_turns.to_string());
            }
        }
        process.env(
            "KIANA_AGENT_PRELOADED_SKILLS",
            json_string(preloaded_skills)?,
        );
        process.env(
            "KIANA_AGENT_MCP_SERVERS",
            json_string(agent_mcp_servers.unwrap_or(&Value::Null))?,
        );
        if let Some(isolation) = isolation {
            process.env("KIANA_AGENT_ISOLATION", &isolation.mode);
            process.env(
                "KIANA_AGENT_WORKTREE_PATH",
                isolation.path.to_string_lossy().to_string(),
            );
            process.env("KIANA_AGENT_WORKTREE_STRATEGY", &isolation.strategy);
        }
        if let Some(model) =
            normalized_optional(input.model.as_deref()).or_else(|| agent_model(agent))
        {
            process.env("KIANA_AGENT_MODEL", model);
        }
        apply_team_env(&mut process, team_agent);
        return Ok(process);
    }

    let exe = std::env::current_exe().map_err(ToolError::from)?;
    let mut process = Command::new(exe);
    for arg in agent_cli_args(input, agent, prompt, preloaded_skills, agent_mcp_servers) {
        process.arg(arg);
    }
    process.env(
        "KIANA_AGENT_PRELOADED_SKILLS",
        json_string(preloaded_skills)?,
    );
    if let Some(isolation) = isolation {
        process.env("KIANA_AGENT_ISOLATION", &isolation.mode);
        process.env(
            "KIANA_AGENT_WORKTREE_PATH",
            isolation.path.to_string_lossy().to_string(),
        );
        process.env("KIANA_AGENT_WORKTREE_STRATEGY", &isolation.strategy);
    }
    apply_team_env(&mut process, team_agent);
    Ok(process)
}

fn apply_team_env(process: &mut Command, team_agent: Option<&TeamAgentRuntime>) {
    let Some(team_agent) = team_agent else {
        return;
    };
    process
        .env("KIANA_AGENT_ID", &team_agent.agent_id)
        .env("KIANA_AGENT_NAME", &team_agent.agent_name)
        .env("KIANA_AGENT_COLOR", &team_agent.color)
        .env("KIANA_TEAM_NAME", &team_agent.team_name)
        .env("KIANA_TASK_LIST_ID", &team_agent.task_list_id)
        .env(
            "KIANA_TEAM_FILE",
            team_agent.team_file_path.to_string_lossy().to_string(),
        )
        .env(
            "KIANA_TEAM_MAILBOX",
            team_agent.mailbox_path.to_string_lossy().to_string(),
        )
        .env(
            "KIANA_TEAMS_ROOT",
            team_agent.teams_root.to_string_lossy().to_string(),
        )
        .env(
            "KIANA_TASKS_ROOT",
            team_agent.tasks_root.to_string_lossy().to_string(),
        )
        .env(
            "KIANA_PLAN_MODE_REQUIRED",
            if team_agent.plan_mode_required {
                "true"
            } else {
                "false"
            },
        )
        .env("CLAUDE_CODE_AGENT_ID", &team_agent.agent_id)
        .env("CLAUDE_CODE_AGENT_NAME", &team_agent.agent_name)
        .env("CLAUDE_CODE_TEAM_NAME", &team_agent.team_name)
        .env("CLAUDE_CODE_TASK_LIST_ID", &team_agent.task_list_id);
}

fn agent_cli_args(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    prompt: &str,
    preloaded_skills: &[PreloadedSkill],
    agent_mcp_servers: Option<&Value>,
) -> Vec<OsString> {
    let mut args = Vec::new();
    if let Some(model) = normalized_optional(input.model.as_deref()).or_else(|| agent_model(agent))
    {
        args.push("--model".into());
        args.push(model.into());
    }
    if let Some(agent) = agent {
        let agent_type = agent.agent_type.clone();
        let mut agents = serde_json::Map::new();
        let model = normalized_optional(input.model.as_deref())
            .or_else(|| agent.model.clone())
            .unwrap_or_else(|| "inherit".to_string());
        agents.insert(
            agent_type.clone(),
            agent_definition_to_cli_json(agent, model, preloaded_skills, agent_mcp_servers),
        );
        args.push("--agents".into());
        args.push(Value::Object(agents).to_string().into());
        args.push("--agent".into());
        args.push(agent_type.into());
        if let Some(tools) = &agent.tools {
            args.push("--tools".into());
            args.push(tool_filter_value(tools).into());
            if !tools.is_empty() {
                args.push("--allowed-tools".into());
                args.push(join_tool_specs(tools).into());
            }
        }
        if !agent.disallowed_tools.is_empty() {
            args.push("--disallowed-tools".into());
            args.push(join_tool_specs(&agent.disallowed_tools).into());
        }
        if let Some(permission_mode) = &agent.permission_mode {
            args.push("--permission-mode".into());
            args.push(permission_mode.clone().into());
        }
        if let Some(max_turns) = agent.max_turns {
            args.push("--max-turns".into());
            args.push(max_turns.to_string().into());
        }
        if let Some(mcp_servers) = agent_mcp_servers {
            args.push("--mcp-config".into());
            args.push(mcp_servers.to_string().into());
        }
    }
    args.push("-p".into());
    args.push(prompt.into());
    args
}

fn agent_definition_to_cli_json(
    agent: &AgentDefinition,
    model: String,
    preloaded_skills: &[PreloadedSkill],
    agent_mcp_servers: Option<&Value>,
) -> Value {
    let mut value = json!({
        "description": agent.description,
        "prompt": agent.system_prompt,
        "model": model
    });
    if let Some(tools) = &agent.tools {
        value["tools"] = json!(tools);
    }
    if !agent.disallowed_tools.is_empty() {
        value["disallowedTools"] = json!(agent.disallowed_tools);
    }
    if !agent.skills.is_empty() {
        value["skills"] = json!(agent.skills);
    }
    if let Some(memory) = agent.memory {
        value["memory"] = Value::String(agent_memory_scope_name(memory).to_string());
    }
    if let Some(permission_mode) = &agent.permission_mode {
        value["permissionMode"] = Value::String(permission_mode.clone());
    }
    if let Some(max_turns) = agent.max_turns {
        value["maxTurns"] = json!(max_turns);
    }
    if let Some(mcp_servers) = agent_mcp_servers {
        value["mcpServers"] = mcp_servers.clone();
    }
    if !preloaded_skills.is_empty() {
        value["preloadedSkills"] = json!(preloaded_skills);
    }
    if let Some(initial_prompt) = &agent.initial_prompt {
        value["initialPrompt"] = Value::String(initial_prompt.clone());
    }
    value
}

fn agent_model(agent: Option<&AgentDefinition>) -> Option<String> {
    agent
        .and_then(|agent| agent.model.as_deref())
        .and_then(|model| normalized_optional(Some(model)))
        .filter(|model| model != "inherit")
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn parse_isolation(value: Option<&str>) -> Option<String> {
    let value = value?.trim();
    if value.is_empty() {
        return None;
    }
    match value {
        "worktree" => Some("worktree".to_string()),
        _ => None,
    }
}

fn effective_isolation(input: &AgentInput, agent: Option<&AgentDefinition>) -> Option<String> {
    parse_isolation(input.isolation.as_deref())
        .or_else(|| agent.and_then(|agent| parse_isolation(agent.isolation.as_deref())))
}

fn prepare_agent_isolation(
    base_cwd: &Path,
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
) -> ToolResult<Option<AgentIsolation>> {
    let Some(mode) = effective_isolation(input, agent) else {
        return Ok(None);
    };
    if input
        .cwd
        .as_deref()
        .map(str::trim)
        .is_some_and(|cwd| !cwd.is_empty())
    {
        return Err(ToolError::ValidationError(
            "cwd cannot be combined with isolation".to_string(),
        ));
    }
    if mode != "worktree" {
        return Err(ToolError::ValidationError(
            "isolation must be \"worktree\" when provided".to_string(),
        ));
    }

    create_agent_isolation(base_cwd, &mode)
}

fn create_agent_isolation(base_cwd: &Path, mode: &str) -> ToolResult<Option<AgentIsolation>> {
    let slug = format!("agent-{}", Uuid::new_v4().simple());
    if let Ok(isolated) = create_git_worktree_isolation(base_cwd, &slug, mode) {
        return Ok(Some(isolated));
    }
    Ok(Some(create_snapshot_isolation(base_cwd, &slug, mode)?))
}

fn create_git_worktree_isolation(
    base_cwd: &Path,
    slug: &str,
    mode: &str,
) -> ToolResult<AgentIsolation> {
    let repo_root = git_repo_root(base_cwd).ok_or_else(|| {
        ToolError::ValidationError("git worktree isolation requires a git repository".to_string())
    })?;
    if git_head_exists(&repo_root)? {
        let worktree_path = repo_root.join(".claude").join("worktrees").join(slug);
        fs::create_dir_all(worktree_path.parent().unwrap())?;
        let status = std::process::Command::new("git")
            .arg("worktree")
            .arg("add")
            .arg("-d")
            .arg(&worktree_path)
            .arg("HEAD")
            .current_dir(&repo_root)
            .status()?;
        if status.success() {
            return Ok(AgentIsolation {
                mode: mode.to_string(),
                path: worktree_path,
                strategy: "git_worktree".to_string(),
                branch: None,
            });
        }
    }
    Err(ToolError::Other(
        "git worktree isolation unavailable".to_string(),
    ))
}

fn create_snapshot_isolation(
    base_cwd: &Path,
    slug: &str,
    mode: &str,
) -> ToolResult<AgentIsolation> {
    let worktree_path = base_cwd.join(".claude").join("worktrees").join(slug);
    if worktree_path.exists() {
        fs::remove_dir_all(&worktree_path)?;
    }
    fs::create_dir_all(&worktree_path)?;
    copy_directory_filtered(base_cwd, &worktree_path, &worktree_path)?;
    Ok(AgentIsolation {
        mode: mode.to_string(),
        path: worktree_path,
        strategy: "snapshot_copy".to_string(),
        branch: None,
    })
}

fn copy_directory_filtered(src: &Path, dst: &Path, root: &Path) -> ToolResult<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if should_skip_isolation_entry(src, &path, root, &name) {
            continue;
        }
        let target = dst.join(&file_name);
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            fs::create_dir_all(&target)?;
            copy_directory_filtered(&path, &target, root)?;
        } else if file_type.is_file() {
            fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

fn should_skip_isolation_entry(
    current_src: &Path,
    entry_path: &Path,
    root: &Path,
    name: &str,
) -> bool {
    if name == ".git" || name == "target" {
        return true;
    }
    if let Ok(rel) = entry_path.strip_prefix(root) {
        let mut components = rel.components();
        if let Some(first) = components.next().and_then(|c| c.as_os_str().to_str()) {
            if first == ".claude" {
                if let Some(second) = components.next().and_then(|c| c.as_os_str().to_str()) {
                    if second == "worktrees" {
                        return true;
                    }
                }
            }
            if first == ".kiana" {
                if let Some(second) = components.next().and_then(|c| c.as_os_str().to_str()) {
                    if second == "worktrees" {
                        return true;
                    }
                }
            }
        }
    }
    if current_src.ends_with(".claude") && name == "worktrees" {
        return true;
    }
    if current_src.ends_with(".kiana") && name == "worktrees" {
        return true;
    }
    false
}

fn git_repo_root(cwd: &Path) -> Option<PathBuf> {
    let output = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn git_head_exists(cwd: &Path) -> ToolResult<bool> {
    let status = std::process::Command::new("git")
        .arg("rev-parse")
        .arg("--verify")
        .arg("HEAD")
        .current_dir(cwd)
        .status()?;
    Ok(status.success())
}

fn json_string<T: Serialize + ?Sized>(value: &T) -> ToolResult<String> {
    Ok(serde_json::to_string(value)?)
}

async fn preload_agent_skills(
    agent: Option<&AgentDefinition>,
    cwd: &Path,
    app_state: &HashMap<String, Value>,
) -> Vec<PreloadedSkill> {
    let Some(agent) = agent else {
        return Vec::new();
    };
    if agent.skills.is_empty() {
        return Vec::new();
    }

    let commands =
        kiana_skills::load_all_skills_with_trust(cwd, project_trust_from_app_state(app_state))
            .await;
    let mut preloaded = Vec::new();
    for requested in &agent.skills {
        let Some(command) = resolve_agent_skill(requested, &commands) else {
            continue;
        };
        if command.disable_model_invocation {
            continue;
        }
        preloaded.push(PreloadedSkill {
            name: command.name.clone(),
            path: skill_path(command),
            content: command.content.clone(),
        });
    }
    preloaded
}

fn resolve_agent_skill<'a>(
    requested: &str,
    commands: &'a [kiana_skills::Command],
) -> Option<&'a kiana_skills::Command> {
    let normalized = requested.trim().trim_start_matches('/').trim();
    if normalized.is_empty() {
        return None;
    }
    kiana_skills::find_command(normalized, commands).or_else(|| {
        commands.iter().find(|command| {
            command
                .name
                .rsplit_once(':')
                .map(|(_, suffix)| suffix.eq_ignore_ascii_case(normalized))
                .unwrap_or(false)
        })
    })
}

fn skill_path(command: &kiana_skills::Command) -> String {
    command
        .skill_root
        .as_ref()
        .map(|path| path.join("SKILL.md").to_string_lossy().to_string())
        .unwrap_or_default()
}

fn prompt_with_preloaded_skills(prompt: &str, skills: &[PreloadedSkill]) -> String {
    if skills.is_empty() {
        return prompt.to_string();
    }

    let mut content = String::new();
    content.push_str("# Preloaded Skills\n\n");
    for skill in skills {
        content.push_str("### Skill: ");
        content.push_str(&skill.name);
        content.push('\n');
        content.push_str("Path: ");
        content.push_str(&skill.path);
        content.push_str("\n\n");
        content.push_str(skill.content.trim());
        content.push_str("\n\n");
    }
    content.push_str("# Task\n\n");
    content.push_str(prompt);
    content
}

fn prompt_with_hook_context(prompt: &str, contexts: &[String]) -> String {
    if contexts.is_empty() {
        return prompt.to_string();
    }
    let mut content = String::new();
    content.push_str("# Hook Additional Context\n\n");
    for context in contexts {
        content.push_str(context.trim());
        content.push_str("\n\n");
    }
    content.push_str("# Task\n\n");
    content.push_str(prompt);
    content
}

fn parse_agent_tools(value: Option<&serde_yaml::Value>) -> Option<Vec<String>> {
    let tools = parse_tool_list(value)?;
    if tools.iter().any(|tool| tool == "*") {
        None
    } else {
        Some(tools)
    }
}

fn parse_agent_lifecycle_hooks(
    value: Option<&serde_yaml::Value>,
    plugin_root: Option<&Path>,
) -> Vec<AgentLifecycleHook> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(events) = value.as_mapping() else {
        return Vec::new();
    };

    let mut hooks = Vec::new();
    for (event_key, event_value) in events {
        let Some(event_name) = yaml_string(event_key) else {
            continue;
        };
        let Some(event) = parse_agent_lifecycle_event(event_name) else {
            continue;
        };
        parse_agent_lifecycle_event_hooks(event, event_value, plugin_root, &mut hooks);
    }
    hooks
}

fn parse_agent_lifecycle_event_hooks(
    event: AgentLifecycleEvent,
    value: &serde_yaml::Value,
    plugin_root: Option<&Path>,
    hooks: &mut Vec<AgentLifecycleHook>,
) {
    match value {
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                parse_agent_lifecycle_matcher(event, item, plugin_root, hooks);
            }
        }
        _ => parse_agent_lifecycle_matcher(event, value, plugin_root, hooks),
    }
}

fn parse_agent_lifecycle_matcher(
    event: AgentLifecycleEvent,
    value: &serde_yaml::Value,
    plugin_root: Option<&Path>,
    hooks: &mut Vec<AgentLifecycleHook>,
) {
    if let Some(hook) = parse_agent_lifecycle_command(event, None, value, plugin_root) {
        hooks.push(hook);
        return;
    }

    let Some(mapping) = value.as_mapping() else {
        return;
    };
    let matcher = mapping
        .get(&serde_yaml::Value::String("matcher".to_string()))
        .and_then(yaml_string)
        .map(str::to_string);
    let Some(hook_values) = mapping
        .get(&serde_yaml::Value::String("hooks".to_string()))
        .or_else(|| mapping.get(&serde_yaml::Value::String("commands".to_string())))
    else {
        return;
    };
    match hook_values {
        serde_yaml::Value::Sequence(items) => {
            for item in items {
                if let Some(hook) =
                    parse_agent_lifecycle_command(event, matcher.clone(), item, plugin_root)
                {
                    hooks.push(hook);
                }
            }
        }
        _ => {
            if let Some(hook) =
                parse_agent_lifecycle_command(event, matcher, hook_values, plugin_root)
            {
                hooks.push(hook);
            }
        }
    }
}

fn parse_agent_lifecycle_command(
    event: AgentLifecycleEvent,
    matcher: Option<String>,
    value: &serde_yaml::Value,
    plugin_root: Option<&Path>,
) -> Option<AgentLifecycleHook> {
    match value {
        serde_yaml::Value::String(command) => Some(AgentLifecycleHook {
            event,
            matcher: clean_optional_string(matcher),
            command: substitute_hook_plugin_variables(command.trim(), plugin_root),
            timeout_ms: None,
        }),
        serde_yaml::Value::Mapping(mapping) => {
            let hook_type = mapping
                .get(&serde_yaml::Value::String("type".to_string()))
                .and_then(yaml_string)
                .unwrap_or("command");
            if hook_type != "command" {
                return None;
            }
            let command = mapping
                .get(&serde_yaml::Value::String("command".to_string()))
                .and_then(yaml_string)
                .map(str::trim)
                .filter(|command| !command.is_empty())?;
            let timeout_ms = mapping
                .get(&serde_yaml::Value::String("timeout".to_string()))
                .and_then(yaml_positive_seconds_to_ms);
            Some(AgentLifecycleHook {
                event,
                matcher: clean_optional_string(matcher),
                command: substitute_hook_plugin_variables(command, plugin_root),
                timeout_ms,
            })
        }
        _ => None,
    }
}

fn parse_agent_lifecycle_event(value: &str) -> Option<AgentLifecycleEvent> {
    match value {
        "SubagentStart" | "subagentStart" | "subagent-start" | "subagent_start" => {
            Some(AgentLifecycleEvent::SubagentStart)
        }
        "SubagentStop" | "subagentStop" | "subagent-stop" | "subagent_stop" => {
            Some(AgentLifecycleEvent::SubagentStop)
        }
        "Stop" | "stop" => Some(AgentLifecycleEvent::Stop),
        _ => None,
    }
}

fn yaml_string(value: &serde_yaml::Value) -> Option<&str> {
    value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn yaml_positive_seconds_to_ms(value: &serde_yaml::Value) -> Option<u64> {
    match value {
        serde_yaml::Value::Number(number) => number
            .as_f64()
            .filter(|seconds| *seconds > 0.0)
            .map(|seconds| (seconds * 1000.0).round() as u64),
        serde_yaml::Value::String(value) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|seconds| *seconds > 0.0)
            .map(|seconds| (seconds * 1000.0).round() as u64),
        _ => None,
    }
}

fn clean_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn substitute_hook_plugin_variables(command: &str, plugin_root: Option<&Path>) -> String {
    let Some(plugin_root) = plugin_root else {
        return command.to_string();
    };
    let plugin_root = plugin_root.to_string_lossy();
    command
        .replace("${CLAUDE_PLUGIN_ROOT}", &plugin_root)
        .replace("${KIANA_PLUGIN_ROOT}", &plugin_root)
}

fn parse_tool_list(value: Option<&serde_yaml::Value>) -> Option<Vec<String>> {
    let value = value?;
    let mut tools = Vec::new();
    match value {
        serde_yaml::Value::Null => {}
        serde_yaml::Value::String(value) => extend_tool_tokens(&mut tools, value),
        serde_yaml::Value::Sequence(values) => {
            for value in values {
                if let serde_yaml::Value::String(value) = value {
                    extend_tool_tokens(&mut tools, value);
                }
            }
        }
        _ => {}
    }
    Some(tools)
}

fn parse_agent_memory_scope(value: Option<String>) -> Option<AgentMemoryScope> {
    match value?.trim() {
        "user" => Some(AgentMemoryScope::User),
        "project" => Some(AgentMemoryScope::Project),
        "local" => Some(AgentMemoryScope::Local),
        _ => None,
    }
}

fn agent_memory_scope_name(scope: AgentMemoryScope) -> &'static str {
    match scope {
        AgentMemoryScope::User => "user",
        AgentMemoryScope::Project => "project",
        AgentMemoryScope::Local => "local",
    }
}

fn inject_agent_memory_tools(tools: &mut Option<Vec<String>>, memory: Option<AgentMemoryScope>) {
    if memory.is_none() || !is_agent_memory_enabled() {
        return;
    }
    let Some(tools) = tools else {
        return;
    };
    for tool in ["Write", "Edit", "Read"] {
        if !tools
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(tool))
        {
            tools.push(tool.to_string());
        }
    }
}

fn system_prompt_with_agent_memory(
    system_prompt: &str,
    agent_type: &str,
    memory: Option<AgentMemoryScope>,
    cwd: &Path,
) -> String {
    let system_prompt = system_prompt.trim();
    let Some(memory) = memory else {
        return system_prompt.to_string();
    };
    if !is_agent_memory_enabled() {
        return system_prompt.to_string();
    }
    let memory_prompt = build_agent_memory_prompt(agent_type, memory, cwd);
    if memory_prompt.trim().is_empty() {
        system_prompt.to_string()
    } else {
        format!("{system_prompt}\n\n{memory_prompt}")
    }
}

fn is_agent_memory_enabled() -> bool {
    if env_truthy("CLAUDE_CODE_DISABLE_AUTO_MEMORY") {
        return false;
    }
    if env_defined_falsy("CLAUDE_CODE_DISABLE_AUTO_MEMORY") {
        return true;
    }
    !(env_truthy("CLAUDE_CODE_SIMPLE") || env_truthy("KIANA_CODE_SIMPLE"))
}

fn env_truthy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn env_defined_falsy(key: &str) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "no" | "off"
            )
        })
        .unwrap_or(false)
}

fn build_agent_memory_prompt(agent_type: &str, scope: AgentMemoryScope, cwd: &Path) -> String {
    let memory_dir = agent_memory_dir(agent_type, scope, cwd);
    let _ = fs::create_dir_all(&memory_dir);
    let entrypoint = memory_dir.join("MEMORY.md");
    let entrypoint_content = fs::read_to_string(&entrypoint).unwrap_or_default();
    let scope_note = match scope {
        AgentMemoryScope::User => {
            "Since this memory is user-scope, keep learnings general across projects."
        }
        AgentMemoryScope::Project => {
            "Since this memory is project-scope, tailor memories to this project and keep team-shared guidance useful."
        }
        AgentMemoryScope::Local => {
            "Since this memory is local-scope, tailor memories to this project and machine; do not assume it is version-controlled."
        }
    };
    let mut prompt = format!(
        "# Persistent Agent Memory\n\nYou have a persistent, file-based memory system at `{}`.\n{scope_note}\n\nUse `MEMORY.md` as the concise index. Save detailed memories as separate files in this directory, then add short links or bullets to `MEMORY.md`. Do not save facts already documented in CLAUDE.md or derivable from the current codebase.\n\n## MEMORY.md\n\n",
        memory_dir.to_string_lossy()
    );
    if entrypoint_content.trim().is_empty() {
        prompt.push_str(
            "Your MEMORY.md is currently empty. When you save new memories, they will appear here.",
        );
    } else {
        prompt.push_str(entrypoint_content.trim());
    }
    prompt
}

fn agent_memory_dir(agent_type: &str, scope: AgentMemoryScope, cwd: &Path) -> PathBuf {
    let dir_name = sanitize_agent_memory_dir(agent_type);
    match scope {
        AgentMemoryScope::Project => cwd.join(".claude").join("agent-memory").join(dir_name),
        AgentMemoryScope::Local => {
            if let Ok(remote_memory_dir) = std::env::var("CLAUDE_CODE_REMOTE_MEMORY_DIR") {
                return PathBuf::from(remote_memory_dir)
                    .join("projects")
                    .join(sanitize_agent_memory_dir(&cwd.to_string_lossy()))
                    .join("agent-memory-local")
                    .join(dir_name);
            }
            cwd.join(".claude")
                .join("agent-memory-local")
                .join(dir_name)
        }
        AgentMemoryScope::User => agent_memory_base_dir().join("agent-memory").join(dir_name),
    }
}

fn agent_memory_base_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CODE_REMOTE_MEMORY_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from))
        .or_else(|| {
            std::env::var_os("KIANA_HOME")
                .map(PathBuf::from)
                .map(|path| path.join("claude-memory"))
        })
        .or_else(|| home_dir().map(|home| home.join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
}

fn sanitize_agent_memory_dir(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "agent".to_string()
    } else {
        sanitized
    }
}

fn parse_mcp_servers(value: Option<&serde_yaml::Value>) -> Option<Value> {
    serde_json::to_value(value?).ok()
}

fn resolve_agent_mcp_servers(
    agent: Option<&AgentDefinition>,
    context: &ToolContext,
) -> Option<Value> {
    let specs = agent?.mcp_servers.as_ref()?;
    let parent = parent_mcp_servers(context);
    let mut merged = serde_json::Map::new();
    merge_mcp_server_specs(specs, parent.as_ref(), &mut merged);
    if merged.is_empty() {
        None
    } else {
        Some(Value::Object(merged))
    }
}

fn parent_mcp_servers(context: &ToolContext) -> Option<Value> {
    context
        .app_state
        .get(MCP_SERVERS_APP_STATE_KEY)
        .or_else(|| context.app_state.get("mcpServers"))
        .cloned()
        .or_else(|| {
            std::env::var(MCP_SERVERS_ENV)
                .ok()
                .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        })
        .and_then(normalized_mcp_servers_object)
}

fn normalized_mcp_servers_object(value: Value) -> Option<Value> {
    if let Some(servers) = value
        .get("mcpServers")
        .or_else(|| value.get(MCP_SERVERS_APP_STATE_KEY))
    {
        return normalized_mcp_servers_object(servers.clone());
    }
    if value.as_object().is_some() {
        Some(value)
    } else {
        None
    }
}

fn merge_mcp_server_specs(
    specs: &Value,
    parent: Option<&Value>,
    merged: &mut serde_json::Map<String, Value>,
) {
    match specs {
        Value::Array(items) => {
            for item in items {
                merge_mcp_server_specs(item, parent, merged);
            }
        }
        Value::String(name) => {
            if let Some(config) = parent
                .and_then(Value::as_object)
                .and_then(|servers| servers.get(name))
            {
                merged.insert(name.clone(), config.clone());
            }
        }
        Value::Object(object) => {
            if let Some(servers) = object
                .get("mcpServers")
                .or_else(|| object.get(MCP_SERVERS_APP_STATE_KEY))
            {
                merge_mcp_server_specs(servers, parent, merged);
            } else {
                for (name, config) in object {
                    if !name.trim().is_empty() {
                        merged.insert(name.clone(), config.clone());
                    }
                }
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone)]
struct AgentLifecycleContext<'a> {
    agent_id: &'a str,
    agent_type: &'a str,
    prompt: &'a str,
    cwd: &'a Path,
    status: Option<&'a str>,
    exit_code: Option<i32>,
    stdout: Option<&'a str>,
    stderr: Option<&'a str>,
    project_trust: ProjectTrust,
}

async fn run_agent_lifecycle_hooks(
    agent: Option<&AgentDefinition>,
    event: AgentLifecycleEvent,
    lifecycle: AgentLifecycleContext<'_>,
) -> ToolResult<AgentLifecycleHookOutcome> {
    let hooks = agent_lifecycle_hooks_for_event(
        agent,
        event,
        lifecycle.agent_type,
        lifecycle.project_trust,
        lifecycle.cwd,
    );
    if hooks.is_empty() {
        return Ok(AgentLifecycleHookOutcome::default());
    }

    let mut outcome = AgentLifecycleHookOutcome::default();
    let input_json = agent_lifecycle_hook_input(event, &lifecycle);
    for hook in hooks {
        let result = run_agent_lifecycle_hook(&hook, &input_json, lifecycle.cwd).await?;
        if let Some(context) = result
            .additional_context
            .as_deref()
            .map(str::trim)
            .filter(|context| !context.is_empty())
        {
            outcome.additional_contexts.push(context.to_string());
        }
        if let Some(error) = result
            .blocking_error
            .as_deref()
            .map(str::trim)
            .filter(|error| !error.is_empty())
        {
            outcome.blocking_errors.push(error.to_string());
        }
        outcome.results.push(result);
    }

    if !outcome.blocking_errors.is_empty() {
        return Err(ToolError::Other(format!(
            "Agent lifecycle hook blocked: {}",
            outcome.blocking_errors.join("\n")
        )));
    }
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
async fn stop_agent_lifecycle_hooks_for_output(
    agent: Option<&AgentDefinition>,
    agent_id: &str,
    agent_type: &str,
    prompt: &str,
    cwd: &Path,
    status: &str,
    exit_code: i32,
    stdout: &str,
    stderr: &str,
    start_hook_results: Vec<AgentLifecycleHookResult>,
    project_trust: ProjectTrust,
) -> AgentLifecycleHookOutcome {
    let mut outcome = AgentLifecycleHookOutcome {
        results: start_hook_results,
        additional_contexts: Vec::new(),
        blocking_errors: Vec::new(),
    };
    match run_agent_lifecycle_hooks(
        agent,
        AgentLifecycleEvent::SubagentStop,
        AgentLifecycleContext {
            agent_id,
            agent_type,
            prompt,
            cwd,
            status: Some(status),
            exit_code: Some(exit_code),
            stdout: Some(stdout),
            stderr: Some(stderr),
            project_trust,
        },
    )
    .await
    {
        Ok(stop) => {
            outcome.results.extend(stop.results);
            outcome.additional_contexts.extend(stop.additional_contexts);
            outcome.blocking_errors.extend(stop.blocking_errors);
        }
        Err(error) => outcome.blocking_errors.push(error.to_string()),
    }
    outcome
}

fn agent_lifecycle_hooks_for_event(
    agent: Option<&AgentDefinition>,
    event: AgentLifecycleEvent,
    agent_type: &str,
    project_trust: ProjectTrust,
    cwd: &Path,
) -> Vec<AgentLifecycleHook> {
    let mut hooks = global_agent_lifecycle_hooks(event, project_trust, cwd);
    if let Some(agent) = agent {
        hooks.extend(agent.hooks.iter().filter_map(|hook| {
            if agent_hook_event_matches(hook.event, event)
                && agent_hook_matcher_matches(hook.matcher.as_deref(), agent_type)
            {
                Some(hook.clone())
            } else {
                None
            }
        }));
    }
    hooks
}

fn agent_hook_event_matches(hook_event: AgentLifecycleEvent, target: AgentLifecycleEvent) -> bool {
    hook_event == target
        || (target == AgentLifecycleEvent::SubagentStop && hook_event == AgentLifecycleEvent::Stop)
}

fn agent_hook_matcher_matches(matcher: Option<&str>, agent_type: &str) -> bool {
    let Some(matcher) = matcher.map(str::trim).filter(|matcher| !matcher.is_empty()) else {
        return true;
    };
    if matcher == "*" {
        return true;
    }
    if matcher.split('|').any(|part| {
        let part = part.trim();
        !part.is_empty() && part.eq_ignore_ascii_case(agent_type)
    }) {
        return true;
    }
    regex::Regex::new(matcher)
        .map(|regex| regex.is_match(agent_type))
        .unwrap_or(false)
}

async fn run_agent_lifecycle_hook(
    hook: &AgentLifecycleHook,
    input_json: &str,
    cwd: &Path,
) -> ToolResult<AgentLifecycleHookResult> {
    let timeout_ms = hook
        .timeout_ms
        .unwrap_or_else(agent_lifecycle_hook_timeout_ms);
    let started = Instant::now();

    let output = tokio::time::timeout(Duration::from_millis(timeout_ms), async {
        let mut process = lifecycle_shell_command(&hook.command);
        process
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = process.spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(input_json.as_bytes()).await?;
        }
        child.wait_with_output().await
    })
    .await
    .map_err(|_| {
        ToolError::Other(format!(
            "Agent lifecycle hook timed out after {timeout_ms}ms: {}",
            hook.command
        ))
    })??;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let exit_code = output.status.code();
    let mut parsed = parse_agent_lifecycle_hook_output(&stdout, hook.event);
    if !output.status.success() {
        parsed.blocking_error.get_or_insert_with(|| {
            let detail = if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            };
            if detail.is_empty() {
                format!("Hook exited with {}", exit_code.unwrap_or(-1))
            } else {
                format!("Hook exited with {}: {detail}", exit_code.unwrap_or(-1))
            }
        });
    }

    Ok(AgentLifecycleHookResult {
        event: agent_lifecycle_event_name(hook.event).to_string(),
        command: hook.command.clone(),
        exit_code,
        stdout,
        stderr,
        additional_context: parsed.additional_context,
        blocking_error: parsed.blocking_error,
        duration_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
    })
}

#[derive(Debug, Default)]
struct ParsedAgentLifecycleHookOutput {
    additional_context: Option<String>,
    blocking_error: Option<String>,
}

fn parse_agent_lifecycle_hook_output(
    stdout: &str,
    expected_event: AgentLifecycleEvent,
) -> ParsedAgentLifecycleHookOutput {
    let Ok(value) = serde_json::from_str::<Value>(stdout.trim()) else {
        return ParsedAgentLifecycleHookOutput::default();
    };
    let additional_context = value
        .get("hookSpecificOutput")
        .and_then(|output| {
            let event_matches = output
                .get("hookEventName")
                .and_then(Value::as_str)
                .map(|event| event == agent_lifecycle_event_name(expected_event))
                .unwrap_or(true);
            if event_matches {
                output.get("additionalContext").and_then(Value::as_str)
            } else {
                None
            }
        })
        .or_else(|| value.get("additionalContext").and_then(Value::as_str))
        .or_else(|| value.get("additional_context").and_then(Value::as_str))
        .map(str::to_string);
    let blocking_error = if value.get("decision").and_then(Value::as_str) == Some("block") {
        value
            .get("reason")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| Some("Blocked by hook".to_string()))
    } else if value.get("continue").and_then(Value::as_bool) == Some(false) {
        value
            .get("stopReason")
            .or_else(|| value.get("stop_reason"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| Some("Hook prevented continuation".to_string()))
    } else {
        value
            .get("blocking_error")
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    ParsedAgentLifecycleHookOutput {
        additional_context,
        blocking_error,
    }
}

fn agent_lifecycle_hook_input(
    event: AgentLifecycleEvent,
    lifecycle: &AgentLifecycleContext<'_>,
) -> String {
    let mut input = json!({
        "hook_event_name": agent_lifecycle_event_name(event),
        "agent_id": lifecycle.agent_id,
        "agent_type": lifecycle.agent_type,
        "prompt": lifecycle.prompt,
        "cwd": lifecycle.cwd.to_string_lossy(),
    });
    if let Some(status) = lifecycle.status {
        input["status"] = json!(status);
    }
    if let Some(exit_code) = lifecycle.exit_code {
        input["exit_code"] = json!(exit_code);
    }
    if let Some(stdout) = lifecycle.stdout {
        input["last_assistant_message"] = json!(stdout);
        input["stdout"] = json!(stdout);
    }
    if let Some(stderr) = lifecycle.stderr {
        input["stderr"] = json!(stderr);
    }
    input.to_string()
}

fn global_agent_lifecycle_hooks(
    event: AgentLifecycleEvent,
    project_trust: ProjectTrust,
    cwd: &Path,
) -> Vec<AgentLifecycleHook> {
    let mut hooks = Vec::new();
    if let Some(commands) = env_agent_lifecycle_commands(event) {
        hooks.extend(commands.into_iter().map(|command| AgentLifecycleHook {
            event,
            matcher: None,
            command,
            timeout_ms: None,
        }));
        return hooks;
    }

    hooks.extend(
        file_agent_lifecycle_commands(event, project_trust, cwd)
            .into_iter()
            .map(|command| AgentLifecycleHook {
                event,
                matcher: None,
                command,
                timeout_ms: None,
            }),
    );
    hooks
}

fn env_agent_lifecycle_commands(event: AgentLifecycleEvent) -> Option<Vec<String>> {
    let env_name = match event {
        AgentLifecycleEvent::SubagentStart => "KIANA_SUBAGENT_START_HOOKS",
        AgentLifecycleEvent::SubagentStop | AgentLifecycleEvent::Stop => {
            "KIANA_SUBAGENT_STOP_HOOKS"
        }
    };
    env::var(env_name)
        .ok()
        .or_else(|| env::var("KIANA_AGENT_HOOKS").ok())
        .map(|value| parse_hook_command_list(&value))
}

fn file_agent_lifecycle_commands(
    event: AgentLifecycleEvent,
    project_trust: ProjectTrust,
    cwd: &Path,
) -> Vec<String> {
    let mut commands = Vec::new();
    for path in agent_lifecycle_hooks_file_paths(project_trust, cwd) {
        let Some(mut file_commands) = fs::read_to_string(path)
            .ok()
            .and_then(|contents| serde_json::from_str::<Value>(&contents).ok())
            .map(|value| hook_commands_from_json_value(&value, agent_lifecycle_file_keys(event)))
        else {
            continue;
        };
        commands.append(&mut file_commands);
    }
    commands
}

fn agent_lifecycle_hooks_file_paths(project_trust: ProjectTrust, cwd: &Path) -> Vec<PathBuf> {
    if let Ok(path) = env::var("KIANA_HOOKS_FILE") {
        return vec![PathBuf::from(path)];
    }
    let mut paths = Vec::new();
    if let Ok(home) = env::var("KIANA_HOME") {
        paths.push(PathBuf::from(home).join("hooks.json"));
    } else if let Some(home) = home_dir() {
        paths.push(home.join(".kiana").join("hooks.json"));
    }
    if project_trust.allows_project_resources() {
        let project_path = cwd.join(".kiana").join("hooks.json");
        if !paths.contains(&project_path) {
            paths.push(project_path);
        }
    }
    paths
}

fn hook_commands_from_json_value(value: &Value, keys: &[&str]) -> Vec<String> {
    if value.is_array() || value.is_string() {
        return parse_hook_command_value(value);
    }
    let Some(object) = value.as_object() else {
        return Vec::new();
    };
    for key in keys {
        if let Some(value) = object.get(*key) {
            return parse_hook_command_value(value);
        }
    }
    Vec::new()
}

fn parse_hook_command_list(value: &str) -> Vec<String> {
    if let Ok(value) = serde_json::from_str::<Value>(value) {
        return parse_hook_command_value(&value);
    }
    value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_hook_command_value(value: &Value) -> Vec<String> {
    if let Some(command) = value.as_str() {
        return vec![command.trim().to_string()]
            .into_iter()
            .filter(|command| !command.is_empty())
            .collect();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|command| !command.is_empty())
        .map(str::to_string)
        .collect()
}

fn agent_lifecycle_file_keys(event: AgentLifecycleEvent) -> &'static [&'static str] {
    match event {
        AgentLifecycleEvent::SubagentStart => &[
            "SubagentStart",
            "subagentStart",
            "subagent_start",
            "subagent-start",
            "KIANA_SUBAGENT_START_HOOKS",
        ],
        AgentLifecycleEvent::SubagentStop | AgentLifecycleEvent::Stop => &[
            "SubagentStop",
            "subagentStop",
            "subagent_stop",
            "subagent-stop",
            "Stop",
            "KIANA_SUBAGENT_STOP_HOOKS",
        ],
    }
}

fn lifecycle_shell_command(command: &str) -> Command {
    #[cfg(windows)]
    {
        let mut shell = Command::new(preferred_bash_program());
        shell.arg("-lc").arg(command);
        shell
    }
    #[cfg(not(windows))]
    {
        let program = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let mut shell = Command::new(program);
        shell.arg("-lc").arg(command);
        shell
    }
}

fn agent_lifecycle_hook_timeout_ms() -> u64 {
    env::var("KIANA_AGENT_HOOK_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30_000)
}

fn agent_lifecycle_event_name(event: AgentLifecycleEvent) -> &'static str {
    match event {
        AgentLifecycleEvent::SubagentStart => "SubagentStart",
        AgentLifecycleEvent::SubagentStop | AgentLifecycleEvent::Stop => "SubagentStop",
    }
}

fn parse_permission_mode(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_string();
    if value.is_empty() {
        return None;
    }
    let normalized = match value.as_str() {
        "acceptEdits" | "accept-edits" | "accept_edits" => "acceptEdits",
        "bypassPermissions" | "bypass-permissions" | "bypass_permissions" => "bypassPermissions",
        "dontAsk" | "dont-ask" | "dont_ask" => "dontAsk",
        "default" => "default",
        "plan" => "plan",
        "auto" => "auto",
        "ask" => "ask",
        _ => return None,
    };
    Some(normalized.to_string())
}

fn parse_positive_u64(value: Option<&serde_yaml::Value>) -> Option<u64> {
    match value? {
        serde_yaml::Value::Number(number) => number.as_u64().filter(|value| *value > 0),
        serde_yaml::Value::String(value) => {
            value.trim().parse::<u64>().ok().filter(|value| *value > 0)
        }
        _ => None,
    }
}

fn extend_tool_tokens(target: &mut Vec<String>, value: &str) {
    for token in value
        .split(|ch: char| ch == ',' || ch.is_ascii_whitespace())
        .map(str::trim)
        .filter(|token| !token.is_empty())
    {
        if !target
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(token))
        {
            target.push(token.to_string());
        }
    }
}

fn tool_filter_value(tools: &[String]) -> String {
    let mut names = Vec::new();
    for tool in tools {
        let name = tool_name_from_rule(tool);
        if !names
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&name))
        {
            names.push(name);
        }
    }
    names.join(",")
}

fn join_tool_specs(tools: &[String]) -> String {
    tools.join(",")
}

fn tool_name_from_rule(rule: &str) -> String {
    rule.split_once('(')
        .map(|(name, _)| name.trim())
        .unwrap_or_else(|| rule.trim())
        .to_string()
}

fn spawn_background_agent(
    input: AgentInput,
    agent: Option<AgentDefinition>,
    context: &mut ToolContext,
    prompt: String,
    runner_prompt: String,
    description: String,
    run_cwd: PathBuf,
    preloaded_skills: Vec<PreloadedSkill>,
    agent_mcp_servers: Option<Value>,
    isolation: Option<AgentIsolation>,
    team_agent: Option<TeamAgentRuntime>,
) -> ToolResult<ToolOutput> {
    let root = background_root(context);
    ensure_background_dirs(&root)?;
    context.app_state.insert(
        BACKGROUND_ROOT_KEY.to_string(),
        Value::String(root.to_string_lossy().to_string()),
    );

    let task_id = Uuid::new_v4().to_string();
    let prompt_path = root.join("prompts").join(format!("{task_id}.txt"));
    let runner_path = root.join("scripts").join(format!("{task_id}.agent.sh"));
    let log_path = root.join("logs").join(format!("{task_id}.log"));
    let exit_path = root.join("exits").join(format!("{task_id}.exit"));
    let task_path = root.join("tasks").join(format!("{task_id}.json"));
    let now = now_unix_seconds();

    fs::write(&prompt_path, &runner_prompt)?;
    fs::write(
        &runner_path,
        agent_runner_script(
            &input,
            agent.as_ref(),
            &prompt_path,
            &log_path,
            &exit_path,
            &preloaded_skills,
            agent_mcp_servers.as_ref(),
            isolation.as_ref(),
            team_agent.as_ref(),
        )?,
    )?;
    append_log(
        &log_path,
        &format!("created background agent task {task_id}\n"),
    )?;

    let mut process = background_agent_process(&runner_path);
    let child = process
        .current_dir(&run_cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let task = json!({
        "id": task_id,
        "agent_id": task_id,
        "description": description,
        "prompt": prompt,
        "cwd": run_cwd.to_string_lossy().to_string(),
        "status": "running",
        "pid": child.id(),
        "created_at": now,
        "updated_at": now,
        "exit_code": Value::Null,
        "error": Value::Null,
        "task_type": "background_agent",
        "subagent_type": agent
            .as_ref()
            .map(|agent| agent.agent_type.clone())
            .or(input.subagent_type),
        "model": normalized_optional(input.model.as_deref())
            .or_else(|| agent_model(agent.as_ref())),
        "agent_definition_path": agent
            .as_ref()
            .and_then(|agent| agent.path.as_ref())
            .map(|path| path.to_string_lossy().to_string()),
        "agent_initial_prompt": agent
            .as_ref()
            .and_then(|agent| agent.initial_prompt.clone()),
        "agent_tools": agent
            .as_ref()
            .and_then(|agent| agent.tools.clone()),
        "agent_disallowed_tools": agent
            .as_ref()
            .map(|agent| agent.disallowed_tools.clone())
            .unwrap_or_default(),
        "agent_skills": agent
            .as_ref()
            .map(|agent| agent.skills.clone())
            .unwrap_or_default(),
        "agent_permission_mode": agent
            .as_ref()
            .and_then(|agent| agent.permission_mode.clone()),
        "agent_max_turns": agent
            .as_ref()
            .and_then(|agent| agent.max_turns),
        "agent_preloaded_skills": preloaded_skills,
        "agent_mcp_servers": agent_mcp_servers,
        "team_name": team_agent.as_ref().map(|team| team.team_name.clone()),
        "teamName": team_agent.as_ref().map(|team| team.team_name.clone()),
        "teammate_id": team_agent.as_ref().map(|team| team.agent_id.clone()),
        "teammateId": team_agent.as_ref().map(|team| team.agent_id.clone()),
        "agent_name": team_agent.as_ref().map(|team| team.agent_name.clone()),
        "agentName": team_agent.as_ref().map(|team| team.agent_name.clone()),
        "task_list_id": team_agent.as_ref().map(|team| team.task_list_id.clone()),
        "taskListId": team_agent.as_ref().map(|team| team.task_list_id.clone()),
        "team_file_path": team_agent.as_ref().map(|team| team.team_file_path.to_string_lossy().to_string()),
        "mailbox_path": team_agent.as_ref().map(|team| team.mailbox_path.to_string_lossy().to_string()),
        "plan_mode_required": team_agent.as_ref().map(|team| team.plan_mode_required),
        "planModeRequired": team_agent.as_ref().map(|team| team.plan_mode_required),
        "isolation": isolation.as_ref().map(|isolation| isolation.mode.clone()),
        "worktree_path": isolation.as_ref().map(|isolation| isolation.path.to_string_lossy().to_string()),
        "worktree_strategy": isolation.as_ref().map(|isolation| isolation.strategy.clone()),
        "worktree_branch": isolation.as_ref().and_then(|isolation| isolation.branch.clone()),
        "output_file": log_path.to_string_lossy().to_string()
    });
    fs::write(&task_path, serde_json::to_string_pretty(&task)?)?;

    Ok(ToolOutput {
        data: json!({
            "status": "async_launched",
            "agentId": task_id,
            "task_id": task_id,
            "description": description,
            "prompt": prompt,
            "cwd": run_cwd.to_string_lossy(),
            "team_name": team_agent.as_ref().map(|team| team.team_name.clone()),
            "teamName": team_agent.as_ref().map(|team| team.team_name.clone()),
            "teammate_id": team_agent.as_ref().map(|team| team.agent_id.clone()),
            "teammateId": team_agent.as_ref().map(|team| team.agent_id.clone()),
            "agent_name": team_agent.as_ref().map(|team| team.agent_name.clone()),
            "agentName": team_agent.as_ref().map(|team| team.agent_name.clone()),
            "task_list_id": team_agent.as_ref().map(|team| team.task_list_id.clone()),
            "taskListId": team_agent.as_ref().map(|team| team.task_list_id.clone()),
            "team_file_path": team_agent.as_ref().map(|team| team.team_file_path.to_string_lossy().to_string()),
            "mailbox_path": team_agent.as_ref().map(|team| team.mailbox_path.to_string_lossy().to_string()),
            "plan_mode_required": team_agent.as_ref().map(|team| team.plan_mode_required),
            "planModeRequired": team_agent.as_ref().map(|team| team.plan_mode_required),
            "isolation": isolation.as_ref().map(|isolation| isolation.mode.clone()),
            "worktree_path": isolation.as_ref().map(|isolation| isolation.path.to_string_lossy().to_string()),
            "worktree_strategy": isolation.as_ref().map(|isolation| isolation.strategy.clone()),
            "outputFile": log_path.to_string_lossy(),
            "canReadOutputFile": true,
            "message": "Agent launched in the background."
        }),
        metadata: None,
    })
}

#[cfg(unix)]
fn background_agent_process(runner_path: &Path) -> std::process::Command {
    let mut process = std::process::Command::new("setsid");
    process.arg("bash").arg(runner_path);
    process
}

#[cfg(not(unix))]
fn background_agent_process(runner_path: &Path) -> std::process::Command {
    let mut process = std::process::Command::new(preferred_bash_program());
    process.arg(runner_path);
    process
}

fn agent_runner_script(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    prompt_path: &Path,
    log_path: &Path,
    exit_path: &Path,
    preloaded_skills: &[PreloadedSkill],
    agent_mcp_servers: Option<&Value>,
    isolation: Option<&AgentIsolation>,
    team_agent: Option<&TeamAgentRuntime>,
) -> ToolResult<String> {
    let runner = if let Ok(command) = std::env::var(AGENT_COMMAND_ENV) {
        format!("\"$BASH\" -lc {}", shell_quote(&command))
    } else {
        let exe = std::env::current_exe().map_err(ToolError::from)?;
        let mut command = shell_quote_path(&exe);
        for arg in agent_cli_args_before_prompt(input, agent, preloaded_skills, agent_mcp_servers) {
            command.push(' ');
            command.push_str(&shell_quote(&arg.to_string_lossy()));
        }
        let resident_flag = if team_agent.is_some() {
            " --resident-teammate"
        } else {
            ""
        };
        format!("{command} -p{resident_flag} \"$KIANA_AGENT_PROMPT\"")
    };

    Ok(format!(
        "#!/usr/bin/env bash\nset +e\nexport KIANA_AGENT_PROMPT=\"$(cat {prompt})\"\nexport KIANA_AGENT_DESCRIPTION={description}\nexport KIANA_AGENT_TYPE={agent_type}\nexport KIANA_AGENT_MODEL={model}\nexport KIANA_AGENT_SYSTEM_PROMPT={system_prompt}\nexport KIANA_AGENT_INITIAL_PROMPT={initial_prompt}\nexport KIANA_AGENT_TOOLS={tools}\nexport KIANA_AGENT_DISALLOWED_TOOLS={disallowed_tools}\nexport KIANA_AGENT_SKILLS={skills}\nexport KIANA_AGENT_PRELOADED_SKILLS={preloaded_skills}\nexport KIANA_AGENT_PERMISSION_MODE={permission_mode}\nexport KIANA_AGENT_MAX_TURNS={max_turns}\nexport KIANA_AGENT_MCP_SERVERS={mcp_servers}\nexport KIANA_AGENT_ISOLATION={isolation_mode}\nexport KIANA_AGENT_WORKTREE_PATH={worktree_path}\nexport KIANA_AGENT_WORKTREE_STRATEGY={worktree_strategy}\nexport KIANA_AGENT_ID={team_agent_id}\nexport KIANA_AGENT_NAME={team_agent_name}\nexport KIANA_AGENT_COLOR={team_agent_color}\nexport KIANA_TEAM_NAME={team_name}\nexport KIANA_TASK_LIST_ID={task_list_id}\nexport KIANA_TEAM_FILE={team_file}\nexport KIANA_TEAM_MAILBOX={team_mailbox}\nexport KIANA_TEAMS_ROOT={teams_root}\nexport KIANA_TASKS_ROOT={tasks_root}\nexport KIANA_PLAN_MODE_REQUIRED={plan_mode_required}\nexport CLAUDE_CODE_AGENT_ID={team_agent_id}\nexport CLAUDE_CODE_AGENT_NAME={team_agent_name}\nexport CLAUDE_CODE_TEAM_NAME={team_name}\nexport CLAUDE_CODE_TASK_LIST_ID={task_list_id}\nprintf '[%s] agent started\\n' \"$(date +%s)\" >> {log}\n{runner}>> {log} 2>&1\ncode=$?\nprintf '%s\\n' \"$code\" > {exit}\nif [ \"$code\" -eq 0 ]; then status=completed; else status=failed; fi\nprintf '[%s] agent %s exit_code=%s\\n' \"$(date +%s)\" \"$status\" \"$code\" >> {log}\n",
        prompt = shell_quote_path(prompt_path),
        description = shell_quote(&normalized_description(input, agent, "")),
        agent_type = shell_quote(
            &agent
                .map(|agent| agent.agent_type.clone())
                .or_else(|| normalized_optional(input.subagent_type.as_deref()))
                .unwrap_or_default()
        ),
        model = shell_quote(
            &normalized_optional(input.model.as_deref())
                .or_else(|| agent_model(agent))
                .unwrap_or_default()
        ),
        system_prompt = shell_quote(&agent.map(|agent| agent.system_prompt.clone()).unwrap_or_default()),
        initial_prompt = shell_quote(&agent.and_then(|agent| agent.initial_prompt.clone()).unwrap_or_default()),
        tools = shell_quote(&json_string(&agent.and_then(|agent| agent.tools.clone())).unwrap_or_else(|_| "null".to_string())),
        disallowed_tools = shell_quote(&json_string(&agent.map(|agent| agent.disallowed_tools.clone()).unwrap_or_default()).unwrap_or_else(|_| "[]".to_string())),
        skills = shell_quote(&json_string(&agent.map(|agent| agent.skills.clone()).unwrap_or_default()).unwrap_or_else(|_| "[]".to_string())),
        preloaded_skills = shell_quote(&json_string(preloaded_skills).unwrap_or_else(|_| "[]".to_string())),
        permission_mode = shell_quote(&agent.and_then(|agent| agent.permission_mode.clone()).unwrap_or_default()),
        max_turns = shell_quote(&agent.and_then(|agent| agent.max_turns).map(|turns| turns.to_string()).unwrap_or_default()),
        mcp_servers = shell_quote(&json_string(agent_mcp_servers.unwrap_or(&Value::Null)).unwrap_or_else(|_| "null".to_string())),
        isolation_mode = shell_quote(&isolation.map(|isolation| isolation.mode.clone()).unwrap_or_default()),
        worktree_path = shell_quote(&isolation.map(|isolation| isolation.path.to_string_lossy().to_string()).unwrap_or_default()),
        worktree_strategy = shell_quote(&isolation.map(|isolation| isolation.strategy.clone()).unwrap_or_default()),
        team_agent_id = shell_quote(&team_agent.map(|team| team.agent_id.clone()).unwrap_or_default()),
        team_agent_name = shell_quote(&team_agent.map(|team| team.agent_name.clone()).unwrap_or_default()),
        team_agent_color = shell_quote(&team_agent.map(|team| team.color.clone()).unwrap_or_default()),
        team_name = shell_quote(&team_agent.map(|team| team.team_name.clone()).unwrap_or_default()),
        task_list_id = shell_quote(&team_agent.map(|team| team.task_list_id.clone()).unwrap_or_default()),
        team_file = shell_quote(&team_agent.map(|team| team.team_file_path.to_string_lossy().to_string()).unwrap_or_default()),
        team_mailbox = shell_quote(&team_agent.map(|team| team.mailbox_path.to_string_lossy().to_string()).unwrap_or_default()),
        teams_root = shell_quote(&team_agent.map(|team| team.teams_root.to_string_lossy().to_string()).unwrap_or_default()),
        tasks_root = shell_quote(&team_agent.map(|team| team.tasks_root.to_string_lossy().to_string()).unwrap_or_default()),
        plan_mode_required = shell_quote(&team_agent.map(|team| if team.plan_mode_required { "true".to_string() } else { "false".to_string() }).unwrap_or_default()),
        log = shell_quote_path(log_path),
        runner = runner,
        exit = shell_quote_path(exit_path)
    ))
}

fn agent_cli_args_before_prompt(
    input: &AgentInput,
    agent: Option<&AgentDefinition>,
    preloaded_skills: &[PreloadedSkill],
    agent_mcp_servers: Option<&Value>,
) -> Vec<OsString> {
    let mut args = agent_cli_args(input, agent, "", preloaded_skills, agent_mcp_servers);
    args.truncate(args.len().saturating_sub(2));
    args
}

#[cfg(test)]
fn resolve_agent_definition(
    agent_type: &str,
    cwd: impl AsRef<Path>,
) -> Result<AgentDefinition, String> {
    resolve_agent_definition_with_trust(agent_type, cwd, ProjectTrust::Trusted)
}

fn resolve_agent_definition_with_trust(
    agent_type: &str,
    cwd: impl AsRef<Path>,
    project_trust: ProjectTrust,
) -> Result<AgentDefinition, String> {
    let agent_type = agent_type.trim();
    let agents = load_agent_definitions_with_trust(cwd.as_ref(), project_trust);
    if let Some(agent) = agents
        .into_iter()
        .find(|agent| agent.agent_type.eq_ignore_ascii_case(agent_type))
    {
        return Ok(agent);
    }
    built_in_agent_definition(agent_type).ok_or_else(|| format!("Unknown agent type: {agent_type}"))
}

fn load_agent_definitions_with_trust(
    cwd: &Path,
    project_trust: ProjectTrust,
) -> Vec<AgentDefinition> {
    let mut by_name = HashMap::new();
    for agent in load_plugin_agent_definitions(cwd) {
        by_name.insert(agent.agent_type.to_ascii_lowercase(), agent);
    }
    for dir in agent_dirs_with_trust(cwd, project_trust) {
        for path in read_markdown_files_sorted(&dir) {
            if let Some(agent) = load_agent_definition(&path, cwd) {
                by_name.insert(agent.agent_type.to_ascii_lowercase(), agent);
            }
        }
    }
    let mut agents = by_name.into_values().collect::<Vec<_>>();
    agents.sort_by(|a, b| a.agent_type.cmp(&b.agent_type));
    agents
}

fn load_agent_definition(path: &Path, cwd: &Path) -> Option<AgentDefinition> {
    let content = fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&content)?;
    let frontmatter: AgentFrontmatter = serde_yaml::from_str(frontmatter).ok()?;
    let agent_type = nonempty_string(frontmatter.name)?;
    let description = nonempty_string(frontmatter.description)?;
    let base_system_prompt = body.trim();
    if base_system_prompt.is_empty() {
        return None;
    }
    let memory = parse_agent_memory_scope(frontmatter.memory);
    let system_prompt =
        system_prompt_with_agent_memory(base_system_prompt, &agent_type, memory, cwd);
    let mut tools = parse_agent_tools(frontmatter.tools.as_ref());
    inject_agent_memory_tools(&mut tools, memory);
    Some(AgentDefinition {
        agent_type,
        description,
        system_prompt,
        tools,
        disallowed_tools: parse_tool_list(frontmatter.disallowed_tools.as_ref())
            .unwrap_or_default(),
        skills: parse_tool_list(frontmatter.skills.as_ref()).unwrap_or_default(),
        hooks: parse_agent_lifecycle_hooks(frontmatter.hooks.as_ref(), None),
        memory,
        permission_mode: parse_permission_mode(frontmatter.permission_mode),
        max_turns: parse_positive_u64(frontmatter.max_turns.as_ref()),
        mcp_servers: parse_mcp_servers(frontmatter.mcp_servers.as_ref()),
        isolation: parse_isolation(frontmatter.isolation.as_deref()),
        model: frontmatter
            .model
            .and_then(|value| normalized_optional(Some(&value))),
        initial_prompt: frontmatter
            .initial_prompt
            .and_then(|value| normalized_optional(Some(&value))),
        background: parse_bool_like(frontmatter.background.as_ref()).unwrap_or(false),
        path: Some(path.to_path_buf()),
    })
}

fn load_plugin_agent_definitions(cwd: &Path) -> Vec<AgentDefinition> {
    let mut agents = Vec::new();
    for plugin in installed_plugin_roots()
        .into_iter()
        .filter_map(read_plugin_agent_source)
    {
        let mut loaded_paths = HashSet::new();
        for source in plugin.agent_sources {
            if source.is_file() {
                if let Some(agent) = load_plugin_agent_file(
                    &plugin.name,
                    &plugin.root,
                    &source,
                    &[],
                    &mut loaded_paths,
                    cwd,
                ) {
                    agents.push(agent);
                }
            } else if source.is_dir() {
                agents.extend(load_plugin_agents_from_dir(
                    &plugin.name,
                    &plugin.root,
                    &source,
                    &mut loaded_paths,
                    cwd,
                ));
            }
        }
    }
    agents
}

#[derive(Debug)]
struct PluginAgentSource {
    name: String,
    root: PathBuf,
    agent_sources: Vec<PathBuf>,
}

fn read_plugin_agent_source(plugin_root: PathBuf) -> Option<PluginAgentSource> {
    let manifest_path = find_plugin_manifest_path(&plugin_root)?;
    let manifest = read_plugin_manifest(&manifest_path);
    let name = manifest
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .or_else(|| {
            plugin_root
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })?;

    let mut agent_sources = Vec::new();
    push_plugin_agent_source(&mut agent_sources, plugin_root.join("agents"));
    if let Some(manifest) = manifest.as_ref() {
        push_manifest_agent_sources(&mut agent_sources, &plugin_root, manifest);
    }
    let agent_sources = dedupe_paths(agent_sources);
    if agent_sources.is_empty() {
        return None;
    }
    Some(PluginAgentSource {
        name,
        root: plugin_root,
        agent_sources,
    })
}

fn load_plugin_agents_from_dir(
    plugin_name: &str,
    plugin_root: &Path,
    agents_dir: &Path,
    loaded_paths: &mut HashSet<String>,
    cwd: &Path,
) -> Vec<AgentDefinition> {
    let mut agents = Vec::new();
    for file in read_plugin_markdown_files_sorted(agents_dir) {
        let namespace = plugin_agent_namespace(&file, agents_dir);
        if let Some(agent) = load_plugin_agent_file(
            plugin_name,
            plugin_root,
            &file,
            &namespace,
            loaded_paths,
            cwd,
        ) {
            agents.push(agent);
        }
    }
    agents
}

fn load_plugin_agent_file(
    plugin_name: &str,
    plugin_root: &Path,
    path: &Path,
    namespace: &[String],
    loaded_paths: &mut HashSet<String>,
    cwd: &Path,
) -> Option<AgentDefinition> {
    if !loaded_paths.insert(path_key(path)) {
        return None;
    }
    if !path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        return None;
    }
    let content = fs::read_to_string(path).ok()?;
    let (frontmatter, body) = split_frontmatter(&content)?;
    let frontmatter: AgentFrontmatter = serde_yaml::from_str(frontmatter).ok()?;
    let base_agent_name = nonempty_string(frontmatter.name).or_else(|| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_string)
    })?;
    let agent_type = plugin_agent_type(plugin_name, namespace, &base_agent_name);
    let description = nonempty_string(frontmatter.description)
        .unwrap_or_else(|| format!("Agent from {plugin_name} plugin"));
    let base_system_prompt = substitute_plugin_variables(body.trim(), plugin_root);
    if base_system_prompt.trim().is_empty() {
        return None;
    }
    let memory = parse_agent_memory_scope(frontmatter.memory);
    let system_prompt =
        system_prompt_with_agent_memory(&base_system_prompt, &agent_type, memory, cwd);
    let mut tools = parse_agent_tools(frontmatter.tools.as_ref());
    inject_agent_memory_tools(&mut tools, memory);

    Some(AgentDefinition {
        agent_type,
        description,
        system_prompt,
        tools,
        disallowed_tools: parse_tool_list(frontmatter.disallowed_tools.as_ref())
            .unwrap_or_default(),
        skills: parse_tool_list(frontmatter.skills.as_ref()).unwrap_or_default(),
        hooks: parse_agent_lifecycle_hooks(frontmatter.hooks.as_ref(), Some(plugin_root)),
        memory,
        permission_mode: None,
        max_turns: parse_positive_u64(frontmatter.max_turns.as_ref()),
        mcp_servers: None,
        isolation: parse_isolation(frontmatter.isolation.as_deref()),
        model: frontmatter
            .model
            .and_then(|value| normalized_optional(Some(&value))),
        initial_prompt: frontmatter
            .initial_prompt
            .and_then(|value| normalized_optional(Some(&value))),
        background: parse_bool_like(frontmatter.background.as_ref()).unwrap_or(false),
        path: Some(path.to_path_buf()),
    })
}

fn plugin_agent_type(plugin_name: &str, namespace: &[String], base_agent_name: &str) -> String {
    let mut parts = Vec::with_capacity(namespace.len() + 2);
    parts.push(plugin_name.trim().to_string());
    parts.extend(namespace.iter().map(|part| part.trim().to_string()));
    parts.push(base_agent_name.trim().to_string());
    parts
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

fn plugin_agent_namespace(file: &Path, base_dir: &Path) -> Vec<String> {
    file.parent()
        .and_then(|parent| parent.strip_prefix(base_dir).ok())
        .map(|relative| {
            relative
                .components()
                .filter_map(|component| match component {
                    Component::Normal(value) => value.to_str().map(str::to_string),
                    _ => None,
                })
                .filter(|part| !part.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn read_plugin_markdown_files_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_plugin_markdown_files(dir, &mut files);
    files.sort();
    files
}

fn visit_plugin_markdown_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.join("SKILL.md").is_file() {
                continue;
            }
            visit_plugin_markdown_files(&path, files);
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            files.push(path);
        }
    }
}

fn push_manifest_agent_sources(sources: &mut Vec<PathBuf>, plugin_root: &Path, manifest: &Value) {
    let Some(agents) = manifest.get("agents") else {
        return;
    };
    match agents {
        Value::String(path) => push_safe_plugin_agent_source(sources, plugin_root, path),
        Value::Array(paths) => {
            for path in paths {
                if let Some(path) = path.as_str() {
                    push_safe_plugin_agent_source(sources, plugin_root, path);
                }
            }
        }
        _ => {}
    }
}

fn push_safe_plugin_agent_source(sources: &mut Vec<PathBuf>, plugin_root: &Path, path: &str) {
    let Some(path) = safe_relative_plugin_path(path) else {
        return;
    };
    push_plugin_agent_source(sources, plugin_root.join(path));
}

fn push_plugin_agent_source(sources: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir()
        || path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
            && path.is_file()
    {
        sources.push(path);
    }
}

fn safe_relative_plugin_path(path: &str) -> Option<PathBuf> {
    let path = PathBuf::from(path);
    if path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return None;
    }
    Some(path)
}

fn substitute_plugin_variables(content: &str, plugin_root: &Path) -> String {
    let plugin_root = plugin_root.to_string_lossy();
    content
        .replace("${CLAUDE_PLUGIN_ROOT}", &plugin_root)
        .replace("${KIANA_PLUGIN_ROOT}", &plugin_root)
}

fn installed_plugin_roots() -> Vec<PathBuf> {
    kiana_types::plugin::installed_plugin_roots()
}

fn read_plugin_manifest(path: &Path) -> Option<Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
}

fn find_plugin_manifest_path(plugin_root: &Path) -> Option<PathBuf> {
    [
        plugin_root.join(".codex-plugin").join("plugin.json"),
        plugin_root.join(".claude-plugin").join("plugin.json"),
        plugin_root.join("plugin.json"),
    ]
    .into_iter()
    .find(|path| path.is_file())
}

fn path_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}

fn general_purpose_agent_definition() -> AgentDefinition {
    AgentDefinition {
        agent_type: "general-purpose".to_string(),
        description: "General-purpose agent for delegated coding tasks".to_string(),
        system_prompt: "You are a general-purpose coding subagent. Complete the delegated task in the given repository context and report concise, verifiable results.".to_string(),
        tools: None,
        disallowed_tools: Vec::new(),
        skills: Vec::new(),
        hooks: Vec::new(),
        memory: None,
        permission_mode: None,
        max_turns: None,
        mcp_servers: None,
        isolation: None,
        model: None,
        initial_prompt: None,
        background: false,
        path: None,
    }
}

const STATUSLINE_SETUP_SYSTEM_PROMPT: &str = r#"You are a status line setup agent for Claude Code. Your job is to create or update the statusLine command in the user's Claude Code settings.

When asked to convert a shell prompt, inspect common shell configuration files, extract PS1 when present, translate common PS1 escapes into shell commands, preserve colors with printf, and remove trailing prompt markers such as "$" or ">" from the status line output.

The statusLine command receives JSON on stdin with session, transcript, cwd, model, workspace, version, output_style, context_window, rate_limits, vim, agent, and worktree fields. Build commands that read this JSON with tools such as jq, or write a helper script under the user's Claude configuration directory and point settings at that script.

Preserve existing settings, follow symlinks to update the target settings file, and return a concise summary of what was configured. At the end, remind the parent agent that the statusline-setup agent should be used for future status line changes."#;

const CLAUDE_CODE_GUIDE_SYSTEM_PROMPT: &str = r#"You are the Claude guide agent. Help users understand and use Claude Code, the Claude Agent SDK, and the Claude API effectively.

Use official documentation first:
- Claude Code docs map: https://code.claude.com/docs/en/claude_code_docs_map.md
- Claude Agent SDK and Claude API docs map: https://platform.claude.com/llms.txt

Determine whether the question is about Claude Code, Agent SDK, or API usage. Fetch the relevant docs map, inspect the most relevant pages, then answer with concise, actionable guidance and examples when useful. Use local project files such as CLAUDE.md and .claude/ configuration when relevant. If official docs do not answer the question, use web search and clearly distinguish unsupported or uncertain guidance."#;

async fn enrich_claude_code_guide_agent(
    agent: Option<&mut AgentDefinition>,
    cwd: &Path,
    context: &ToolContext,
) {
    let Some(agent) = agent else {
        return;
    };
    if agent.path.is_some() || !agent.agent_type.eq_ignore_ascii_case("claude-code-guide") {
        return;
    }
    let context_prompt = claude_code_guide_context_prompt(cwd, context).await;
    if context_prompt.trim().is_empty() {
        return;
    }
    agent.system_prompt = format!(
        "{base}\n\n---\n\n# User's Current Configuration\n\nThe user has the following custom setup in their environment:\n\n{context_prompt}\n\nWhen answering questions, consider these configured features and proactively suggest them when relevant.",
        base = agent.system_prompt.trim(),
    );
}

async fn claude_code_guide_context_prompt(cwd: &Path, context: &ToolContext) -> String {
    let mut sections = Vec::new();

    let skills = kiana_skills::load_all_skills_with_trust(
        cwd,
        project_trust_from_app_state(&context.app_state),
    )
    .await;
    let custom_skills = skills
        .iter()
        .filter(|skill| skill.loaded_from != kiana_skills::LoadedFrom::Plugin)
        .map(format_skill_context_line)
        .collect::<Vec<_>>();
    if !custom_skills.is_empty() {
        sections.push(format!(
            "**Available custom skills in this project:**\n{}",
            custom_skills.join("\n")
        ));
    }

    let custom_agents =
        load_agent_definitions_with_trust(cwd, project_trust_from_app_state(&context.app_state))
            .into_iter()
            .map(|agent| format!("- {}: {}", agent.agent_type, agent.description))
            .collect::<Vec<_>>();
    if !custom_agents.is_empty() {
        sections.push(format!(
            "**Available custom agents configured:**\n{}",
            custom_agents.join("\n")
        ));
    }

    if let Some(mcp_servers) = parent_mcp_servers(context) {
        let mcp_names = mcp_server_names(&mcp_servers);
        if !mcp_names.is_empty() {
            sections.push(format!(
                "**Configured MCP servers:**\n{}",
                mcp_names
                    .into_iter()
                    .map(|name| format!("- {name}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }

    let plugin_skills = skills
        .iter()
        .filter(|skill| skill.loaded_from == kiana_skills::LoadedFrom::Plugin)
        .map(format_skill_context_line)
        .collect::<Vec<_>>();
    if !plugin_skills.is_empty() {
        sections.push(format!(
            "**Available plugin skills:**\n{}",
            plugin_skills.join("\n")
        ));
    }

    if let Some(settings) = current_settings_json(cwd) {
        sections.push(format!(
            "**User's settings.json:**\n```json\n{}\n```",
            serde_json::to_string_pretty(&settings).unwrap_or_else(|_| settings.to_string())
        ));
    }

    sections.join("\n\n")
}

fn format_skill_context_line(skill: &kiana_skills::Command) -> String {
    let description = if !skill.description.trim().is_empty() {
        skill.description.trim()
    } else {
        skill
            .when_to_use
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("No description")
    };
    format!("- /{}: {}", skill.name, description)
}

fn mcp_server_names(mcp_servers: &Value) -> Vec<String> {
    let mut names = mcp_servers
        .as_object()
        .map(|servers| servers.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    names.sort();
    names
}

fn current_settings_json(cwd: &Path) -> Option<Value> {
    if let Ok(raw) = env::var("KIANA_SETTINGS_JSON") {
        if let Some(settings) = parse_settings_json(&raw) {
            return Some(settings);
        }
    }

    for key in [
        "KIANA_SETTINGS_FILE",
        "KIANA_REMOTE_SETTINGS_FILE",
        "KIANA_CONFIG_FILE",
    ] {
        if let Ok(path) = env::var(key) {
            if let Some(settings) = read_settings_json(Path::new(&path)) {
                return Some(settings);
            }
        }
    }

    for path in settings_json_candidates(cwd) {
        if let Some(settings) = read_settings_json(&path) {
            return Some(settings);
        }
    }
    None
}

fn settings_json_candidates(cwd: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(cwd.join(".claude").join("settings.json"));
    candidates.push(cwd.join(".claude").join("settings.local.json"));
    if let Some(kiana_home) = env::var_os("KIANA_HOME") {
        candidates.push(PathBuf::from(kiana_home).join("settings.json"));
    }
    if let Some(home) = home_dir() {
        candidates.push(home.join(".claude").join("settings.json"));
        candidates.push(home.join(".claude").join("settings.local.json"));
    }
    dedupe_paths(candidates)
}

fn read_settings_json(path: &Path) -> Option<Value> {
    let contents = fs::read_to_string(path).ok()?;
    if path.extension().and_then(|extension| extension.to_str()) != Some("json")
        && !contents.trim_start().starts_with('{')
    {
        return None;
    }
    parse_settings_json(&contents)
}

fn parse_settings_json(raw: &str) -> Option<Value> {
    let value: Value = serde_json::from_str(raw).ok()?;
    value.as_object()?;
    Some(value)
}

fn built_in_agent_definition(agent_type: &str) -> Option<AgentDefinition> {
    match agent_type.to_ascii_lowercase().as_str() {
        "general-purpose" => Some(general_purpose_agent_definition()),
        "statusline-setup" => Some(AgentDefinition {
            agent_type: "statusline-setup".to_string(),
            description: "Use this agent to configure the user's Claude Code status line setting."
                .to_string(),
            system_prompt: STATUSLINE_SETUP_SYSTEM_PROMPT.to_string(),
            tools: Some(vec!["Read".to_string(), "Edit".to_string()]),
            disallowed_tools: Vec::new(),
            skills: Vec::new(),
            hooks: Vec::new(),
            memory: None,
            permission_mode: None,
            max_turns: None,
            mcp_servers: None,
            isolation: None,
            model: Some("sonnet".to_string()),
            initial_prompt: None,
            background: false,
            path: None,
        }),
        "claude-code-guide" => Some(AgentDefinition {
            agent_type: "claude-code-guide".to_string(),
            description: "Use this agent for questions about Claude Code, the Claude Agent SDK, and the Claude API.".to_string(),
            system_prompt: CLAUDE_CODE_GUIDE_SYSTEM_PROMPT.to_string(),
            tools: Some(vec![
                "Glob".to_string(),
                "Grep".to_string(),
                "Read".to_string(),
                "WebFetch".to_string(),
                "WebSearch".to_string(),
            ]),
            disallowed_tools: Vec::new(),
            skills: Vec::new(),
            hooks: Vec::new(),
            memory: None,
            permission_mode: Some("dontAsk".to_string()),
            max_turns: None,
            mcp_servers: None,
            isolation: None,
            model: Some("haiku".to_string()),
            initial_prompt: None,
            background: false,
            path: None,
        }),
        _ => None,
    }
}

fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];
    let body = rest[end + 4..].trim_start_matches(['\r', '\n']);
    Some((frontmatter, body))
}

fn nonempty_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_bool_like(value: Option<&serde_yaml::Value>) -> Option<bool> {
    match value? {
        serde_yaml::Value::Bool(value) => Some(*value),
        serde_yaml::Value::String(value) => match value.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn agent_dirs_with_trust(cwd: &Path, project_trust: ProjectTrust) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    push_existing_agent_dir(
        &mut dirs,
        home_dir().map(|home| home.join(".kiana").join("agents")),
    );
    push_existing_agent_dir(
        &mut dirs,
        home_dir().map(|home| home.join(".claude").join("agents")),
    );
    push_existing_agent_dir(
        &mut dirs,
        env::var_os("KIANA_HOME").map(|home| PathBuf::from(home).join("agents")),
    );

    if project_trust.allows_project_resources() {
        let mut ancestors = Vec::new();
        let mut current = Some(cwd);
        while let Some(dir) = current {
            ancestors.push(dir.to_path_buf());
            current = dir.parent();
        }
        ancestors.reverse();
        for dir in &ancestors {
            push_existing_agent_dir(&mut dirs, Some(dir.join(".kiana").join("agents")));
            push_existing_agent_dir(&mut dirs, Some(dir.join(".claude").join("agents")));
        }
        for dir in &ancestors {
            push_existing_agent_dir(&mut dirs, Some(dir.join(".kiana").join("agents-local")));
            push_existing_agent_dir(&mut dirs, Some(dir.join(".claude").join("agents-local")));
        }
    }
    dedupe_paths(dirs)
}

fn push_existing_agent_dir(dirs: &mut Vec<PathBuf>, dir: Option<PathBuf>) {
    if let Some(dir) = dir {
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
}

fn read_markdown_files_sorted(dir: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(dir)
        .ok()
        .into_iter()
        .flat_map(|entries| entries.filter_map(Result::ok))
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("md")
        })
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    paths
        .into_iter()
        .filter(|path| {
            seen.insert(
                path.canonicalize()
                    .unwrap_or_else(|_| path.clone())
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect()
}

fn home_dir() -> Option<PathBuf> {
    if let Some(home) = env::var_os("HOME").filter(|home| !home.is_empty()) {
        return Some(PathBuf::from(home));
    }
    #[cfg(windows)]
    {
        env::var_os("USERPROFILE")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

fn background_root(context: &ToolContext) -> PathBuf {
    if let Some(path) = context
        .app_state
        .get(BACKGROUND_ROOT_KEY)
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_BG_DIR") {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("KIANA_HOME") {
        return PathBuf::from(path).join("bg-tasks");
    }
    if let Some(home) = dirs::home_dir() {
        return home.join(".kiana").join("bg-tasks");
    }
    PathBuf::from(".kiana").join("bg-tasks")
}

fn ensure_background_dirs(root: &Path) -> ToolResult<()> {
    for dir in ["tasks", "logs", "scripts", "exits", "prompts"] {
        fs::create_dir_all(root.join(dir))?;
    }
    Ok(())
}

fn append_log(path: &Path, line: &str) -> ToolResult<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(format!("[{}] {}", now_unix_seconds(), line).as_bytes())?;
    Ok(())
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        agent_cli_args, agent_runner_script, claude_code_guide_context_prompt,
        resolve_agent_definition, AgentDefinition, AgentInput, AgentTool, TeamAgentRuntime,
    };
    use crate::task_output::TaskOutputTool;
    use crate::team_create::TeamCreateTool;
    use crate::{Tool, ToolContext};
    use serde_json::{json, Value};
    use std::collections::HashMap;
    use std::fs;
    use uuid::Uuid;

    struct EnvSnapshot {
        values: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl EnvSnapshot {
        fn take(keys: &[&'static str]) -> Self {
            Self {
                values: keys
                    .iter()
                    .map(|key| (*key, std::env::var_os(key)))
                    .collect(),
            }
        }
    }

    impl Drop for EnvSnapshot {
        fn drop(&mut self) {
            for (key, value) in self.values.iter().rev() {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn test_context(root: &std::path::Path) -> ToolContext {
        test_context_with_cwd(root, &std::env::current_dir().unwrap())
    }

    fn test_context_with_cwd(root: &std::path::Path, cwd: &std::path::Path) -> ToolContext {
        let (_abort_tx, abort_rx) = tokio::sync::watch::channel(false);
        ToolContext {
            cwd: cwd.to_string_lossy().to_string(),
            read_file_state: HashMap::new(),
            app_state: HashMap::from([
                (
                    "background_root".to_string(),
                    json!(root.to_string_lossy().to_string()),
                ),
                (
                    "teams_root".to_string(),
                    json!(root.join("teams").to_string_lossy().to_string()),
                ),
                (
                    "tasks_root".to_string(),
                    json!(root.join("tasks").to_string_lossy().to_string()),
                ),
            ]),
            abort_signal: abort_rx,
        }
    }

    fn read_json(path: impl AsRef<std::path::Path>) -> Value {
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap()
    }

    fn write_plugin_manifest(plugin_root: &std::path::Path, name: &str, extra: Value) {
        fs::create_dir_all(plugin_root.join(".codex-plugin")).unwrap();
        let mut manifest = serde_json::Map::new();
        manifest.insert("name".to_string(), json!(name));
        if let Some(extra) = extra.as_object() {
            for (key, value) in extra {
                manifest.insert(key.clone(), value.clone());
            }
        }
        fs::write(
            plugin_root.join(".codex-plugin").join("plugin.json"),
            Value::Object(manifest).to_string(),
        )
        .unwrap();
    }

    #[tokio::test]
    async fn agent_runs_foreground_runner_command() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'agent saw: %s\\n' \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let mut context = test_context(&root);
        let tool = AgentTool::new();
        let input = json!({
            "description": "Inspect MCP",
            "prompt": "Inspect MCP startup"
        });

        let validation = tool.validate_input(&input, &context).await;
        assert!(validation.result, "{:?}", validation.message);

        let output = tool.call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["status"], "completed");
        assert!(output.data["stdout"]
            .as_str()
            .unwrap()
            .contains("agent saw: Inspect MCP startup"));

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_runs_listed_built_in_agent_definitions() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_PLUGINS_DIR",
            "KIANA_AGENT_COMMAND",
        ]);
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'type=%s model=%s mode=%s tools=%s system=%s prompt=%s\\n' \"$KIANA_AGENT_TYPE\" \"$KIANA_AGENT_MODEL\" \"$KIANA_AGENT_PERMISSION_MODE\" \"$KIANA_AGENT_TOOLS\" \"$KIANA_AGENT_SYSTEM_PROMPT\" \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-builtins-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&repo).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        let mut context = test_context_with_cwd(&root.join("bg"), &repo);

        let statusline_input = json!({
            "subagent_type": "statusline-setup",
            "prompt": "Configure my prompt"
        });
        let validation = AgentTool::new()
            .validate_input(&statusline_input, &context)
            .await;
        assert!(validation.result, "{validation:?}");
        let statusline = AgentTool::new()
            .call(&statusline_input, &mut context)
            .await
            .unwrap();
        let stdout = statusline.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("type=statusline-setup"));
        assert!(stdout.contains("model=sonnet"));
        assert!(stdout.contains("tools=[\"Read\",\"Edit\"]"));
        assert!(stdout.contains("statusLine command"));

        let guide_input = json!({
            "subagent_type": "claude-code-guide",
            "prompt": "How do hooks work?"
        });
        let validation = AgentTool::new()
            .validate_input(&guide_input, &context)
            .await;
        assert!(validation.result, "{validation:?}");
        let guide = AgentTool::new()
            .call(&guide_input, &mut context)
            .await
            .unwrap();
        let stdout = guide.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("type=claude-code-guide"));
        assert!(stdout.contains("model=haiku"));
        assert!(stdout.contains("mode=dontAsk"));
        assert!(stdout.contains("WebFetch"));
        assert!(stdout.contains("Claude Code docs map"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn claude_code_guide_includes_dynamic_project_context() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        kiana_skills::clear_caches();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_PLUGINS_DIR",
            "KIANA_AGENT_COMMAND",
            "KIANA_SETTINGS_JSON",
        ]);
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'system=%s\\n' \"$KIANA_AGENT_SYSTEM_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-guide-context-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        let bg = root.join("bg");
        let skill_dir = repo.join(".claude").join("skills").join("repo-audit");
        let agent_dir = repo.join(".claude").join("agents");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("ops");
        let plugin_skill_dir = plugin_root.join("skills").join("deploy-check");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&skill_dir).unwrap();
        fs::create_dir_all(&agent_dir).unwrap();
        fs::create_dir_all(&plugin_skill_dir).unwrap();
        write_plugin_manifest(&plugin_root, "ops", json!({}));
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Repo Audit\ndescription: Audit this repo\n---\nAudit body.\n",
        )
        .unwrap();
        fs::write(
            agent_dir.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews local code\n---\nReview carefully.\n",
        )
        .unwrap();
        fs::write(
            plugin_skill_dir.join("SKILL.md"),
            "---\nname: Deploy Check\ndescription: Check deployments from plugin\n---\nDeploy body.\n",
        )
        .unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);
        std::env::set_var(
            "KIANA_SETTINGS_JSON",
            r#"{"settings":{"theme":"dark"},"agents":{"from-settings":{"description":"visible settings agent"}}}"#,
        );

        let mut context = test_context_with_cwd(&bg, &repo);
        context.app_state.insert(
            "mcp_servers".to_string(),
            json!({
                "docs": {
                    "command": "docs-mcp"
                }
            }),
        );
        let input = json!({
            "subagent_type": "claude-code-guide",
            "prompt": "What is available here?"
        });

        let validation = AgentTool::new().validate_input(&input, &context).await;
        assert!(validation.result, "{validation:?}");
        let output = AgentTool::new().call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["status"], "completed");
        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("# User's Current Configuration"));
        assert!(stdout.contains("**Available custom skills in this project:**"));
        assert!(stdout.contains("- /repo-audit: Audit this repo"));
        assert!(stdout.contains("**Available custom agents configured:**"));
        assert!(stdout.contains("- reviewer: Reviews local code"));
        assert!(stdout.contains("**Configured MCP servers:**"));
        assert!(stdout.contains("- docs"));
        assert!(stdout.contains("**Available plugin skills:**"));
        assert!(stdout.contains("- /ops:deploy-check: Check deployments from plugin"));
        assert!(stdout.contains("\"theme\": \"dark\""));

        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn claude_code_guide_hides_project_agents_when_project_is_untrusted() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        kiana_skills::clear_caches();
        let _env = EnvSnapshot::take(&["HOME", "KIANA_HOME", "KIANA_PLUGINS_DIR"]);
        let root = std::env::temp_dir().join(format!("kiana-guide-trust-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        let agent_dir = repo.join(".claude").join("agents");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&agent_dir).unwrap();
        fs::write(
            agent_dir.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews local code\n---\nReview carefully.\n",
        )
        .unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));

        let mut context = test_context_with_cwd(&root.join("bg"), &repo);
        context
            .app_state
            .insert("project_trusted".to_string(), json!(false));

        let prompt = claude_code_guide_context_prompt(&repo, &context).await;

        assert!(!prompt.contains("**Available custom agents configured:**"));
        assert!(!prompt.contains("- reviewer: Reviews local code"));

        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_resolution_uses_kiana_dirs_and_local_precedence() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["HOME", "KIANA_HOME", "KIANA_PLUGINS_DIR"]);
        let root = std::env::temp_dir().join(format!("kiana-agent-dirs-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        let project_agents = repo.join(".kiana").join("agents");
        let local_agents = repo.join(".claude").join("agents-local");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_agents).unwrap();
        fs::create_dir_all(&local_agents).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        fs::write(
            project_agents.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Project reviewer\n---\nProject prompt.\n",
        )
        .unwrap();
        fs::write(
            local_agents.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Local reviewer\n---\nLocal prompt.\n",
        )
        .unwrap();
        fs::write(
            project_agents.join("statusline-setup.md"),
            "---\nname: statusline-setup\ndescription: Project override\nmodel: custom-model\n---\nCustom statusline prompt.\n",
        )
        .unwrap();

        let reviewer = resolve_agent_definition("reviewer", &repo).unwrap();
        assert_eq!(reviewer.description, "Local reviewer");
        assert_eq!(reviewer.system_prompt, "Local prompt.");

        let statusline = resolve_agent_definition("statusline-setup", &repo).unwrap();
        assert_eq!(statusline.description, "Project override");
        assert_eq!(statusline.system_prompt, "Custom statusline prompt.");
        assert_eq!(statusline.model.as_deref(), Some("custom-model"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn untrusted_project_does_not_resolve_project_agent_definition() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&["HOME", "KIANA_HOME", "KIANA_PLUGINS_DIR"]);
        let root = std::env::temp_dir().join(format!("kiana-agent-trust-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        let project_agents = repo.join(".kiana").join("agents");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&project_agents).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        std::env::set_var("KIANA_PLUGINS_DIR", root.join("missing-plugins"));
        fs::write(
            project_agents.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Project reviewer\n---\nProject prompt.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(&root.join("bg"), &repo);
        context
            .app_state
            .insert("project_trusted".to_string(), json!(false));

        let project_validation = AgentTool::new()
            .validate_input(
                &json!({
                    "subagent_type": "reviewer",
                    "prompt": "Review this project"
                }),
                &context,
            )
            .await;
        assert!(!project_validation.result, "{project_validation:?}");
        assert!(project_validation
            .message
            .as_deref()
            .unwrap()
            .contains("Unknown agent type: reviewer"));

        let builtin_validation = AgentTool::new()
            .validate_input(
                &json!({
                    "subagent_type": "claude-code-guide",
                    "prompt": "What can I use?"
                }),
                &context,
            )
            .await;
        assert!(builtin_validation.result, "{builtin_validation:?}");

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_registers_foreground_teammate_with_team_context_and_env() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'team=%s name=%s id=%s tasklist=%s plan=%s prompt=%s\\n' \"$KIANA_TEAM_NAME\" \"$KIANA_AGENT_NAME\" \"$KIANA_AGENT_ID\" \"$KIANA_TASK_LIST_ID\" \"$KIANA_PLAN_MODE_REQUIRED\" \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-team-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let mut context = test_context_with_cwd(&root.join("bg"), &repo);
        TeamCreateTool::new()
            .call(
                &json!({ "team_name": "review", "members": ["alice"] }),
                &mut context,
            )
            .await
            .unwrap();

        let output = AgentTool::new()
            .call(
                &json!({
                    "name": "alice",
                    "prompt": "Inspect project wiring",
                    "planModeRequired": true
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["status"], "completed");
        assert_eq!(output.data["team_name"], "review");
        assert_eq!(output.data["agent_name"], "alice");
        assert_eq!(output.data["teammate_id"], "alice@review");
        assert_eq!(output.data["task_list_id"], "review");
        assert_eq!(output.data["planModeRequired"], true);
        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("team=review"));
        assert!(stdout.contains("name=alice"));
        assert!(stdout.contains("id=alice@review"));
        assert!(stdout.contains("tasklist=review"));
        assert!(stdout.contains("plan=true"));
        assert!(stdout.contains("prompt=Inspect project wiring"));

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let team_file = read_json(std::path::Path::new(teams_root).join("review/config.json"));
        let alice = team_file["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["name"] == "alice")
            .unwrap();
        assert_eq!(alice["agentId"], "alice@review");
        assert_eq!(alice["backendType"], "foreground-agent");
        assert_eq!(alice["isActive"], true);
        assert_eq!(alice["planModeRequired"], true);
        assert_eq!(
            context.app_state["team_context"]["teammates"]["alice@review"]["name"],
            "alice"
        );

        let inbox = read_json(std::path::Path::new(teams_root).join("review/inboxes/alice.json"));
        assert_eq!(inbox[0]["from"], "team-lead");
        assert_eq!(inbox[0]["text"], "Inspect project wiring");
        assert_eq!(inbox[0]["summary"], "initial_prompt");

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_loads_project_agent_definition_for_subagent_type() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        kiana_skills::clear_caches();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'type=%s model=%s mode=%s turns=%s desc=%s initial=%s system=%s tools=%s disallowed=%s skills=%s mcp=%s preloaded=%s prompt=%s\\n' \"$KIANA_AGENT_TYPE\" \"$KIANA_AGENT_MODEL\" \"$KIANA_AGENT_PERMISSION_MODE\" \"$KIANA_AGENT_MAX_TURNS\" \"$KIANA_AGENT_DESCRIPTION\" \"$KIANA_AGENT_INITIAL_PROMPT\" \"$KIANA_AGENT_SYSTEM_PROMPT\" \"$KIANA_AGENT_TOOLS\" \"$KIANA_AGENT_DISALLOWED_TOOLS\" \"$KIANA_AGENT_SKILLS\" \"$KIANA_AGENT_MCP_SERVERS\" \"$KIANA_AGENT_PRELOADED_SKILLS\" \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let bg = root.join("bg");
        let agents_dir = repo.join(".claude").join("agents");
        let skill_dir = repo.join(".claude").join("skills").join("refactor");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            agents_dir.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews code changes\nmodel: agent-model\npermissionMode: ask\nmaxTurns: \"4\"\ninitialPrompt: First inspect the diff.\ntools:\n  - Bash(git:*)\n  - Read\ndisallowedTools:\n  - Write\nskills:\n  - refactor\nmcpServers:\n  - parent\n  - docs:\n      command: docs-mcp\n      args:\n        - --stdio\n---\nYou are a strict code reviewer.\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Refactor Helper\ndescription: Improve refactors\n---\nUse repo refactor flow.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(&bg, &repo);
        context.app_state.insert(
            "mcp_servers".to_string(),
            json!({
                "parent": {
                    "command": "parent-mcp"
                }
            }),
        );
        let input = json!({
            "subagent_type": "reviewer",
            "prompt": "Inspect changes"
        });
        let validation = AgentTool::new().validate_input(&input, &context).await;
        assert!(validation.result, "{validation:?}");

        let output = AgentTool::new().call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["status"], "completed");
        assert_eq!(output.data["prompt"], "Inspect changes");
        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("type=reviewer"));
        assert!(stdout.contains("model=agent-model"));
        assert!(stdout.contains("mode=ask"));
        assert!(stdout.contains("turns=4"));
        assert!(stdout.contains("desc=Reviews code changes"));
        assert!(stdout.contains("initial=First inspect the diff."));
        assert!(stdout.contains("system=You are a strict code reviewer."));
        assert!(stdout.contains("tools=[\"Bash(git:*)\",\"Read\"]"));
        assert!(stdout.contains("disallowed=[\"Write\"]"));
        assert!(stdout.contains("skills=[\"refactor\"]"));
        assert!(stdout.contains("\"parent\":{\"command\":\"parent-mcp\"}"));
        assert!(stdout.contains("\"docs\":{\"args\":[\"--stdio\"],\"command\":\"docs-mcp\"}"));
        assert!(stdout.contains("\"name\":\"refactor\""));
        assert!(stdout.contains("Use repo refactor flow."));
        assert!(stdout.contains("### Skill: refactor"));
        assert!(stdout.contains("Inspect changes"));

        std::env::remove_var("KIANA_AGENT_COMMAND");
        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_memory_frontmatter_appends_prompt_and_injects_tools() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_AGENT_COMMAND",
            "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
            "CLAUDE_CODE_SIMPLE",
            "KIANA_CODE_SIMPLE",
        ]);
        std::env::remove_var("CLAUDE_CODE_DISABLE_AUTO_MEMORY");
        std::env::remove_var("CLAUDE_CODE_SIMPLE");
        std::env::remove_var("KIANA_CODE_SIMPLE");
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'scope=%s tools=%s system=%s\\n' \"$KIANA_AGENT_MEMORY_SCOPE\" \"$KIANA_AGENT_TOOLS\" \"$KIANA_AGENT_SYSTEM_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-memory-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        let agents_dir = repo.join(".claude").join("agents");
        let memory_dir = repo.join(".claude").join("agent-memory").join("reviewer");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&agents_dir).unwrap();
        fs::create_dir_all(&memory_dir).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        fs::write(
            memory_dir.join("MEMORY.md"),
            "- [Review policy](review_policy.md) - Always inspect tests first.\n",
        )
        .unwrap();
        fs::write(
            agents_dir.join("reviewer.md"),
            "---\nname: reviewer\ndescription: Reviews with memory\nmemory: project\ntools:\n  - Bash(git:*)\n---\nReview with remembered project policy.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(&root.join("bg"), &repo);
        let input = json!({
            "subagent_type": "reviewer",
            "prompt": "Review changes"
        });

        let validation = AgentTool::new().validate_input(&input, &context).await;
        assert!(validation.result, "{validation:?}");
        let output = AgentTool::new().call(&input, &mut context).await.unwrap();

        assert_eq!(output.data["status"], "completed");
        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("scope=project"));
        assert!(stdout.contains("tools=[\"Bash(git:*)\",\"Write\",\"Edit\",\"Read\"]"));
        assert!(stdout.contains("Review with remembered project policy."));
        assert!(stdout.contains("# Persistent Agent Memory"));
        assert!(
            stdout.contains(".claude/agent-memory/reviewer")
                || stdout.contains(r".claude\agent-memory\reviewer")
        );
        assert!(stdout.contains("Always inspect tests first."));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_frontmatter_lifecycle_hooks_add_context_and_run_subagent_stop() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_AGENT_COMMAND",
            "KIANA_AGENT_HOOKS",
            "KIANA_SUBAGENT_START_HOOKS",
            "KIANA_SUBAGENT_STOP_HOOKS",
            "KIANA_HOOKS_FILE",
        ]);
        std::env::remove_var("KIANA_AGENT_HOOKS");
        std::env::remove_var("KIANA_SUBAGENT_START_HOOKS");
        std::env::remove_var("KIANA_SUBAGENT_STOP_HOOKS");
        std::env::remove_var("KIANA_HOOKS_FILE");
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'prompt=%s\\n' \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-hooks-{}", Uuid::new_v4()));
        let home = root.join("home");
        let repo = root.join("repo");
        let agents_dir = repo.join(".claude").join("agents");
        let start_input = root.join("start-input.json");
        let stop_input = root.join("stop-input.json");
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(&agents_dir).unwrap();
        std::env::set_var("HOME", &home);
        std::env::set_var("KIANA_HOME", home.join(".kiana"));
        fs::write(
            agents_dir.join("reviewer.md"),
            format!(
                "---\nname: reviewer\ndescription: Reviews with lifecycle hooks\nhooks:\n  SubagentStart:\n    - matcher: reviewer\n      hooks:\n        - type: command\n          command: |\n            input=$(cat)\n            printf '%s' \"$input\" > {}\n            printf '%s' '{{\"hookSpecificOutput\":{{\"hookEventName\":\"SubagentStart\",\"additionalContext\":\"Use hook-provided context.\"}}}}'\n  Stop:\n    - matcher: reviewer\n      hooks:\n        - type: command\n          command: |\n            cat > {}\n            printf '%s' '{{}}'\n---\nReview with lifecycle hooks.\n",
                super::shell_quote_path(&start_input),
                super::shell_quote_path(&stop_input)
            ),
        )
        .unwrap();

        let mut context = test_context_with_cwd(&root.join("bg"), &repo);
        let input = json!({
            "subagent_type": "reviewer",
            "prompt": "Inspect hooked runtime"
        });

        let validation = AgentTool::new().validate_input(&input, &context).await;
        assert!(validation.result, "{validation:?}");
        let output = AgentTool::new().call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["status"], "completed");
        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("# Hook Additional Context"));
        assert!(stdout.contains("Use hook-provided context."));
        assert!(stdout.contains("Inspect hooked runtime"));
        let hook_results = output.data["hook_results"].as_array().unwrap();
        assert_eq!(hook_results.len(), 2);

        let start_json = read_json(&start_input);
        assert_eq!(start_json["hook_event_name"], "SubagentStart");
        assert_eq!(start_json["agent_type"], "reviewer");
        assert_eq!(start_json["prompt"], "Inspect hooked runtime");

        let stop_json = read_json(&stop_input);
        assert_eq!(stop_json["hook_event_name"], "SubagentStop");
        assert_eq!(stop_json["agent_type"], "reviewer");
        assert_eq!(stop_json["status"], "completed");
        assert!(stop_json["last_assistant_message"]
            .as_str()
            .unwrap()
            .contains("Use hook-provided context."));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_lifecycle_hooks_ignore_project_hooks_file_when_project_is_untrusted() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_AGENT_COMMAND",
            "KIANA_AGENT_HOOKS",
            "KIANA_SUBAGENT_START_HOOKS",
            "KIANA_SUBAGENT_STOP_HOOKS",
            "KIANA_HOOKS_FILE",
        ]);
        std::env::remove_var("HOME");
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_AGENT_HOOKS");
        std::env::remove_var("KIANA_SUBAGENT_START_HOOKS");
        std::env::remove_var("KIANA_SUBAGENT_STOP_HOOKS");
        std::env::remove_var("KIANA_HOOKS_FILE");
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'prompt=%s\\n' \"$KIANA_AGENT_PROMPT\"",
        );

        let root =
            std::env::temp_dir().join(format!("kiana-agent-project-hooks-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        fs::create_dir_all(repo.join(".kiana")).unwrap();
        fs::write(
            repo.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "SubagentStart": [
                    "printf '%s' '{\"decision\":\"block\",\"reason\":\"project lifecycle hook should not run\"}'"
                ]
            })
            .to_string(),
        )
        .unwrap();
        let input = json!({ "prompt": "Inspect project hooks" });
        let mut trusted_context = test_context_with_cwd(&root.join("trusted-bg"), &repo);

        let trusted_error = AgentTool::new()
            .call(&input, &mut trusted_context)
            .await
            .unwrap_err();
        assert!(trusted_error
            .to_string()
            .contains("project lifecycle hook should not run"));

        let mut untrusted_context = test_context_with_cwd(&root.join("untrusted-bg"), &repo);
        untrusted_context
            .app_state
            .insert("project_trusted".to_string(), json!(false));
        let output = AgentTool::new()
            .call(&input, &mut untrusted_context)
            .await
            .unwrap();

        assert_eq!(output.data["status"], "completed");
        assert!(output.data["stdout"]
            .as_str()
            .unwrap()
            .contains("Inspect project hooks"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_lifecycle_hooks_load_project_hooks_alongside_home_hooks_when_trusted() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let _env = EnvSnapshot::take(&[
            "HOME",
            "KIANA_HOME",
            "KIANA_AGENT_COMMAND",
            "KIANA_AGENT_HOOKS",
            "KIANA_SUBAGENT_START_HOOKS",
            "KIANA_SUBAGENT_STOP_HOOKS",
            "KIANA_HOOKS_FILE",
        ]);
        std::env::remove_var("KIANA_HOME");
        std::env::remove_var("KIANA_AGENT_HOOKS");
        std::env::remove_var("KIANA_SUBAGENT_START_HOOKS");
        std::env::remove_var("KIANA_SUBAGENT_STOP_HOOKS");
        std::env::remove_var("KIANA_HOOKS_FILE");
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'prompt=%s\\n' \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!(
            "kiana-agent-trusted-project-hooks-{}",
            Uuid::new_v4()
        ));
        let home = root.join("home");
        let repo = root.join("repo");
        fs::create_dir_all(home.join(".kiana")).unwrap();
        fs::create_dir_all(repo.join(".kiana")).unwrap();
        std::env::set_var("HOME", &home);
        fs::write(
            home.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "SubagentStart": [
                    "printf '%s' '{\"decision\":\"block\",\"reason\":\"home lifecycle hook blocked\"}'"
                ]
            })
            .to_string(),
        )
        .unwrap();
        fs::write(
            repo.join(".kiana").join("hooks.json"),
            serde_json::json!({
                "SubagentStart": [
                    "printf '%s' '{\"decision\":\"block\",\"reason\":\"project lifecycle hook blocked\"}'"
                ]
            })
            .to_string(),
        )
        .unwrap();
        let mut context = test_context_with_cwd(&root.join("bg"), &repo);

        let error = AgentTool::new()
            .call(
                &json!({ "prompt": "Inspect trusted project hooks" }),
                &mut context,
            )
            .await
            .unwrap_err();
        let error = error.to_string();

        assert!(error.contains("home lifecycle hook blocked"));
        assert!(error.contains("project lifecycle hook blocked"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_loads_plugin_agent_definition_for_subagent_type() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        kiana_skills::clear_caches();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'type=%s model=%s mode=%s turns=%s desc=%s initial=%s system=%s tools=%s disallowed=%s skills=%s mcp=%s preloaded=%s prompt=%s\\n' \"$KIANA_AGENT_TYPE\" \"$KIANA_AGENT_MODEL\" \"$KIANA_AGENT_PERMISSION_MODE\" \"$KIANA_AGENT_MAX_TURNS\" \"$KIANA_AGENT_DESCRIPTION\" \"$KIANA_AGENT_INITIAL_PROMPT\" \"$KIANA_AGENT_SYSTEM_PROMPT\" \"$KIANA_AGENT_TOOLS\" \"$KIANA_AGENT_DISALLOWED_TOOLS\" \"$KIANA_AGENT_SKILLS\" \"$KIANA_AGENT_MCP_SERVERS\" \"$KIANA_AGENT_PRELOADED_SKILLS\" \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-plugin-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let bg = root.join("bg");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        let agents_dir = plugin_root.join("agents").join("git");
        let skill_dir = plugin_root.join("skills").join("review-helper");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&agents_dir).unwrap();
        fs::create_dir_all(&skill_dir).unwrap();
        write_plugin_manifest(&plugin_root, "review-tools", json!({}));
        fs::write(
            agents_dir.join("reviewer.md"),
            "---\nname: reviewer\nwhen-to-use: Reviews plugin-delivered changes\nmodel: inherit\npermissionMode: bypassPermissions\nmaxTurns: 5\ninitialPrompt: Inspect plugin docs first.\ntools:\n  - Read\n  - Bash(git:*)\ndisallowedTools:\n  - Write\nskills:\n  - review-tools:review-helper\nmcpServers:\n  docs:\n    command: docs-mcp\n---\nUse ${CLAUDE_PLUGIN_ROOT}/rubric.md before reviewing.\n",
        )
        .unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: Review Helper\ndescription: Plugin review help\n---\nApply the plugin review rubric.\n",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let mut context = test_context_with_cwd(&bg, &repo);
        let input = json!({
            "subagent_type": "review-tools:git:reviewer",
            "prompt": "Inspect plugin changes"
        });
        let validation = AgentTool::new().validate_input(&input, &context).await;
        assert!(validation.result, "{validation:?}");

        let output = AgentTool::new().call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["status"], "completed");
        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("type=review-tools:git:reviewer"));
        assert!(stdout.contains("model= mode="));
        assert!(stdout.contains("turns=5"));
        assert!(stdout.contains("desc=Reviews plugin-delivered changes"));
        assert!(stdout.contains("initial=Inspect plugin docs first."));
        assert!(stdout.contains("tools=[\"Read\",\"Bash(git:*)\"]"));
        assert!(stdout.contains("disallowed=[\"Write\"]"));
        assert!(stdout.contains("skills=[\"review-tools:review-helper\"]"));
        assert!(stdout.contains("mcp=null"));
        assert!(stdout.contains("mode= "));
        assert!(stdout.contains(&format!(
            "system=Use {}/rubric.md before reviewing.",
            plugin_root.to_string_lossy()
        )));
        assert!(stdout.contains("\"name\":\"review-tools:review-helper\""));
        assert!(stdout.contains("Apply the plugin review rubric."));
        assert!(stdout.contains("Inspect plugin changes"));

        let agent = resolve_agent_definition("review-tools:git:reviewer", &repo).unwrap();
        let args = agent_cli_args(
            &AgentInput {
                task: None,
                prompt: Some("Inspect plugin changes".to_string()),
                description: None,
                context: None,
                subagent_type: Some("review-tools:git:reviewer".to_string()),
                model: None,
                name: None,
                team_name: None,
                plan_mode_required: false,
                run_in_background: false,
                cwd: None,
                isolation: None,
                timeout_ms: None,
            },
            Some(&agent),
            "Inspect plugin changes",
            &[],
            None,
        )
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--agent")
                .map(|pair| pair[1].as_str()),
            Some("review-tools:git:reviewer")
        );
        let agents_json = args
            .windows(2)
            .find(|pair| pair[0] == "--agents")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(agents_json.contains("\"review-tools:git:reviewer\""));
        assert!(agents_json.contains("\"model\":\"inherit\""));
        assert!(!agents_json.contains("permissionMode"));
        assert!(!agents_json.contains("mcpServers"));

        std::env::remove_var("KIANA_AGENT_COMMAND");
        std::env::remove_var("KIANA_PLUGINS_DIR");
        kiana_skills::clear_caches();
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn plugin_agent_manifest_paths_load_and_reject_traversal() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-agent-manifest-test-{}",
            Uuid::new_v4()
        ));
        let repo = root.join("repo");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("ops");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(plugin_root.join("extra")).unwrap();
        write_plugin_manifest(
            &plugin_root,
            "ops",
            json!({ "agents": ["extra/runbook.md", "../outside.md"] }),
        );
        fs::write(
            plugin_root.join("extra").join("runbook.md"),
            "---\ndescription: Runs operational checks\n---\nCheck the deployment runbook.\n",
        )
        .unwrap();
        fs::write(
            plugins_dir.join("outside.md"),
            "---\nname: outside\ndescription: Should not load\n---\nOutside plugin root.\n",
        )
        .unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        let agent = resolve_agent_definition("ops:runbook", &repo).unwrap();
        assert_eq!(agent.description, "Runs operational checks");
        assert_eq!(agent.system_prompt, "Check the deployment runbook.");
        assert!(resolve_agent_definition("ops:outside", &repo).is_err());

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn disabled_plugin_agent_definition_is_not_resolved() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "kiana-plugin-agent-disabled-test-{}",
            Uuid::new_v4()
        ));
        let repo = root.join("repo");
        let plugins_dir = root.join("plugins");
        let plugin_root = plugins_dir.join("review-tools");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(plugin_root.join("agents")).unwrap();
        write_plugin_manifest(&plugin_root, "review-tools", json!({}));
        fs::write(
            plugin_root.join("agents").join("reviewer.md"),
            "---\ndescription: Reviews changes\n---\nReview carefully.\n",
        )
        .unwrap();
        kiana_types::plugin::set_plugin_enabled(&plugins_dir, "review-tools", false).unwrap();
        std::env::set_var("KIANA_PLUGINS_DIR", &plugins_dir);

        assert!(resolve_agent_definition("review-tools:reviewer", &repo).is_err());

        std::env::remove_var("KIANA_PLUGINS_DIR");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn agent_cli_args_forward_tool_filters_and_agent_definition_metadata() {
        let input = AgentInput {
            task: None,
            prompt: None,
            description: None,
            context: None,
            subagent_type: Some("reviewer".to_string()),
            model: None,
            name: None,
            team_name: None,
            plan_mode_required: false,
            run_in_background: false,
            cwd: None,
            isolation: None,
            timeout_ms: None,
        };
        let agent = AgentDefinition {
            agent_type: "reviewer".to_string(),
            description: "Reviews code".to_string(),
            system_prompt: "Review strictly.".to_string(),
            tools: Some(vec!["Bash(git:*)".to_string(), "Read".to_string()]),
            disallowed_tools: vec!["Write".to_string()],
            skills: vec!["refactor".to_string()],
            hooks: Vec::new(),
            memory: None,
            permission_mode: Some("plan".to_string()),
            max_turns: Some(3),
            mcp_servers: Some(json!({"docs": {"command": "docs-mcp"}})),
            isolation: None,
            model: None,
            initial_prompt: None,
            background: false,
            path: None,
        };

        let mcp_servers = json!({"docs": {"command": "docs-mcp"}});
        let args = agent_cli_args(
            &input,
            Some(&agent),
            "Inspect changes",
            &[],
            Some(&mcp_servers),
        )
        .into_iter()
        .map(|arg| arg.to_string_lossy().to_string())
        .collect::<Vec<_>>();

        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--tools")
                .map(|pair| pair[1].as_str()),
            Some("Bash,Read")
        );
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--allowed-tools")
                .map(|pair| pair[1].as_str()),
            Some("Bash(git:*),Read")
        );
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--disallowed-tools")
                .map(|pair| pair[1].as_str()),
            Some("Write")
        );
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--permission-mode")
                .map(|pair| pair[1].as_str()),
            Some("plan")
        );
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--max-turns")
                .map(|pair| pair[1].as_str()),
            Some("3")
        );
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "--mcp-config")
                .map(|pair| pair[1].as_str()),
            Some("{\"docs\":{\"command\":\"docs-mcp\"}}")
        );
        let agents_json = args
            .windows(2)
            .find(|pair| pair[0] == "--agents")
            .map(|pair| pair[1].as_str())
            .unwrap();
        assert!(agents_json.contains("\"tools\":[\"Bash(git:*)\",\"Read\"]"));
        assert!(agents_json.contains("\"disallowedTools\":[\"Write\"]"));
        assert!(agents_json.contains("\"skills\":[\"refactor\"]"));
        assert!(agents_json.contains("\"permissionMode\":\"plan\""));
        assert!(agents_json.contains("\"maxTurns\":3"));
        assert!(agents_json.contains("\"mcpServers\":{\"docs\":{\"command\":\"docs-mcp\"}}"));
    }

    #[test]
    fn team_background_agent_runner_script_uses_resident_teammate_mode() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::remove_var("KIANA_AGENT_COMMAND");
        let root =
            std::env::temp_dir().join(format!("kiana-agent-resident-script-{}", Uuid::new_v4()));
        let input = AgentInput {
            task: None,
            prompt: Some("Run team worker".to_string()),
            description: Some("Team worker".to_string()),
            context: None,
            subagent_type: None,
            model: None,
            name: Some("runner".to_string()),
            team_name: Some("review".to_string()),
            plan_mode_required: false,
            run_in_background: true,
            cwd: None,
            isolation: None,
            timeout_ms: None,
        };
        let team_agent = TeamAgentRuntime {
            team_name: "review".to_string(),
            agent_name: "runner".to_string(),
            agent_id: "runner@review".to_string(),
            task_list_id: "review".to_string(),
            team_file_path: root.join("teams/review/config.json"),
            mailbox_path: root.join("teams/review/inboxes/runner.json"),
            teams_root: root.join("teams"),
            tasks_root: root.join("tasks"),
            color: "green".to_string(),
            plan_mode_required: false,
        };

        let script = agent_runner_script(
            &input,
            None,
            &root.join("prompt.txt"),
            &root.join("agent.log"),
            &root.join("exit"),
            &[],
            None,
            None,
            Some(&team_agent),
        )
        .unwrap();

        assert!(script.contains("-p --resident-teammate \"$KIANA_AGENT_PROMPT\""));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_definition_can_force_background_execution() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'forced background type=%s system=%s prompt=%s\\n' \"$KIANA_AGENT_TYPE\" \"$KIANA_AGENT_SYSTEM_PROMPT\" \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let bg = root.join("bg");
        let agents_dir = repo.join(".claude").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::write(
            agents_dir.join("sweeper.md"),
            "---\nname: sweeper\ndescription: Runs independently\nbackground: true\n---\nYou run independently.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(&bg, &repo);
        let started = AgentTool::new()
            .call(
                &json!({
                    "subagent_type": "sweeper",
                    "prompt": "Clean up diagnostics"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(started.data["status"], "async_launched");
        let task_id = started.data["agentId"].as_str().unwrap().to_string();

        let output = TaskOutputTool::new()
            .call(
                &json!({
                    "task_id": task_id,
                    "block": true,
                    "timeout_ms": 5000
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["subagent_type"], "sweeper");
        assert!(output.data["task"]["agent_definition_path"]
            .as_str()
            .unwrap()
            .ends_with("sweeper.md"));
        assert!(output.data["task"]["output"]
            .as_str()
            .unwrap()
            .contains("forced background type=sweeper system=You run independently. prompt=Clean up diagnostics"));

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn unknown_subagent_type_is_rejected() {
        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let context = test_context_with_cwd(&root.join("bg"), &repo);

        let validation = AgentTool::new()
            .validate_input(
                &json!({
                    "subagent_type": "missing",
                    "prompt": "Inspect"
                }),
                &context,
            )
            .await;
        assert!(!validation.result);
        assert!(validation
            .message
            .as_deref()
            .unwrap()
            .contains("Unknown agent type: missing"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn isolation_cannot_be_combined_with_explicit_cwd() {
        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let context = test_context_with_cwd(&root.join("bg"), &repo);

        let validation = AgentTool::new()
            .validate_input(
                &json!({
                    "prompt": "Inspect isolated workspace",
                    "cwd": ".",
                    "isolation": "worktree"
                }),
                &context,
            )
            .await;

        assert!(!validation.result);
        assert!(validation
            .message
            .as_deref()
            .unwrap()
            .contains("cwd cannot be combined with isolation"));

        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn agent_frontmatter_isolation_runs_in_snapshot_copy() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'pwd=%s isolation=%s path=%s strategy=%s file=%s\\n' \"$PWD\" \"$KIANA_AGENT_ISOLATION\" \"$KIANA_AGENT_WORKTREE_PATH\" \"$KIANA_AGENT_WORKTREE_STRATEGY\" \"$(cat repo.txt)\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let bg = root.join("bg");
        let agents_dir = repo.join(".claude").join("agents");
        fs::create_dir_all(&agents_dir).unwrap();
        fs::write(repo.join("repo.txt"), "copied from source").unwrap();
        fs::write(
            agents_dir.join("isolated.md"),
            "---\nname: isolated\ndescription: Runs in an isolated workspace\nisolation: worktree\n---\nUse an isolated workspace.\n",
        )
        .unwrap();

        let mut context = test_context_with_cwd(&bg, &repo);
        let input = json!({
            "subagent_type": "isolated",
            "prompt": "Inspect isolated workspace"
        });
        let validation = AgentTool::new().validate_input(&input, &context).await;
        assert!(validation.result, "{validation:?}");

        let output = AgentTool::new().call(&input, &mut context).await.unwrap();
        assert_eq!(output.data["status"], "completed");
        assert_eq!(output.data["isolation"], "worktree");
        assert_eq!(output.data["worktree_strategy"], "snapshot_copy");
        let worktree_path = output.data["worktree_path"].as_str().unwrap();
        assert_ne!(worktree_path, repo.to_string_lossy());
        assert!(std::path::Path::new(worktree_path)
            .join("repo.txt")
            .is_file());

        let stdout = output.data["stdout"].as_str().unwrap();
        assert!(stdout.contains("isolation=worktree"));
        assert!(stdout.contains("strategy=snapshot_copy"));
        assert!(stdout.contains("file=copied from source"));
        assert!(
            shell_path_candidates(worktree_path)
                .iter()
                .any(|path| stdout.contains(&format!("pwd={path}"))),
            "{stdout}"
        );
        assert!(
            shell_path_candidates(worktree_path)
                .iter()
                .any(|path| stdout.contains(&format!("path={path}"))),
            "{stdout}"
        );

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn background_agent_records_isolation_metadata() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'background isolation=%s path=%s strategy=%s file=%s\\n' \"$KIANA_AGENT_ISOLATION\" \"$KIANA_AGENT_WORKTREE_PATH\" \"$KIANA_AGENT_WORKTREE_STRATEGY\" \"$(cat repo.txt)\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        let bg = root.join("bg");
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("repo.txt"), "background copy").unwrap();
        let mut context = test_context_with_cwd(&bg, &repo);
        let started = AgentTool::new()
            .call(
                &json!({
                    "prompt": "Review isolated task output wiring",
                    "description": "Review isolation wiring",
                    "run_in_background": true,
                    "isolation": "worktree"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(started.data["status"], "async_launched");
        assert_eq!(started.data["isolation"], "worktree");
        assert_eq!(started.data["worktree_strategy"], "snapshot_copy");
        let task_id = started.data["agentId"].as_str().unwrap().to_string();
        let worktree_path = started.data["worktree_path"].as_str().unwrap().to_string();
        assert!(std::path::Path::new(&worktree_path)
            .join("repo.txt")
            .is_file());

        let output = TaskOutputTool::new()
            .call(
                &json!({
                    "task_id": task_id,
                    "block": true,
                    "timeout_ms": 5000
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["status"], "completed");
        assert_eq!(output.data["task"]["isolation"], "worktree");
        assert_eq!(output.data["task"]["worktree_path"], worktree_path);
        assert_eq!(output.data["task"]["worktree_strategy"], "snapshot_copy");
        assert!(output.data["task"]["output"]
            .as_str()
            .unwrap()
            .contains("background isolation=worktree"));
        assert!(output.data["task"]["output"]
            .as_str()
            .unwrap()
            .contains("file=background copy"));

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn background_agent_registers_teammate_and_inherits_team_env() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'bg team=%s name=%s id=%s tasklist=%s mailbox=%s prompt=%s\\n' \"$KIANA_TEAM_NAME\" \"$KIANA_AGENT_NAME\" \"$KIANA_AGENT_ID\" \"$KIANA_TASK_LIST_ID\" \"$KIANA_TEAM_MAILBOX\" \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-team-bg-{}", Uuid::new_v4()));
        let repo = root.join("repo");
        fs::create_dir_all(&repo).unwrap();
        let mut context = test_context_with_cwd(&root.join("bg"), &repo);
        TeamCreateTool::new()
            .call(&json!({ "team_name": "review" }), &mut context)
            .await
            .unwrap();

        let started = AgentTool::new()
            .call(
                &json!({
                    "name": "runner",
                    "prompt": "Run background review",
                    "run_in_background": true,
                    "team_name": "review"
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(started.data["status"], "async_launched");
        assert_eq!(started.data["team_name"], "review");
        assert_eq!(started.data["agent_name"], "runner");
        assert_eq!(started.data["teammate_id"], "runner@review");
        let task_id = started.data["task_id"].as_str().unwrap().to_string();

        let output = TaskOutputTool::new()
            .call(
                &json!({
                    "task_id": task_id,
                    "block": true,
                    "timeout_ms": 5000
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["team_name"], "review");
        assert_eq!(output.data["task"]["agent_name"], "runner");
        assert_eq!(output.data["task"]["teammate_id"], "runner@review");
        assert_eq!(output.data["task"]["task_list_id"], "review");
        let log = output.data["task"]["output"].as_str().unwrap();
        assert!(log.contains("bg team=review"));
        assert!(log.contains("name=runner"));
        assert!(log.contains("id=runner@review"));
        assert!(log.contains("tasklist=review"));
        assert!(log.contains("prompt=Run background review"));

        let teams_root = context.app_state["teams_root"].as_str().unwrap();
        let team_file = read_json(std::path::Path::new(teams_root).join("review/config.json"));
        let runner = team_file["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|member| member["name"] == "runner")
            .unwrap();
        assert_eq!(runner["agentId"], "runner@review");
        assert_eq!(runner["backendType"], "background-agent");
        assert_eq!(runner["isActive"], false);
        assert_eq!(runner["lastExitReason"], "completed");

        let inbox = read_json(std::path::Path::new(teams_root).join("review/inboxes/runner.json"));
        assert_eq!(inbox[0]["text"], "Run background review");

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn background_agent_output_is_readable_with_task_output() {
        let _guard = crate::test_support::env_lock().lock().unwrap();
        std::env::set_var(
            "KIANA_AGENT_COMMAND",
            "printf 'background agent saw: %s\\n' \"$KIANA_AGENT_PROMPT\"",
        );

        let root = std::env::temp_dir().join(format!("kiana-agent-test-{}", Uuid::new_v4()));
        let mut context = test_context(&root);
        let started = AgentTool::new()
            .call(
                &json!({
                    "prompt": "Review task output wiring",
                    "description": "Review wiring",
                    "run_in_background": true
                }),
                &mut context,
            )
            .await
            .unwrap();
        assert_eq!(started.data["status"], "async_launched");
        let task_id = started.data["agentId"].as_str().unwrap().to_string();

        let output = TaskOutputTool::new()
            .call(
                &json!({
                    "task_id": task_id,
                    "block": true,
                    "timeout_ms": 5000
                }),
                &mut context,
            )
            .await
            .unwrap();

        assert_eq!(output.data["retrieval_status"], "success");
        assert_eq!(output.data["task"]["status"], "completed");
        assert_eq!(output.data["task"]["task_type"], "background");
        assert!(output.data["task"]["output"]
            .as_str()
            .unwrap()
            .contains("background agent saw: Review task output wiring"));

        std::env::remove_var("KIANA_AGENT_COMMAND");
        let _ = fs::remove_dir_all(root);
    }

    fn shell_path_candidates(path: &str) -> Vec<String> {
        let mut candidates = vec![path.to_string()];
        #[cfg(windows)]
        {
            let normalized = path.replace('\\', "/");
            if let Some((drive, rest)) = normalized.split_once(':') {
                if drive.len() == 1 && drive.as_bytes()[0].is_ascii_alphabetic() {
                    candidates.push(format!(
                        "/{}/{}",
                        drive.to_ascii_lowercase(),
                        rest.trim_start_matches('/')
                    ));
                }
            }
            for temp_var in ["TEMP", "TMP"] {
                if let Ok(temp_root) = std::env::var(temp_var) {
                    let temp_root = temp_root.replace('\\', "/");
                    if let Some(rest) = strip_prefix_ignore_ascii_case(
                        normalized.trim_end_matches('/'),
                        temp_root.trim_end_matches('/'),
                    ) {
                        candidates.push(format!("/tmp/{}", rest.trim_start_matches('/')));
                    }
                }
            }
        }
        candidates
    }

    #[cfg(windows)]
    fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
        if value.len() < prefix.len() || !value[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return None;
        }
        Some(&value[prefix.len()..])
    }
}
