//! Commands and events exchanged across the runner boundary.

use kiana_domain::{CapabilityRequest, CapabilityResult, RunId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_HARNESS_SANDBOX: &str = "read-only";
pub const HARNESS_SANDBOX_WORKSPACE_WRITE: &str = "workspace-write";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum RunnerCommand {
    Start {
        run_id: RunId,
        prompt: String,
        #[serde(default)]
        project_root: String,
        #[serde(default = "default_harness_sandbox")]
        sandbox: String,
    },
    CapabilityResult {
        run_id: RunId,
        result: CapabilityResult,
    },
    Continue {
        run_id: RunId,
        prompt: String,
    },
    Cancel {
        run_id: RunId,
        reason: String,
    },
}

fn default_harness_sandbox() -> String {
    DEFAULT_HARNESS_SANDBOX.to_owned()
}

impl RunnerCommand {
    pub fn start(run_id: RunId, prompt: impl Into<String>) -> Self {
        Self::Start {
            run_id,
            prompt: prompt.into(),
            project_root: String::new(),
            sandbox: default_harness_sandbox(),
        }
    }

    pub fn start_in(
        run_id: RunId,
        prompt: impl Into<String>,
        project_root: impl Into<String>,
        sandbox: impl Into<String>,
    ) -> Self {
        Self::Start {
            run_id,
            prompt: prompt.into(),
            project_root: project_root.into(),
            sandbox: sandbox.into(),
        }
    }

    pub fn continue_run(run_id: RunId, prompt: impl Into<String>) -> Self {
        Self::Continue {
            run_id,
            prompt: prompt.into(),
        }
    }

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
pub enum RunnerEvent {
    Started {
        run_id: RunId,
    },
    Delta {
        run_id: RunId,
        text: String,
    },
    CapabilityRequested {
        run_id: RunId,
        request: CapabilityRequest,
    },
    Completed {
        run_id: RunId,
        output: Value,
    },
    Failed {
        run_id: RunId,
        error: String,
    },
}

impl RunnerEvent {
    pub const fn run_id(&self) -> RunId {
        match self {
            Self::Started { run_id }
            | Self::Delta { run_id, .. }
            | Self::CapabilityRequested { run_id, .. }
            | Self::Completed { run_id, .. }
            | Self::Failed { run_id, .. } => *run_id,
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
}
