//! Owned in-process agent harness.
//!
//! Loop and inbox semantics are derived from DeepSeek Harness (MIT)
//! `packages/core/agent-loop` + `packages/core/agent`. Model-visible tool names
//! and item shapes are derived from OpenAI Codex (Apache-2.0) `codex-rs/exec`
//! and `codex-rs/protocol` (`shell`, `apply_patch`).
//!
//! Copied into Kiana; `reference/` is audit-only and is not a workspace member.
//! Tools never execute here. Each call becomes a `CapabilityRequest` for the
//! control plane broker.

use crate::compact::{
    compact_if_needed, COMPACT_USER_MESSAGE_MAX_TOKENS, DEFAULT_COMPACT_TRIGGER_TOKENS,
};
use crate::inbox::{Inbox, InboxMessage, InboxTarget};
use crate::model::{
    ModelClient, ModelMessage, ModelRequest, ModelToolCall, ScriptedModel, UnavailableModel,
};
use crate::tools::{capability_for_tool, tool_schemas};
use async_trait::async_trait;
use kiana_domain::{CapabilityResult, RunId};
use kiana_ports::{PortError, RunnerPort};
use kiana_runner_protocol::{
    RunnerCommand, RunnerEvent, DEFAULT_HARNESS_SANDBOX, HARNESS_SANDBOX_WORKSPACE_WRITE,
};
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

