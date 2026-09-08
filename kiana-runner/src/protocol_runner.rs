//! 未选择 owned harness 时使用的 fail-closed 协议 Runner。
//!
//! 该适配器只把 RunnerCommand 映射为协议事件：Start 明确报告模型网关不可用，Continue
//! 报告运行不存在，Cancel 报告取消，CapabilityResult 仅回传已有结果。它不启动模型循环、
//! 不执行工具，也不把不可用状态伪装成 Completed；因此可作为兼容入口的安全兜底，但不是
//! 生产模型执行器。

use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

#[derive(Clone, Copy, Debug, Default)]
/// 仅生成确定性协议事件的无状态 Runner。
pub struct ProtocolRunner;

impl ProtocolRunner {
    /// 将命令转换为 fail-closed RunnerEvent 列表。
    ///
    /// 返回列表中的顺序是协议事实顺序；调用方仍需把这些事件写入自己的 EventLog，不能
    /// 把本函数的返回值本身当作持久化证据。
    pub async fn send(&self, command: RunnerCommand) -> Vec<RunnerEvent> {
        match command {
            // 未接入模型网关时先报告启动，再报告明确失败，绝不产生完成事件。
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
            RunnerCommand::Continue { run_id, .. } => vec![RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            }],
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
            .send(RunnerCommand::start(run_id, "hello"))
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
