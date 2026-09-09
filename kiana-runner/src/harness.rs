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
    ModelClient, ModelDelta, ModelMessage, ModelOutput, ModelRequest, ModelToolCall, ScriptedModel,
    UnavailableModel,
};
use crate::tools::{capability_for_tool, tool_schemas};
use async_trait::async_trait;
use kiana_domain::{redact_text, CapabilityResult, RunId, StreamingRedactor};
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
    pub repeated_tool_call_threshold: u32,
    pub compact_trigger_tokens: usize,
    pub compact_user_message_max_tokens: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_steps_per_turn: 32,
            repeated_tool_call_threshold: 3,
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

struct EventEmitter<'a> {
    events: Vec<RunnerEvent>,
    sink: Option<&'a mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send)>,
}

impl<'a> EventEmitter<'a> {
    fn new(sink: Option<&'a mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send)>) -> Self {
        Self {
            events: Vec::new(),
            sink,
        }
    }

    fn emit(&mut self, event: RunnerEvent) -> Result<(), String> {
        if let Some(sink) = self.sink.as_mut() {
            sink(event.clone()).map_err(|error| format!("runner_event_sink_failed:{error}"))?;
        }
        self.events.push(event);
        Ok(())
    }

    fn emit_event(&mut self, event: RunnerEvent) -> Result<(), KianaHarnessError> {
        self.emit(event).map_err(KianaHarnessError::Failed)
    }

    fn replace_since(&mut self, checkpoint: usize, event: RunnerEvent) -> Result<(), String> {
        if self.sink.is_none() {
            self.events.truncate(checkpoint);
        }
        self.emit(event)
    }

    fn replace_event_since(
        &mut self,
        checkpoint: usize,
        event: RunnerEvent,
    ) -> Result<(), KianaHarnessError> {
        self.replace_since(checkpoint, event)
            .map_err(KianaHarnessError::Failed)
    }

    fn into_events(self) -> Vec<RunnerEvent> {
        self.events
    }
}

struct ActiveRun {
    run_id: RunId,
    sandbox: String,
    project_root: String,
    inbox: Inbox,
    messages: Vec<ModelMessage>,
    pending_tools: VecDeque<(kiana_domain::RequestId, ModelToolCall)>,
    last_tool_call: Option<RepeatedToolCall>,
    steps: u32,
    last_text: String,
    cancellation: Arc<RunCancellation>,
}

struct RepeatedToolCall {
    name: String,
    canonical_arguments: String,
    count: u32,
}

#[derive(Default)]
struct RunCancellation {
    error: Mutex<Option<String>>,
}

impl RunCancellation {
    fn request(&self, error: String) -> Result<bool, KianaHarnessError> {
        let mut state = self
            .error
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        if state.is_some() {
            return Ok(false);
        }
        *state = Some(error);
        Ok(true)
    }

    fn error(&self) -> Result<Option<String>, KianaHarnessError> {
        self.error
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))
            .map(|state| state.clone())
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<String>>, KianaHarnessError> {
        self.error
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))
    }
}

struct StartInput {
    run_id: RunId,
    prompt: String,
    history: Vec<kiana_domain::ConversationMessage>,
    sandbox: String,
    project_root: String,
    instructions: String,
}