pub const HARNESS_ID: &str = "kiana-harness";
pub const HARNESS_RESULT_SCHEMA: &str = "kiana.harness-result.v1";
const ENV_HARNESS_SCRIPT: &str = "KIANA_HARNESS_SCRIPT";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RuntimeConfig {
    pub max_steps_per_turn: u32,
    pub compact_trigger_tokens: usize,
    pub compact_user_message_max_tokens: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_steps_per_turn: 32,
            compact_trigger_tokens: DEFAULT_COMPACT_TRIGGER_TOKENS,
            compact_user_message_max_tokens: COMPACT_USER_MESSAGE_MAX_TOKENS,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KianaHarnessError {
    #[error("kiana_harness_unavailable:{0}")]
    Unavailable(String),
    #[error("kiana_harness_failed:{0}")]
    Failed(String),
}

struct ActiveRun {
    run_id: RunId,
    sandbox: String,
    project_root: String,
    inbox: Inbox,
    messages: Vec<ModelMessage>,
    pending_tools: VecDeque<(kiana_domain::RequestId, ModelToolCall)>,
    steps: u32,
    last_text: String,
}

pub struct KianaHarness {
    model: Arc<dyn ModelClient>,
    runs: Mutex<HashMap<RunId, ActiveRun>>,
    compact_trigger_tokens: usize,
    compact_user_message_max_tokens: usize,
    max_steps_per_turn: u32,
}

impl Default for KianaHarness {
    fn default() -> Self {
        Self::from_env()
    }
}

impl KianaHarness {
    pub fn new(model: Arc<dyn ModelClient>) -> Self {
        Self::with_config(model, RuntimeConfig::default())
    }

    pub fn with_config(model: Arc<dyn ModelClient>, config: RuntimeConfig) -> Self {
        Self {
            model,
            runs: Mutex::new(HashMap::new()),
            compact_trigger_tokens: config.compact_trigger_tokens,
            compact_user_message_max_tokens: config.compact_user_message_max_tokens,
            max_steps_per_turn: config.max_steps_per_turn.max(1),
        }
    }

    pub fn config(&self) -> RuntimeConfig {
        RuntimeConfig {
            max_steps_per_turn: self.max_steps_per_turn,
            compact_trigger_tokens: self.compact_trigger_tokens,
            compact_user_message_max_tokens: self.compact_user_message_max_tokens,
        }
    }

    pub fn with_compact_budget(mut self, trigger_tokens: usize, retain_tokens: usize) -> Self {
        self.compact_trigger_tokens = trigger_tokens;
        self.compact_user_message_max_tokens = retain_tokens;
        self
    }

    pub fn with_max_steps(mut self, max_steps_per_turn: u32) -> Self {
        self.max_steps_per_turn = max_steps_per_turn.max(1);
        self
    }

    pub fn from_env() -> Self {
        let model: Arc<dyn ModelClient> = match std::env::var(ENV_HARNESS_SCRIPT) {
            Ok(path) if !path.trim().is_empty() => match ScriptedModel::from_json_path(path.trim())
            {
                Ok(model) => Arc::new(model),
                Err(error) => Arc::new(UnavailableModel::new(error)),
            },
            _ => Arc::new(UnavailableModel::default()),
        };
        Self::new(model)
    }

    /// DeepSeek `inject`: stage steering text for the next step boundary.
    /// The run must already be stored (typically paused on a capability).
    pub fn inject(&self, run_id: RunId, text: impl Into<String>) -> Result<(), KianaHarnessError> {
        self.insert_next_step(run_id, text)
    }

    /// DeepSeek `steer`: same inbox target as inject. Wakeup is the next
    /// `model_step` after the in-flight capability returns; we do not expand
    /// the runner protocol with a concurrent wake command.
    pub fn steer(&self, run_id: RunId, text: impl Into<String>) -> Result<(), KianaHarnessError> {
        self.insert_next_step(run_id, text)
    }

    fn insert_next_step(
        &self,
        run_id: RunId,
        text: impl Into<String>,
    ) -> Result<(), KianaHarnessError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(KianaHarnessError::Failed(
                "inbox_message_required".to_owned(),
            ));
        }
        let mut runs = self
            .runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        let run = runs
            .get_mut(&run_id)
            .ok_or_else(|| KianaHarnessError::Failed("run_not_found".to_owned()))?;
        run.inbox
            .insert(InboxTarget::NextStep, InboxMessage::user(text));
        Ok(())
    }

    pub async fn send(
        &self,
        command: RunnerCommand,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        self.dispatch(command).await
    }

    async fn dispatch(
        &self,
        command: RunnerCommand,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        match command {
            RunnerCommand::Start {
                run_id,
                prompt,
                project_root,
                sandbox,
                instructions,
                project_trusted: _,
            } => {
                self.start(run_id, prompt, sandbox, project_root, instructions)
                    .await
            }
            RunnerCommand::CapabilityResult { run_id, result } => {
                self.on_capability_result(run_id, result).await
            }
            RunnerCommand::Continue { run_id, prompt } => self.continue_run(run_id, prompt).await,
            RunnerCommand::Cancel { run_id, reason } => self.cancel(run_id, reason),
        }
    }

    async fn start(
        &self,
        run_id: RunId,
        prompt: String,
        sandbox: String,
        project_root: String,
        instructions: String,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        if self.has_run(run_id)? {
            return Ok(vec![RunnerEvent::Failed {
                run_id,
                error: "run_already_exists".to_owned(),
            }]);
        }
        let sandbox = normalize_sandbox(&sandbox)?;
        let mut run = ActiveRun {
            run_id,
            sandbox: sandbox.to_owned(),
            project_root,
            inbox: Inbox::default(),
            messages: Vec::new(),
            pending_tools: VecDeque::new(),
            steps: 0,
            last_text: String::new(),
        };
        if !instructions.trim().is_empty() {
            run.messages.push(ModelMessage::system(instructions));
        }
        run.inbox
            .insert(InboxTarget::NextTurn, InboxMessage::user(prompt));
        for message in run.inbox.claim(InboxTarget::NextTurn) {
            run.messages.push(ModelMessage::user(message.text));
        }

        let mut events = vec![RunnerEvent::Started { run_id }];
        events.extend(self.model_step(&mut run).await);
        self.store_unless_terminal(run, &events)?;
        Ok(events)
    }

    async fn on_capability_result(
        &self,
        run_id: RunId,
        result: CapabilityResult,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        let mut run = self.take_run(run_id)?;
        let Some((expected_id, call)) = run.pending_tools.pop_front() else {
            return Ok(vec![RunnerEvent::Failed {
                run_id,
                error: "unexpected_capability_result".to_owned(),
            }]);
        };
        if expected_id != result.request_id {
            return Ok(vec![RunnerEvent::Failed {
                run_id,
                error: "capability_result_mismatch".to_owned(),
            }]);
        }

        run.messages
            .push(ModelMessage::tool(call.id, tool_result_text(&result)));

        let events = if let Some((request_id, next_call)) = run.pending_tools.front().cloned() {
            let _ = request_id;
            match emit_tool_request(&mut run, &next_call) {
                Ok(event) => vec![event],
                Err(error) => vec![RunnerEvent::Failed { run_id, error }],
            }
        } else {
            self.model_step(&mut run).await
        };
        self.store_unless_terminal(run, &events)?;
        Ok(events)
    }

    async fn continue_run(
        &self,
        run_id: RunId,
        prompt: String,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        if prompt.trim().is_empty() {
            return Ok(vec![RunnerEvent::Failed {
                run_id,
                error: "prompt_required".to_owned(),
            }]);
        }
        let mut run = match self.take_run(run_id) {
            Ok(run) => run,
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => {
                return Ok(vec![RunnerEvent::Failed {
                    run_id,
                    error: "run_not_found".to_owned(),
                }]);
            }
            Err(error) => return Err(error),
        };
        if !run.pending_tools.is_empty() {
            let error = "run_busy".to_owned();
            let events = vec![RunnerEvent::Failed {
                run_id,
                error: error.clone(),
            }];
            self.store_unless_terminal(run, &[])?;
            return Ok(events);
        }
        run.steps = 0;
        run.last_text.clear();
        run.inbox
            .insert(InboxTarget::NextTurn, InboxMessage::user(prompt));
        for message in run.inbox.claim(InboxTarget::NextTurn) {
            run.messages.push(ModelMessage::user(message.text));
        }
        let events = self.model_step(&mut run).await;
        self.store_unless_terminal(run, &events)?;
        Ok(events)
    }

    fn cancel(&self, run_id: RunId, reason: String) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        match self.take_run(run_id) {
            Ok(_) => Ok(vec![RunnerEvent::Failed {
                run_id,
                error: format!("cancelled:{reason}"),
            }]),
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => {
                Ok(vec![RunnerEvent::Failed {
                    run_id,
                    error: "run_not_found".to_owned(),
                }])
            }
            Err(error) => Err(error),
        }
    }

    async fn model_step(&self, run: &mut ActiveRun) -> Vec<RunnerEvent> {
        if run.steps >= self.max_steps_per_turn {
            return vec![RunnerEvent::Failed {
                run_id: run.run_id,
                error: "max_steps_per_turn".to_owned(),
            }];
        }
        run.steps += 1;

        for message in run.inbox.claim(InboxTarget::NextStep) {
            run.messages.push(ModelMessage::user(message.text));
        }

        let compact = compact_if_needed(
            std::mem::take(&mut run.messages),
            self.compact_trigger_tokens,
            self.compact_user_message_max_tokens,
        );
        run.messages = compact.messages;

        let mut events = Vec::new();
        if compact.applied {
            events.push(RunnerEvent::Compacted {
                run_id: run.run_id,
                tokens_before: compact.tokens_before as u64,
                tokens_after: compact.tokens_after as u64,
                summary_present: compact.summary_present,
            });
        }

        let output = match self
            .model
            .complete(ModelRequest {
                messages: run.messages.clone(),
                tools: tool_schemas(),
                sandbox: run.sandbox.clone(),
            })
            .await
        {
            Ok(output) => output,
            Err(error) => {
                events.push(RunnerEvent::Failed {
                    run_id: run.run_id,
                    error,
                });
                return events;
            }
        };
        if !output.text.is_empty() {
            run.last_text = output.text.clone();
            events.push(RunnerEvent::Delta {
                run_id: run.run_id,
                text: output.text.clone(),
            });
        }
        if !output.text.is_empty() || !output.tool_calls.is_empty() {
            run.messages.push(ModelMessage::assistant_with_tools(
                output.text.clone(),
                output.tool_calls.clone(),
            ));
        }

        if output.tool_calls.is_empty() {
            // DeepSeek: a text-only step ends the turn only when next-step is empty.
            if run.inbox.next_step.is_empty() {
                events.push(RunnerEvent::Completed {
                    run_id: run.run_id,
                    output: json!({
                        "schema": HARNESS_RESULT_SCHEMA,
                        "source": HARNESS_ID,
                        "text": run.last_text,
                        "steps": run.steps,
                        "sandbox": run.sandbox,
                    }),
                });
                return events;
            }
            events.extend(Box::pin(self.model_step(run)).await);
            return events;
        }

        run.pending_tools.clear();
        for call in output.tool_calls {
            match capability_for_tool(&call, &run.sandbox, &run.project_root) {
                Ok(request) => run.pending_tools.push_back((request.request_id, call)),
                Err(error) => {
                    return vec![RunnerEvent::Failed {
                        run_id: run.run_id,
                        error,
                    }];
                }
            }
        }

        match run.pending_tools.front().cloned() {
            Some((_, call)) => match emit_tool_request(run, &call) {
                Ok(event) => {
                    events.push(event);
                    events
                }
                Err(error) => vec![RunnerEvent::Failed {
                    run_id: run.run_id,
                    error,
                }],
            },
            None => vec![RunnerEvent::Failed {
                run_id: run.run_id,
                error: "tool_queue_empty".to_owned(),
            }],
        }
    }

    fn take_run(&self, run_id: RunId) -> Result<ActiveRun, KianaHarnessError> {
        self.runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .remove(&run_id)
            .ok_or_else(|| KianaHarnessError::Failed("run_not_found".to_owned()))
    }

    fn has_run(&self, run_id: RunId) -> Result<bool, KianaHarnessError> {
        Ok(self
            .runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .contains_key(&run_id))
    }

    fn store_unless_terminal(
        &self,
        run: ActiveRun,
        events: &[RunnerEvent],
    ) -> Result<(), KianaHarnessError> {
        if events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. }))
        {
            return Ok(());
        }
        self.runs
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .insert(run.run_id, run);
        Ok(())
    }
}

