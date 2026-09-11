//! runner 边界上的命令与事件契约。
//!
//! 本 crate 只定义 `RunnerCommand`/`RunnerEvent` 的可序列化 wire shape。Runner 可以请求
//! capability，但不能直接执行；`CapabilityRequested` 必须回到 daemon broker，结果也
//! 必须按 run ID 关联。`project_trusted`、sandbox 和 instructions 是上游快照/输入，不是
//! runner 自行授予权限的依据。

use kiana_domain::{CapabilityRequest, CapabilityResult, ConversationMessage, RunId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// harness 缺省的只读沙箱档位。
pub const DEFAULT_HARNESS_SANDBOX: &str = "read-only";
/// harness 支持的项目内可写沙箱档位。
pub const HARNESS_SANDBOX_WORKSPACE_WRITE: &str = "workspace-write";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
/// daemon 发给 runner 的控制命令。
pub enum RunnerCommand {
    /// 启动新 run，可带历史、项目根、沙箱和系统指令。
    Start {
        /// run 稳定 ID。
        run_id: RunId,
        /// 当前提示词。
        prompt: String,
        #[serde(default)]
        /// 结构化历史消息；缺失时按空历史处理。
        history: Vec<ConversationMessage>,
        #[serde(default)]
        /// 项目根目录。
        project_root: String,
        #[serde(default = "default_harness_sandbox")]
        /// 请求沙箱档位。
        sandbox: String,
        #[serde(default)]
        /// 上游注入的系统指令。
        instructions: String,
        #[serde(default)]
        /// 项目 trust 快照；不能替代 core 的实时授权。
        project_trusted: bool,
        #[serde(default = "default_harness_max_steps")]
        /// 请求级步数上限；由上游 ControlPlane 按环境覆盖、角色快照或默认值解析。
        max_steps_per_turn: u32,
    },
    /// 将 capability handler 的结果回传给 runner。
    CapabilityResult {
        /// 对应 run ID。
        run_id: RunId,
        /// handler 的结构化结果。
        result: CapabilityResult,
    },
    /// 继续已有 run。
    Continue {
        /// 对应 run ID。
        run_id: RunId,
        /// 新的提示词。
        prompt: String,
    },
    /// 请求取消 run。
    Cancel {
        /// 对应 run ID。
        run_id: RunId,
        /// 取消原因。
        reason: String,
    },
}

fn default_harness_sandbox() -> String {
    DEFAULT_HARNESS_SANDBOX.to_owned()
}

fn default_harness_max_steps() -> u32 {
    32
}

impl RunnerCommand {
    /// 构造默认只读、无项目上下文的新 run 命令。
    pub fn start(run_id: RunId, prompt: impl Into<String>) -> Self {
        Self::Start {
            run_id,
            prompt: prompt.into(),
            history: Vec::new(),
            project_root: String::new(),
            sandbox: default_harness_sandbox(),
            instructions: String::new(),
            project_trusted: false,
            max_steps_per_turn: 32,
        }
    }

    /// 构造带项目根和沙箱的新 run 命令。
    pub fn start_in(
        run_id: RunId,
        prompt: impl Into<String>,
        project_root: impl Into<String>,
        sandbox: impl Into<String>,
    ) -> Self {
        Self::start_in_with_instructions(
            run_id,
            prompt,
            project_root,
            sandbox,
            String::new(),
            false,
        )
    }

    /// 构造带系统指令和 trust 快照的新 run 命令。
    pub fn start_in_with_instructions(
        run_id: RunId,
        prompt: impl Into<String>,
        project_root: impl Into<String>,
        sandbox: impl Into<String>,
        instructions: impl Into<String>,
        project_trusted: bool,
    ) -> Self {
        Self::Start {
            run_id,
            prompt: prompt.into(),
            history: Vec::new(),
            project_root: project_root.into(),
            sandbox: sandbox.into(),
            instructions: instructions.into(),
            project_trusted,
            max_steps_per_turn: 32,
        }
    }

    /// 构造带结构化历史的新 run 命令。
    pub fn start_in_with_history(
        run_id: RunId,
        prompt: impl Into<String>,
        history: Vec<ConversationMessage>,
        project_root: impl Into<String>,
        sandbox: impl Into<String>,
        instructions: impl Into<String>,
        project_trusted: bool,
        max_steps_per_turn: u32,
    ) -> Self {
        Self::Start {
            run_id,
            prompt: prompt.into(),
            history,
            project_root: project_root.into(),
            sandbox: sandbox.into(),
            instructions: instructions.into(),
            project_trusted,
            max_steps_per_turn,
        }
    }

    /// 构造继续命令。
    pub fn continue_run(run_id: RunId, prompt: impl Into<String>) -> Self {
        Self::Continue {
            run_id,
            prompt: prompt.into(),
        }
    }

    /// 取得任意命令关联的 run ID。
    pub const fn run_id(&self) -> RunId {
        match self {
            Self::Start { run_id, .. }
            | Self::CapabilityResult { run_id, .. }
            | Self::Continue { run_id, .. }
            | Self::Cancel { run_id, .. } => *run_id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
/// runner 返回给 daemon 的事件。
pub enum RunnerEvent {
    /// run 已接受并开始。
    Started {
        /// 对应 run ID。
        run_id: RunId,
    },
    /// 模型输出增量；是否持久化由 daemon 事件适配器决定。
    Delta {
        /// 对应 run ID。
        run_id: RunId,
        /// 增量文本。
        text: String,
    },
    /// runner 请求 daemon 执行一个能力。
    CapabilityRequested {
        /// 对应 run ID。
        run_id: RunId,
        /// 待审批/执行的能力请求。
        request: CapabilityRequest,
    },
    /// runner 认为 run 已完成；仍需由 daemon 形成最终 receipt。
    Completed {
        /// 对应 run ID。
        run_id: RunId,
        /// 模型输出 JSON。
        output: Value,
    },
    /// runner 报告失败。
    Failed {
        /// 对应 run ID。
        run_id: RunId,
        /// 失败原因。
        error: String,
    },
    /// 上下文压缩摘要事件。
    Compacted {
        /// 对应 run ID。
        run_id: RunId,
        /// 压缩前估算 token 数。
        tokens_before: u64,
        /// 压缩后估算 token 数。
        tokens_after: u64,
        /// 是否包含摘要文本。
        summary_present: bool,
    },
}

impl RunnerEvent {
    /// 取得事件关联的 run ID。
    pub const fn run_id(&self) -> RunId {
        match self {
            Self::Started { run_id }
            | Self::Delta { run_id, .. }
            | Self::CapabilityRequested { run_id, .. }
            | Self::Completed { run_id, .. }
            | Self::Failed { run_id, .. }
            | Self::Compacted { run_id, .. } => *run_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_protocol_round_trips_without_implementation_types() {
        let command = RunnerCommand::start_in(
            RunId::new(),
            "inspect architecture",
            "/repo",
            DEFAULT_HARNESS_SANDBOX,
        );
        let encoded = serde_json::to_vec(&command).unwrap();
        assert_eq!(
            serde_json::from_slice::<RunnerCommand>(&encoded).unwrap(),
            command
        );
    }

    #[test]
    fn start_command_defaults_to_read_only_sandbox() {
        let command = RunnerCommand::start(RunId::new(), "hello");
        let encoded = serde_json::to_value(&command).unwrap();
        assert_eq!(encoded["sandbox"], DEFAULT_HARNESS_SANDBOX);
        assert_eq!(encoded["project_root"], "");
        assert_eq!(encoded["instructions"], "");
        assert_eq!(encoded["project_trusted"], false);
    }

    #[test]
    fn start_command_deserializes_legacy_payload_without_instructions() {
        let run_id = RunId::new();
        let encoded = serde_json::json!({
            "command": "start",
            "run_id": run_id,
            "prompt": "hello"
        });
        let command: RunnerCommand = serde_json::from_value(encoded).unwrap();
        match command {
            RunnerCommand::Start {
                instructions,
                project_trusted,
                sandbox,
                project_root,
                ..
            } => {
                assert_eq!(instructions, "");
                assert!(!project_trusted);
                assert_eq!(sandbox, DEFAULT_HARNESS_SANDBOX);
                assert_eq!(project_root, "");
            }
            other => panic!("expected start: {other:?}"),
        }
    }

    #[test]
    fn continue_command_round_trips_run_id_and_prompt() {
        let command = RunnerCommand::continue_run(RunId::new(), "keep going");
        let encoded = serde_json::to_vec(&command).unwrap();
        assert_eq!(
            serde_json::from_slice::<RunnerCommand>(&encoded).unwrap(),
            command
        );
    }

    #[test]
    fn compacted_event_round_trips_token_counts() {
        let run_id = RunId::new();
        let event = RunnerEvent::Compacted {
            run_id,
            tokens_before: 500,
            tokens_after: 120,
            summary_present: true,
        };
        let encoded = serde_json::to_value(&event).unwrap();
        assert_eq!(encoded["event"], "compacted");
        assert_eq!(encoded["tokens_before"], 500);
        assert_eq!(encoded["summary_present"], true);
        assert_eq!(
            serde_json::from_value::<RunnerEvent>(encoded).unwrap(),
            event
        );
    }
}