pub struct KianaHarness {
    model: Arc<dyn ModelClient>,
    runs: Mutex<HashMap<RunId, ActiveRun>>,
    in_flight: Mutex<HashMap<RunId, Arc<RunCancellation>>>,
    compact_trigger_tokens: usize,
    compact_user_message_max_tokens: usize,
    max_steps_per_turn: u32,
    repeated_tool_call_threshold: u32,
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
            in_flight: Mutex::new(HashMap::new()),
            compact_trigger_tokens: config.compact_trigger_tokens,
            compact_user_message_max_tokens: config.compact_user_message_max_tokens,
            max_steps_per_turn: config.max_steps_per_turn.max(1),
            repeated_tool_call_threshold: config.repeated_tool_call_threshold.max(1),
        }
    }

    pub fn config(&self) -> RuntimeConfig {
        RuntimeConfig {
            max_steps_per_turn: self.max_steps_per_turn,
            repeated_tool_call_threshold: self.repeated_tool_call_threshold,
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

    pub fn with_repeated_tool_call_threshold(mut self, threshold: u32) -> Self {
        self.repeated_tool_call_threshold = threshold.max(1);
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
        self.dispatch(command, None).await
    }

    /// 发送命令，并在每个协议事件产生时同步交付给 `sink`。
    ///
    /// 成功返回时，`sink` 收到的序列与返回值逐项一致。`sink` 返回错误表示下游取消或背压，
    /// harness 会立即停止并返回 `runner_event_sink_failed:*`，不会继续产生后续事件。
    pub async fn send_with_events(
        &self,
        command: RunnerCommand,
        sink: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        self.dispatch(command, Some(sink)).await
    }

    async fn dispatch(
        &self,
        command: RunnerCommand,
        sink: Option<&mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send)>,
    ) -> Result<Vec<RunnerEvent>, KianaHarnessError> {
        let mut emitter = EventEmitter::new(sink);
        match command {
            RunnerCommand::Start {
                run_id,
                prompt,
                history,
                project_root,
                sandbox,
                instructions,
                project_trusted: _,
            } => {
                self.start(
                    StartInput {
                        run_id,
                        prompt,
                        history,
                        sandbox,
                        project_root,
                        instructions,
                    },
                    &mut emitter,
                )
                .await
            }
            RunnerCommand::CapabilityResult { run_id, result } => {
                self.on_capability_result(run_id, result, &mut emitter)
                    .await
            }
            RunnerCommand::Continue { run_id, prompt } => {
                self.continue_run(run_id, prompt, &mut emitter).await
            }
            RunnerCommand::Cancel { run_id, reason } => self.cancel(run_id, reason, &mut emitter),
        }?;
        Ok(emitter.into_events())
    }

    async fn start(
        &self,
        input: StartInput,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let StartInput {
            run_id,
            prompt,
            history,
            sandbox,
            project_root,
            instructions,
        } = input;
        if self.has_run(run_id)? || self.has_in_flight(run_id)? {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "run_already_exists".to_owned(),
            });
        }
        let sandbox = normalize_sandbox(&sandbox)?;
        let cancellation = Arc::new(RunCancellation::default());
        let mut run = ActiveRun {
            run_id,
            sandbox: sandbox.to_owned(),
            project_root,
            inbox: Inbox::default(),
            messages: Vec::new(),
            pending_tools: VecDeque::new(),
            last_tool_call: None,
            steps: 0,
            last_text: String::new(),
            cancellation: cancellation.clone(),
        };
        if !instructions.trim().is_empty() {
            run.messages.push(ModelMessage::system(instructions));
        }
        run.messages
            .extend(history.into_iter().map(|message| match message.role {
                kiana_domain::ConversationRole::User => ModelMessage::user(message.text),
                kiana_domain::ConversationRole::Assistant => ModelMessage::assistant(message.text),
                kiana_domain::ConversationRole::Tool => ModelMessage::tool(
                    message.tool_call_id.unwrap_or_else(|| "history".to_owned()),
                    message.text,
                ),
            }));
        run.inbox
            .insert(InboxTarget::NextTurn, InboxMessage::user(prompt));
        for message in run.inbox.claim(InboxTarget::NextTurn) {
            run.messages.push(ModelMessage::user(message.text));
        }

        self.register_in_flight(run_id, cancellation)?;
        let result = match emitter.emit_event(RunnerEvent::Started { run_id }) {
            Ok(()) => self.model_step(&mut run, emitter).await,
            Err(error) => Err(error),
        };
        self.unregister_in_flight(run_id)?;
        result?;
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    async fn on_capability_result(
        &self,
        run_id: RunId,
        result: CapabilityResult,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let mut run = self.take_run(run_id)?;
        let Some((expected_id, call)) = run.pending_tools.pop_front() else {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "unexpected_capability_result".to_owned(),
            });
        };
        if expected_id != result.request_id {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "capability_result_mismatch".to_owned(),
            });
        }

        run.messages
            .push(ModelMessage::tool(call.id, tool_result_text(&result)));

        if let Some((request_id, next_call)) = run.pending_tools.front().cloned() {
            let _ = request_id;
            let cancellation = run.cancellation.clone();
            let state = cancellation.lock()?;
            if let Some(error) = state.as_ref() {
                let error = error.clone();
                drop(state);
                emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                return Ok(());
            }
            match self.emit_tool_request(&mut run, &next_call) {
                Ok(event) => emitter.emit_event(event)?,
                Err(error) => emitter.emit_event(RunnerEvent::Failed { run_id, error })?,
            }
        } else {
            self.model_step_in_flight(&mut run, emitter).await?;
        }
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    async fn continue_run(
        &self,
        run_id: RunId,
        prompt: String,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        if prompt.trim().is_empty() {
            return emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: "prompt_required".to_owned(),
            });
        }
        let mut run = match self.take_run(run_id) {
            Ok(run) => run,
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => {
                return emitter.emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "run_not_found".to_owned(),
                });
            }
            Err(error) => return Err(error),
        };
        if !run.pending_tools.is_empty() {
            let error = "run_busy".to_owned();
            emitter.emit_event(RunnerEvent::Failed {
                run_id,
                error: error.clone(),
            })?;
            self.store_unless_terminal(run, &[])?;
            return Ok(());
        }
        run.steps = 0;
        run.last_text.clear();
        run.last_tool_call = None;
        run.inbox
            .insert(InboxTarget::NextTurn, InboxMessage::user(prompt));
        for message in run.inbox.claim(InboxTarget::NextTurn) {
            run.messages.push(ModelMessage::user(message.text));
        }
        self.model_step_in_flight(&mut run, emitter).await?;
        self.store_unless_terminal(run, &emitter.events)?;
        Ok(())
    }

    fn cancel(
        &self,
        run_id: RunId,
        reason: String,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        let error = format!("cancelled:{reason}");
        let in_flight = self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .get(&run_id)
            .cloned();
        if let Some(cancellation) = in_flight {
            cancellation.request(error.clone())?;
            return emitter.emit_event(RunnerEvent::Failed { run_id, error });
        }
        match self.take_run(run_id) {
            Ok(_) => emitter.emit_event(RunnerEvent::Failed { run_id, error }),
            Err(KianaHarnessError::Failed(error)) if error == "run_not_found" => emitter
                .emit_event(RunnerEvent::Failed {
                    run_id,
                    error: "run_not_found".to_owned(),
                }),
            Err(error) => Err(error),
        }
    }

    async fn model_step(
        &self,
        run: &mut ActiveRun,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        if let Some(error) = run.cancellation.error()? {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error,
            })?;
            return Ok(());
        }
        let checkpoint = emitter.events.len();
        if run.steps >= self.max_steps_per_turn {
            emitter.emit_event(RunnerEvent::Failed {
                run_id: run.run_id,
                error: "max_steps_per_turn".to_owned(),
            })?;
            return Ok(());
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

        if compact.applied {
            emitter.emit_event(RunnerEvent::Compacted {
                run_id: run.run_id,
                tokens_before: compact.tokens_before as u64,
                tokens_after: compact.tokens_after as u64,
                summary_present: compact.summary_present,
            })?;
        }

        let run_id = run.run_id;
        let cancellation = run.cancellation.clone();
        let mut emitted_delta = false;
        let mut sink_error: Option<String> = None;
        let mut delta_redactor = StreamingRedactor::new();
        let output = self
            .model
            .complete_streaming(
                ModelRequest {
                    messages: run.messages.clone(),
                    tools: tool_schemas(),
                    sandbox: run.sandbox.clone(),
                },
                &mut |delta| {
                    if let Some(error) = sink_error.as_ref() {
                        return Err(error.clone());
                    }
                    match delta {
                        ModelDelta::Text { text } => {
                            let state = cancellation.lock().map_err(|error| error.to_string())?;
                            if let Some(error) = state.as_ref() {
                                sink_error = Some(error.clone());
                                return Err(error.clone());
                            }
                            let text = delta_redactor.push(&text);
                            if text.is_empty() {
                                return Ok(());
                            }
                            emitted_delta = true;
                            if let Err(error) = emitter.emit(RunnerEvent::Delta { run_id, text }) {
                                sink_error = Some(error.clone());
                                return Err(error);
                            }
                        }
                    }
                    Ok(())
                },
            )
            .await;
        if let Some(error) = cancellation.error()? {
            emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
            return Ok(());
        }
        if let Some(error) = sink_error {
            return Err(KianaHarnessError::Failed(error));
        }
        let tail = delta_redactor.finish();
        if !tail.is_empty() {
            emitted_delta = true;
            let state = cancellation.lock()?;
            if let Some(error) = state.as_ref() {
                let error = error.clone();
                drop(state);
                emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                return Ok(());
            }
            emitter.emit_event(RunnerEvent::Delta { run_id, text: tail })?;
        }
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                return Ok(());
            }
        };
        if !output.text.is_empty() {
            let redacted_text = redact_text(&output.text);
            run.last_text = redacted_text.clone();
            if !emitted_delta {
                emitter.emit_event(RunnerEvent::Delta {
                    run_id,
                    text: redacted_text,
                })?;
            }
        }
        if !output.text.is_empty() || !output.tool_calls.is_empty() {
            run.messages.push(ModelMessage::assistant_with_tools(
                redact_text(&output.text),
                output.tool_calls.clone(),
            ));
        }

        if output.tool_calls.is_empty() {
            // DeepSeek: a text-only step ends the turn only when next-step is empty.
            if run.inbox.next_step.is_empty() {
                let ModelOutput {
                    usage,
                    stop_reason,
                    model_id,
                    ..
                } = output;
                let mut completed_output = json!({
                    "schema": HARNESS_RESULT_SCHEMA,
                    "source": HARNESS_ID,
                    "text": run.last_text,
                    "steps": run.steps,
                    "sandbox": run.sandbox,
                });
                let completed = completed_output
                    .as_object_mut()
                    .expect("harness result is a JSON object");
                if let Some(usage) = usage {
                    completed.insert("usage".to_owned(), json!(usage));
                }
                if let Some(stop_reason) = stop_reason {
                    completed.insert("stop_reason".to_owned(), json!(stop_reason));
                }
                if let Some(model_id) = model_id {
                    completed.insert("model_id".to_owned(), json!(model_id));
                }
                let state = cancellation.lock()?;
                if let Some(error) = state.as_ref() {
                    let error = error.clone();
                    drop(state);
                    emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
                    return Ok(());
                }
                emitter.emit_event(RunnerEvent::Completed {
                    run_id: run.run_id,
                    output: completed_output,
                })?;
                return Ok(());
            }
            Box::pin(self.model_step(run, emitter)).await?;
            return Ok(());
        }

        let state = cancellation.lock()?;
        if let Some(error) = state.as_ref() {
            let error = error.clone();
            drop(state);
            emitter.emit_event(RunnerEvent::Failed { run_id, error })?;
            return Ok(());
        }
        run.pending_tools.clear();
        for call in output.tool_calls {
            match capability_for_tool(&call, &run.sandbox, &run.project_root) {
                Ok(request) => run.pending_tools.push_back((request.request_id, call)),
                Err(error) => {
                    emitter.replace_event_since(
                        checkpoint,
                        RunnerEvent::Failed {
                            run_id: run.run_id,
                            error,
                        },
                    )?;
                    return Ok(());
                }
            }
        }

        match run.pending_tools.front().cloned() {
            Some((_, call)) => match self.emit_tool_request(run, &call) {
                Ok(event) => emitter.emit_event(event)?,
                Err(error) => emitter.replace_event_since(
                    checkpoint,
                    RunnerEvent::Failed {
                        run_id: run.run_id,
                        error,
                    },
                )?,
            },
            None => emitter.replace_event_since(
                checkpoint,
                RunnerEvent::Failed {
                    run_id: run.run_id,
                    error: "tool_queue_empty".to_owned(),
                },
            )?,
        }
        Ok(())
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

    fn has_in_flight(&self, run_id: RunId) -> Result<bool, KianaHarnessError> {
        Ok(self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .contains_key(&run_id))
    }

    fn register_in_flight(
        &self,
        run_id: RunId,
        cancellation: Arc<RunCancellation>,
    ) -> Result<(), KianaHarnessError> {
        let mut in_flight = self
            .in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?;
        if in_flight.contains_key(&run_id) {
            return Err(KianaHarnessError::Failed("run_already_exists".to_owned()));
        }
        in_flight.insert(run_id, cancellation);
        Ok(())
    }

    fn unregister_in_flight(&self, run_id: RunId) -> Result<(), KianaHarnessError> {
        self.in_flight
            .lock()
            .map_err(|_| KianaHarnessError::Failed("harness_lock_poisoned".to_owned()))?
            .remove(&run_id);
        Ok(())
    }

    async fn model_step_in_flight(
        &self,
        run: &mut ActiveRun,
        emitter: &mut EventEmitter<'_>,
    ) -> Result<(), KianaHarnessError> {
        self.register_in_flight(run.run_id, run.cancellation.clone())?;
        let result = self.model_step(run, emitter).await;
        self.unregister_in_flight(run.run_id)?;
        result
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

    fn emit_tool_request(
        &self,
        run: &mut ActiveRun,
        call: &ModelToolCall,
    ) -> Result<RunnerEvent, String> {
        if let Some(error) = self.record_tool_call(run, call) {
            return Err(error);
        }
        let request = capability_for_tool(call, &run.sandbox, &run.project_root)?;
        if let Some((pending_id, _)) = run.pending_tools.front_mut() {
            *pending_id = request.request_id;
        }
        Ok(RunnerEvent::CapabilityRequested {
            run_id: run.run_id,
            request,
        })
    }

    fn record_tool_call(&self, run: &mut ActiveRun, call: &ModelToolCall) -> Option<String> {
        let canonical_arguments = canonical_json(&call.arguments);
        let is_same_as_last = run.last_tool_call.as_ref().is_some_and(|last| {
            last.name == call.name && last.canonical_arguments == canonical_arguments
        });
        let count = if is_same_as_last {
            let last = run
                .last_tool_call
                .as_mut()
                .expect("the previous tool call was just observed");
            last.count = last.count.saturating_add(1);
            last.count
        } else {
            run.last_tool_call = Some(RepeatedToolCall {
                name: call.name.clone(),
                canonical_arguments,
                count: 1,
            });
            1
        };
        (count >= self.repeated_tool_call_threshold)
            .then(|| format!("repeated_tool_call:{}", call.name))
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
        self.dispatch(command, None).await.map_err(PortError::from)
    }

    async fn send_with_events(
        &self,
        command: RunnerCommand,
        on_event: &mut (dyn FnMut(RunnerEvent) -> Result<(), String> + Send),
    ) -> Result<Vec<RunnerEvent>, PortError> {
        self.dispatch(command, Some(on_event))
            .await
            .map_err(PortError::from)
    }
}

