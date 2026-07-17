//! Model-loop runtime constrained by the Kiana runner protocol.

use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

#[derive(Clone, Copy, Debug, Default)]
pub struct ProtocolRunner;

impl ProtocolRunner {
    pub async fn send(&self, command: RunnerCommand) -> Vec<RunnerEvent> {
        match command {
            RunnerCommand::Start { run_id, .. } => vec![
                RunnerEvent::Started { run_id },
                RunnerEvent::Failed {
                    run_id,
                    error: "model_gateway_unavailable".to_owned(),
                },
            ],
            RunnerCommand::CapabilityResult { run_id, result } => {
                if result.success {
                    vec![RunnerEvent::Completed {
                        run_id,
                        output: result.output,
                    }]
                } else {
                    vec![RunnerEvent::Failed {
                        run_id,
                        error: result
                            .output
                            .get("error")
                            .and_then(|value| value.as_str())
                            .unwrap_or("capability_failed")
                            .to_owned(),
                    }]
                }
            }
            RunnerCommand::Cancel { run_id, reason } => vec![RunnerEvent::Failed {
                run_id,
                error: format!("cancelled:{reason}"),
            }],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::RunId;

    #[tokio::test]
    async fn unavailable_model_gateway_fails_without_completed_event() {
        let run_id = RunId::new();
        let events = ProtocolRunner
            .send(RunnerCommand::Start {
                run_id,
                prompt: "hello".to_owned(),
            })
            .await;
        assert_eq!(events.first(), Some(&RunnerEvent::Started { run_id }));
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "model_gateway_unavailable".to_owned(),
            })
        );
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }
}
