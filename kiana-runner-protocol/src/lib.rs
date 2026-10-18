//! runner 边界上的命令与事件契约。
//!
//! 本 crate 只定义 `RunnerCommand`/`RunnerEvent` 的可序列化 wire shape。Runner 可以请求
//! capability，但不能直接执行；`CapabilityRequested` 必须回到 daemon broker，结果也
//! 必须按 run ID 关联。`project_trusted`、sandbox 和 instructions 是上游快照/输入，不是
//! runner 自行授予权限的依据。

use kiana_domain::{
    CapabilityRequest, CapabilityResult, ClarificationRequest, ConversationMessage, InputId,
    RequestId, RunId, TurnId,
};
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
        /// server-derived turn identity; absent only for legacy v1 callers.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<TurnId>,
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
    /// Queue an input for the next safe Runner boundary. This never authorizes a capability or
    /// wakes an idle run; ControlPlane owns the accepted/claimed facts.
    Inject {
        run_id: RunId,
        input_id: InputId,
        source: String,
        target: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_turn_id: Option<TurnId>,
        text: String,
    },
    /// 继续已有 run。
    Continue {
        /// 对应 run ID。
        run_id: RunId,
        /// 新的提示词。
        prompt: String,
    },
    /// Resume a previously restored run after ControlPlane has re-authorized and claimed it.
    /// The checkpoint is installed through `RunnerPort::restore`; this command only re-enters
    /// the canonical model loop and carries no caller-owned authority or prompt.
    Resume {
        /// 对应 run ID。
        run_id: RunId,
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
            turn_id: None,
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
            turn_id: None,
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
        Self::start_in_with_history_and_turn(
            run_id,
            prompt,
            history,
            project_root,
            sandbox,
            instructions,
            project_trusted,
            max_steps_per_turn,
            None,
        )
    }

    /// Construct a new run command with the server-owned Turn identity.
    pub fn start_in_with_history_and_turn(
        run_id: RunId,
        prompt: impl Into<String>,
        history: Vec<ConversationMessage>,
        project_root: impl Into<String>,
        sandbox: impl Into<String>,
        instructions: impl Into<String>,
        project_trusted: bool,
        max_steps_per_turn: u32,
        turn_id: Option<TurnId>,
    ) -> Self {
        Self::Start {
            run_id,
            turn_id,
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
            | Self::Inject { run_id, .. }
            | Self::Continue { run_id, .. }
            | Self::Resume { run_id }
            | Self::Cancel { run_id, .. } => *run_id,
        }
    }

    /// 构造恢复命令。恢复材料必须先由 ControlPlane 校验并通过 `restore` 安装。
    pub fn resume(run_id: RunId) -> Self {
        Self::Resume { run_id }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
/// runner 返回给 daemon 的事件。
pub enum RunnerEvent {
    ToolCancelled {
        run_id: RunId,
        request_id: RequestId,
        call_id: String,
        result: Value,
    },
    /// Effective model request provenance and provider usage for each step, including tool turns.
    ModelTurn {
        run_id: RunId,
        step: u32,
        metadata: Value,
    },

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
    /// The model requested a bounded human clarification. Core must persist and project the
    /// question before any answer is accepted; this event is not an approval request.
    ClarificationRequested {
        run_id: RunId,
        request: ClarificationRequest,
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
            | Self::ToolCancelled { run_id, .. }
            | Self::ModelTurn { run_id, .. }
            | Self::Delta { run_id, .. }
            | Self::CapabilityRequested { run_id, .. }
            | Self::ClarificationRequested { run_id, .. }
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

/// 稳定错误码：模型步在 unwind 中中止，结果无法举证。
///
/// 这是 `RunnerEvent::Failed` 的 payload，Core 必须把它投影成 `ResultUnknown`，
/// 不能降格成普通 failed。
pub const HARNESS_PANIC_ERROR: &str = "result_unknown:harness_panic";

/// panic 后无法举证的单个资源。
///
/// `stop_confirmed` / `effect_known` 是**证据位**，不是「希望它已停」的标记。Runner
/// 捕获 unwind 时两者都只能为 `false`，因为 unwind 之后没有任何可信的终止证据。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerPanicResource {
    /// 资源稳定 ID（如 `model_task`、`tool_future:0`）。
    pub resource_id: String,
    /// 资源类别，由 Core 映射到 `CleanupResourceKind`。
    pub kind: String,
    /// 该资源是否要求停止确认才能释放。
    pub requires_stop_confirmation: bool,
    /// 是否有确认的停止证据。
    pub stop_confirmed: bool,
    /// 副作用是否已知。
    pub effect_known: bool,
    /// 稳定原因码；不包含 panic payload 原文。
    pub reason: String,
}

/// 一次 panic 中止的 run 留下的资源证据。
///
/// 它只说明「哪些资源欠一次对账」，不证明任何外部效果已停；Core 用它构造
/// `CleanupCause::Panic` 的 `ResourceCleanupPlan`。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerPanicRecord {
    /// 被中止的 run。
    pub run_id: RunId,
    /// 稳定原因码（`panic_unwound` / `panic_payload_not_string`）。
    pub reason: String,
    /// 欠对账的资源列表。
    pub resources: Vec<RunnerPanicResource>,
}

/// 兼容别名：Runner 捕获 unwind 后交给 Core 的 panic 证据。
pub type HarnessPanicRecord = RunnerPanicRecord;

/// 兼容别名：panic 证据中的单个资源。
pub type HarnessPanicResource = RunnerPanicResource;