impl From<KianaHarnessError> for PortError {
    fn from(error: KianaHarnessError) -> Self {
        match error {
            KianaHarnessError::Unavailable(message) => PortError::Unavailable(message),
            KianaHarnessError::Failed(message) => PortError::Failed(message),
        }
    }
}

#[async_trait]
impl RunnerPort for KianaHarness {
    async fn send(&self, command: RunnerCommand) -> Result<Vec<RunnerEvent>, PortError> {
        self.dispatch(command).await.map_err(PortError::from)
    }
}

fn emit_tool_request(run: &mut ActiveRun, call: &ModelToolCall) -> Result<RunnerEvent, String> {
    let request = capability_for_tool(call, &run.sandbox, &run.project_root)?;
    if let Some((pending_id, _)) = run.pending_tools.front_mut() {
        *pending_id = request.request_id;
    }
    Ok(RunnerEvent::CapabilityRequested {
        run_id: run.run_id,
        request,
    })
}

fn normalize_sandbox(sandbox: &str) -> Result<&str, KianaHarnessError> {
    match sandbox.trim() {
        "" | DEFAULT_HARNESS_SANDBOX => Ok(DEFAULT_HARNESS_SANDBOX),
        HARNESS_SANDBOX_WORKSPACE_WRITE => Ok(HARNESS_SANDBOX_WORKSPACE_WRITE),
        other => Err(KianaHarnessError::Failed(format!(
            "sandbox_unsupported:{other}"
        ))),
    }
}

