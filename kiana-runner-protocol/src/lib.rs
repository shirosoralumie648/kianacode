//! Commands and events exchanged across the runner boundary.

use kiana_domain::{CapabilityRequest, CapabilityResult, RunId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
pub enum RunnerCommand {
    Start {
        run_id: RunId,
        prompt: String,
    },
    CapabilityResult {
        run_id: RunId,
        result: CapabilityResult,
    },
    Cancel {
        run_id: RunId,
        reason: String,
    },
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
        let command = RunnerCommand::Start {
            run_id: RunId::new(),
            prompt: "inspect architecture".to_owned(),
        };
        let encoded = serde_json::to_vec(&command).unwrap();
        assert_eq!(
            serde_json::from_slice::<RunnerCommand>(&encoded).unwrap(),
            command
        );
    }
}