fn canonical_json(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(object) => {
            let mut entries = object.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(left, _)| *left);
            let mut canonical = String::from("{");
            for (index, (key, value)) in entries.into_iter().enumerate() {
                if index > 0 {
                    canonical.push(',');
                }
                canonical.push_str(
                    &serde_json::to_string(key).expect("JSON object key serialization cannot fail"),
                );
                canonical.push(':');
                canonical.push_str(&canonical_json(value));
            }
            canonical.push('}');
            canonical
        }
        serde_json::Value::Array(values) => {
            let mut canonical = String::from("[");
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    canonical.push(',');
                }
                canonical.push_str(&canonical_json(value));
            }
            canonical.push(']');
            canonical
        }
        _ => serde_json::to_string(value).expect("JSON scalar serialization cannot fail"),
    }
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
    use kiana_domain::CapabilityKind;
    use kiana_runner_protocol::RunnerCommand;
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn scripted(outputs: Value) -> KianaHarness {
        KianaHarness::new(Arc::new(ScriptedModel::from_json(&outputs).unwrap()))
    }

    fn capability_request_id(events: &[RunnerEvent]) -> kiana_domain::RequestId {
        events
            .iter()
            .find_map(|event| match event {
                RunnerEvent::CapabilityRequested { request, .. } => Some(request.request_id),
                _ => None,
            })
            .expect("expected a capability request")
    }

    async fn send_capability_success(
        harness: &KianaHarness,
        run_id: RunId,
        request_id: kiana_domain::RequestId,
    ) -> Vec<RunnerEvent> {
        harness
            .send(RunnerCommand::CapabilityResult {
                run_id,
                result: CapabilityResult::success(request_id, json!({"ok": true})),
            })
            .await
            .unwrap()
    }

    #[test]
    fn runtime_config_defaults_repeated_tool_call_threshold_to_three() {
        assert_eq!(RuntimeConfig::default().repeated_tool_call_threshold, 3);
    }

    #[tokio::test]
    async fn configured_repeated_tool_call_threshold_fails_at_that_count() {
        let harness = KianaHarness::with_config(
            Arc::new(
                ScriptedModel::from_json(&json!([
                    {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
                    {"text": "second", "tool_calls": [{"id": "c2", "name": "shell", "arguments": {"command": "ls"}}]},
                    {"text": "done"}
                ]))
                .unwrap(),
            ),
            RuntimeConfig {
                repeated_tool_call_threshold: 2,
                ..RuntimeConfig::default()
            },
        );
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;

        assert_eq!(
            second.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "repeated_tool_call:shell".to_owned(),
            })
        );
    }

    #[tokio::test]
    async fn third_consecutive_identical_tool_call_fails_closed() {
        let harness = scripted(json!([
            {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "second", "tool_calls": [{"id": "c2", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "third", "tool_calls": [{"id": "c3", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;

        assert_eq!(
            third.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "repeated_tool_call:shell".to_owned(),
            })
        );
        assert!(!third.iter().any(|event| matches!(
            event,
            RunnerEvent::CapabilityRequested { .. } | RunnerEvent::Completed { .. }
        )));
    }

    #[tokio::test]
    async fn consecutive_same_tool_with_different_arguments_is_not_blocked() {
        let harness = scripted(json!([
            {"text": "first", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "second", "tool_calls": [{"id": "c2", "name": "shell", "arguments": {"command": "ls src"}}]},
            {"text": "third", "tool_calls": [{"id": "c3", "name": "shell", "arguments": {"command": "ls tests"}}]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;
        let completed =
            send_capability_success(&harness, run_id, capability_request_id(&third)).await;

        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn alternating_different_tools_reset_repetition_count() {
        let harness = scripted(json!([
            {"text": "shell", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "patch", "tool_calls": [{"id": "c2", "name": "apply_patch", "arguments": {"patch": "one"}}]},
            {"text": "shell", "tool_calls": [{"id": "c3", "name": "shell", "arguments": {"command": "pwd"}}]},
            {"text": "patch", "tool_calls": [{"id": "c4", "name": "apply_patch", "arguments": {"patch": "two"}}]},
            {"text": "shell", "tool_calls": [{"id": "c5", "name": "shell", "arguments": {"command": "ls"}}]},
            {"text": "done"}
        ]));
        let run_id = RunId::new();
        let mut events = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();

        for _ in 0..4 {
            assert!(!events
                .iter()
                .any(|event| matches!(event, RunnerEvent::Failed { .. })));
            events =
                send_capability_success(&harness, run_id, capability_request_id(&events)).await;
        }
        let completed =
            send_capability_success(&harness, run_id, capability_request_id(&events)).await;

        assert!(completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!completed
            .iter()
            .any(|event| matches!(event, RunnerEvent::Failed { .. })));
    }

    #[tokio::test]
    async fn repeated_tool_call_detection_ignores_json_object_key_order() {
        let harness = scripted(json!([
            {
                "text": "first",
                "tool_calls": [{
                    "id": "c1",
                    "name": "mcp",
                    "arguments": {"server": "local", "tool": "read", "arguments": {"beta": 2, "alpha": 1}}
                }]
            },
            {
                "text": "second",
                "tool_calls": [{
                    "id": "c2",
                    "name": "mcp",
                    "arguments": {"arguments": {"alpha": 1, "beta": 2}, "tool": "read", "server": "local"}
                }]
            },
            {
                "text": "third",
                "tool_calls": [{
                    "id": "c3",
                    "name": "mcp",
                    "arguments": {"server": "local", "tool": "read", "arguments": {"beta": 2, "alpha": 1}}
                }]
            },
            {"text": "done"}
        ]));
        let run_id = RunId::new();

        let first = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();
        let second = send_capability_success(&harness, run_id, capability_request_id(&first)).await;
        let third = send_capability_success(&harness, run_id, capability_request_id(&second)).await;

        assert_eq!(
            third.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "repeated_tool_call:mcp".to_owned(),
            })
        );
    }

    #[derive(Debug)]
    struct ChunkedModel {
        chunks: Vec<&'static str>,
    }

    #[async_trait]
    impl ModelClient for ChunkedModel {
        async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
            Err("chunked_model_complete_must_not_be_called".to_owned())
        }

        async fn complete_streaming(
            &self,
            _request: ModelRequest,
            on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
        ) -> Result<ModelOutput, String> {
            let mut text = String::new();
            for chunk in &self.chunks {
                text.push_str(chunk);
                on_delta(ModelDelta::Text {
                    text: (*chunk).to_owned(),
                })?;
            }
            Ok(ModelOutput::text(text))
        }
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
    async fn sink_receives_each_streamed_delta_in_order() {
        let harness = KianaHarness::new(Arc::new(ChunkedModel {
            chunks: vec!["alpha", " beta", " gamma"],
        }));
        let run_id = RunId::new();
        let mut delivered = Vec::new();

        let events = harness
            .send_with_events(RunnerCommand::start(run_id, "stream it"), &mut |event| {
                delivered.push(event);
                Ok(())
            })
            .await
            .unwrap();

        assert_eq!(delivered, events);
        let deltas = events
            .iter()
            .filter_map(|event| match event {
                RunnerEvent::Delta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(deltas, vec!["alpha", " beta", " gamma"]);
        assert!(matches!(events.last(), Some(RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn sink_is_called_before_the_model_step_finishes() {
        #[derive(Debug)]
        struct ObservingModel {
            observed: Arc<AtomicBool>,
        }

        #[async_trait]
        impl ModelClient for ObservingModel {
            async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
                Err("observing_model_complete_must_not_be_called".to_owned())
            }

            async fn complete_streaming(
                &self,
                _request: ModelRequest,
                on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
            ) -> Result<ModelOutput, String> {
                on_delta(ModelDelta::Text {
                    text: "first".to_owned(),
                })?;
                assert!(
                    self.observed.load(Ordering::SeqCst),
                    "the sink must observe the delta before the model step continues"
                );
                on_delta(ModelDelta::Text {
                    text: " second".to_owned(),
                })?;
                Ok(ModelOutput::text("first second"))
            }
        }

        let observed = Arc::new(AtomicBool::new(false));
        let harness = KianaHarness::new(Arc::new(ObservingModel {
            observed: Arc::clone(&observed),
        }));
        let mut delivered = Vec::new();

        let events = harness
            .send_with_events(
                RunnerCommand::start(RunId::new(), "stream it"),
                &mut |event| {
                    if matches!(event, RunnerEvent::Delta { .. }) {
                        observed.store(true, Ordering::SeqCst);
                    }
                    delivered.push(event);
                    Ok(())
                },
            )
            .await
            .unwrap();

        assert!(observed.load(Ordering::SeqCst));
        assert_eq!(delivered, events);
    }

    #[tokio::test]
    async fn cancelling_an_in_flight_stream_rejects_late_deltas_and_completion() {
        #[derive(Default)]
        struct HoldingModel {
            entered: Arc<tokio::sync::Notify>,
            release: Arc<tokio::sync::Notify>,
            late_delta_emitted: Arc<AtomicBool>,
        }

        #[async_trait]
        impl ModelClient for HoldingModel {
            async fn complete(&self, _request: ModelRequest) -> Result<ModelOutput, String> {
                Err("holding_model_complete_must_not_be_called".to_owned())
            }

            async fn complete_streaming(
                &self,
                _request: ModelRequest,
                on_delta: &mut (dyn FnMut(ModelDelta) -> Result<(), String> + Send),
            ) -> Result<ModelOutput, String> {
                on_delta(ModelDelta::Text {
                    text: "before".to_owned(),
                })?;
                self.entered.notify_one();
                self.release.notified().await;
                on_delta(ModelDelta::Text {
                    text: "after".to_owned(),
                })?;
                self.late_delta_emitted.store(true, Ordering::SeqCst);
                Ok(ModelOutput::text("beforeafter"))
            }
        }

        let model = Arc::new(HoldingModel::default());
        let harness = Arc::new(KianaHarness::new(model.clone()));
        let run_id = RunId::new();
        let entered = model.entered.notified();
        let start_task = {
            let harness = harness.clone();
            tokio::spawn(async move { harness.send(RunnerCommand::start(run_id, "hold")).await })
        };
        entered.await;

        let cancel = harness
            .send(RunnerCommand::Cancel {
                run_id,
                reason: "user".to_owned(),
            })
            .await
            .unwrap();
        assert_eq!(
            cancel.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "cancelled:user".to_owned(),
            })
        );

        model.release.notify_one();
        let events = start_task.await.unwrap().unwrap();
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
        assert!(!events.iter().any(|event| matches!(
            event,
            RunnerEvent::Delta { text, .. } if text == "after"
        )));
        assert!(!model.late_delta_emitted.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn sink_error_stops_the_run_without_completed() {
        let harness = KianaHarness::new(Arc::new(ChunkedModel {
            chunks: vec!["first", "second"],
        }));
        let run_id = RunId::new();
        let mut delivered = Vec::new();

        let error = harness
            .send_with_events(RunnerCommand::start(run_id, "stream it"), &mut |event| {
                let is_delta = matches!(event, RunnerEvent::Delta { .. });
                delivered.push(event);
                if is_delta {
                    Err("backpressure".to_owned())
                } else {
                    Ok(())
                }
            })
            .await
            .unwrap_err();

        match error {
            KianaHarnessError::Failed(message) => {
                assert_eq!(message, "runner_event_sink_failed:backpressure");
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            delivered
                .iter()
                .filter(|event| matches!(event, RunnerEvent::Delta { .. }))
                .count(),
            1
        );
        assert!(!delivered
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }

    #[tokio::test]
    async fn completed_event_records_optional_model_metadata() {
        let harness = scripted(json!([{
            "text": "architecture mapped",
            "usage": {"input_tokens": 12, "output_tokens": 4},
            "stop_reason": "end_turn",
            "model_id": "test-model"
        }]));
        let run_id = RunId::new();
        let events = harness
            .send(RunnerCommand::start(run_id, "map it"))
            .await
            .unwrap();

        let RunnerEvent::Completed { output, .. } = events.last().unwrap() else {
            panic!("expected completed");
        };
        assert_eq!(output["usage"]["input_tokens"], 12);
        assert_eq!(output["usage"]["output_tokens"], 4);
        assert_eq!(output["stop_reason"], "end_turn");
        assert_eq!(output["model_id"], "test-model");
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
    async fn invalid_tool_arguments_fail_before_any_capability_request() {
        let harness = scripted(json!([
            {"text": "bad shell call", "tool_calls": [{"id": "c1", "name": "shell", "arguments": {"command": 42}}]}
        ]));
        let run_id = RunId::new();

        let events = harness
            .send(RunnerCommand::start(run_id, "go"))
            .await
            .unwrap();

        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "invalid_arguments:shell:command".to_owned(),
            })
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::CapabilityRequested { .. })));
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
                    ..ModelOutput::default()
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