fn tool_result_text(result: &CapabilityResult) -> String {
    if let Some(text) = result.output.as_str() {
        return text.to_owned();
    }
    serde_json::to_string(&result.output).unwrap_or_else(|_| "{}".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ModelOutput;
    use kiana_domain::CapabilityKind;
    use kiana_runner_protocol::RunnerCommand;
    use serde_json::Value;

    fn scripted(outputs: Value) -> KianaHarness {
        KianaHarness::new(Arc::new(ScriptedModel::from_json(&outputs).unwrap()))
    }

    #[derive(Default)]
    struct CapturingModel {
        seen: Mutex<Vec<Vec<ModelMessage>>>,
    }

    #[async_trait]
    impl ModelClient for CapturingModel {
        async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
            self.seen.lock().unwrap().push(request.messages);
            Ok(ModelOutput::text("compacted"))
        }
    }

    #[tokio::test]
    async fn start_instructions_become_the_first_system_message() {
        let model = Arc::new(CapturingModel::default());
        let harness = KianaHarness::new(model.clone());
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start_in_with_instructions(
                run_id,
                "map it",
                "/repo",
                DEFAULT_HARNESS_SANDBOX,
                "Available skills:\n\n## code03-harness-demo\n",
                true,
            ))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        let first = seen.first().expect("model saw a request");
        assert_eq!(
            first.first().map(|message| message.role.clone()),
            Some(crate::model::ModelRole::System)
        );
        assert!(
            first
                .first()
                .is_some_and(|message| message.text.contains("code03-harness-demo")),
            "{first:?}"
        );
    }

    #[tokio::test]
    async fn text_only_turn_completes_without_a_tool_request() {
        let harness = scripted(json!([{"text": "architecture mapped"}]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();
        assert_eq!(events.first(), Some(&RunnerEvent::Started { run_id }));
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "architecture mapped")));
        let RunnerEvent::Completed { output, .. } = events.last().unwrap() else {
            panic!("expected completed");
        };
        assert_eq!(output["schema"], HARNESS_RESULT_SCHEMA);
        assert_eq!(output["source"], HARNESS_ID);
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
    }

    #[tokio::test]
    async fn over_budget_history_is_compacted_before_the_model_step() {
        let model = Arc::new(CapturingModel::default());
        let harness = KianaHarness::new(model.clone()).with_compact_budget(8, 4);
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "x".repeat(80)))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        let first = seen.first().expect("model saw a request");
        assert!(
            first
                .iter()
                .any(|message| crate::compact::is_summary_message(&message.text)),
            "{first:?}"
        );
        assert!(
            first
                .iter()
                .all(|message| message.role == crate::model::ModelRole::User),
            "{first:?}"
        );
        assert!(
            events.iter().any(|event| matches!(
                event,
                RunnerEvent::Compacted {
                    summary_present: true,
                    ..
                }
            )),
            "{events:?}"
        );
    }

    #[tokio::test]
    async fn shell_call_pauses_for_brokered_capability_result() {
        let harness = scripted(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "architecture mapped"}
        ]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Started { .. })));
        let request = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        assert_eq!(request.capability, CapabilityKind::Process);
        assert_eq!(request.operation, "shell.exec");
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));

        let completed = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request.request_id, json!({"stdout": "listed"})),
            })
            .await
            .unwrap();
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "architecture mapped")));
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn serial_tools_in_one_model_step_request_one_capability_at_a_time() {
        let harness = scripted(json!([
            {"text": "two tools", "tool_calls": [
                {"id": "c1", "name": "shell", "arguments": {"command": "ls"}},
                {"id": "c2", "name": "shell", "arguments": {"command": "pwd"}}
            ]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();
        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let first_request = first
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            first
                .iter()
                .filter(|event| matches!(event, RunnerEvent::CapabilityRequested { .. }))
                .count(),
            1
        );

        let second = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(first_request.request_id, json!({"ok": 1})),
            })
            .await
            .unwrap();
        let second_request = second
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .unwrap();
        assert_ne!(first_request.request_id, second_request.request_id);
        assert!(!second
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));

        let done = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(second_request.request_id, json!({"ok": 2})),
            })
            .await
            .unwrap();
        assert!(done
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn project_root_is_copied_into_shell_capability() {
        let harness = scripted(json!([
            {"text": "running ls", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]}
        ]));
        let events = harness
            .send(RunnerCommand::start_in(
                RunId::new(),
                "map it",
                "/tmp/kiana-project",
                "read-only",
            ))
            .await
            .unwrap();
        let request = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        assert_eq!(request.arguments["project_root"], "/tmp/kiana-project");
        assert_eq!(request.operation, "shell.exec");
    }

    #[tokio::test]
    async fn missing_model_fails_closed() {
        let harness = KianaHarness::new(Arc::new(UnavailableModel::default()));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "hello"))
            .await
            .unwrap();
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "model_unavailable".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn next_step_inject_is_claimed_on_the_following_model_step() {
        #[derive(Debug)]
        struct CaptureModel {
            outputs: std::sync::Mutex<VecDeque<ModelOutput>>,
            seen: std::sync::Mutex<Vec<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl ModelClient for CaptureModel {
            async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
                let texts = request
                    .messages
                    .iter()
                    .map(|message| message.text.clone())
                    .collect();
                self.seen
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .push(texts);
                self.outputs
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .pop_front()
                    .ok_or_else(|| "harness_script_exhausted".to_owned())
            }
        }

        let model = Arc::new(CaptureModel {
            outputs: std::sync::Mutex::new(VecDeque::from([
                ModelOutput {
                    text: "running ls".to_owned(),
                    tool_calls: vec![ModelToolCall {
                        id: "c1".to_owned(),
                        name: "shell".to_owned(),
                        arguments: json!({ "command": "ls" }),
                    }],
                },
                ModelOutput::text("steered"),
            ])),
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let harness = KianaHarness::new(model.clone());
        let run_id = RunId::new();
        let started = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();
        let request = started
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        harness
            .inject(run_id, "steer: stay in the workspace")
            .unwrap();
        let completed = harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request.request_id, json!({"stdout": "listed"})),
            })
            .await
            .unwrap();
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "steered")));
        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            seen[1]
                .iter()
                .any(|text| text == "steer: stay in the workspace"),
            "next-step inject must be claimed before the follow-up model call: {seen:?}"
        );
    }

    #[tokio::test]
    async fn shell_timeout_ms_is_copied_into_the_capability_request() {
        let harness = scripted(json!([{
            "text": "sleeping",
            "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "sleep 8", "timeout_ms": 250}}]
        }]));
        let events = harness
            .send(RunnerCommand::start(RunId::new(), "wait"))
            .await
            .unwrap();
        let request = events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.clone()),
                _ => None,
            })
            .expect("tool request");
        assert_eq!(request.operation, "shell.exec");
        assert_eq!(request.arguments["timeout_ms"], 250);
        assert_eq!(request.arguments["command"], "sleep 8");
    }

    #[tokio::test]
    async fn danger_full_access_is_rejected() {
        let harness = scripted(json!([{"text": "nope"}]));
        let error = harness
            .send(RunnerCommand::start_in(
                RunId::new(),
                "hello",
                "/repo",
                "danger-full-access",
            ))
            .await
            .unwrap_err();
        match error {
            KianaHarnessError::Failed(message) => {
                assert_eq!(message, "sandbox_unsupported:danger-full-access");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn continue_appends_to_the_same_run_messages() {
        #[derive(Debug)]
        struct CaptureModel {
            outputs: std::sync::Mutex<VecDeque<ModelOutput>>,
            seen: std::sync::Mutex<Vec<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl ModelClient for CaptureModel {
            async fn complete(&self, request: ModelRequest) -> Result<ModelOutput, String> {
                let texts = request
                    .messages
                    .iter()
                    .map(|message| message.text.clone())
                    .collect();
                self.seen
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .push(texts);
                self.outputs
                    .lock()
                    .map_err(|_| "capture_lock_poisoned".to_owned())?
                    .pop_front()
                    .ok_or_else(|| "harness_script_exhausted".to_owned())
            }
        }

        let model = Arc::new(CaptureModel {
            outputs: std::sync::Mutex::new(VecDeque::from([
                ModelOutput::text("first turn"),
                ModelOutput::text("continued"),
            ])),
            seen: std::sync::Mutex::new(Vec::new()),
        });
        let harness = KianaHarness::new(model.clone());
        let run_id = RunId::new();
        let started = harness
            .send(RunnerCommand::start(run_id, "hello"))
            .await
            .unwrap();
        assert!(started
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let continued = harness
            .send(RunnerCommand::continue_run(run_id, "keep going"))
            .await
            .unwrap();
        assert!(continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Delta { text, .. } if text == "continued")));
        assert!(continued
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        let seen = model.seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(seen[0].iter().any(|text| text == "hello"));
        assert!(seen[1].iter().any(|text| text == "hello"));
        assert!(seen[1].iter().any(|text| text == "keep going"));
    }

    #[tokio::test]
    async fn continue_unknown_run_fails_without_starting() {
        let harness = scripted(json!([{"text": "should not run"}]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::continue_run(run_id, "keep going"))
            .await
            .unwrap();
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn cancel_unknown_run_fails_closed() {
        let harness = scripted(json!([{"text": "should not run"}]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::Cancel {
                run_id,
                reason: "user".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            })
        );
    }
}
