//! Model-loop runtime constrained by the Kiana runner protocol.
//!
//! The production harness is owned by this crate. Loop/inbox semantics come
//! from DeepSeek Harness; Codex-compatible `shell` / `apply_patch` names are
//! the model-visible tools. Execution stays on the control-plane broker.
//! `KianaHarness` implements `RunnerPort` directly; daemon does not wrap a
//! second adapter loop.

mod compact;
mod harness;
mod inbox;
mod model;
mod protocol_runner;
mod tools;

pub use harness::{KianaHarness, KianaHarnessError, HARNESS_ID, HARNESS_RESULT_SCHEMA};
pub use inbox::{Inbox, InboxMessage, InboxTarget};
pub use model::{
    ModelClient, ModelMessage, ModelOutput, ModelRequest, ModelRole, ModelToolCall, ScriptedModel,
    UnavailableModel,
};
pub use protocol_runner::ProtocolRunner;
