//! 未选择 owned harness 时使用的 fail-closed 协议 Runner。
//!
//! 该适配器只把 RunnerCommand 映射为协议事件：Start 明确报告模型网关不可用，Continue
//! 报告运行不存在，Cancel 报告取消，CapabilityResult 仅回传已有结果。它不启动模型循环、
//! 不执行工具，也不把不可用状态伪装成 Completed；因此可作为兼容入口的安全兜底，但不是
//! 生产模型执行器。
//!
//! ---------------------------------------------------------------------------
//! 【本文件的设计意图】
//!
//! 这是 kiana-ports::RunnerPort trait 的一个最小化实现。
//! 它体现了 Kiana 的 fail-closed 原则：
//!   * 当模型网关不可用时，不伪造成功，而是**明确返回失败**
//!   * 每个命令都产出确定性事件流，没有副作用或状态变化
//!
//! 典型使用场景：
//!   * 离线环境无法连接模型（test cassette 也无法加载）
//!   * 生产故障排查时想测试「只有协议翻译，不跑模型」的路径
//!   * 验证 daemon 的错误处理是否正确
//!
//! 与生产 KianaHarness 的对比：
//!   | 能力 | ProtocolRunner | KianaHarness |
//!   |---|---|---|
//!   | 启动模型循环 | 否 | 是 |
//!   | 执行工具 | 否 | 间接（翻译成 CapabilityRequest）|
//!   | 记录状态 | 否（无状态） | 是（runs 表） |
//!   | 伪装成功 | 否（fail-closed）| 否（依赖 model 和 broker）|
//!   | 生产使用 | 否（调试/容错）| 是 |

use kiana_runner_protocol::{RunnerCommand, RunnerEvent};

// ============================================================================
// 【无状态 Runner 实现】
// ============================================================================

#[derive(Clone, Copy, Debug, Default)]
/// 仅生成确定性协议事件的无状态 Runner。
///
/// 这个结构体**没有任何字段**。它的所有行为都是确定性的：
/// 同样的命令输入总是产生同样的事件输出。
///
/// 使用这个 Runner 的代码路径是：
/// ```ignore
/// kiana-daemon::DaemonHost
///   ├─ KianaHarness（生产）
///   └─ ProtocolRunner（容错/调试）
///      ↑
///      通过 RunnerPort trait 被 ControlPlane 调用
/// ```
///
/// 它实现 `RunnerPort` 这个 kiana-ports 定义的 trait。
/// ControlPlane 不知道底层是哪个实现，只知道有一个 RunnerPort。
pub struct ProtocolRunner;

impl ProtocolRunner {
    /// 将命令转换为 fail-closed RunnerEvent 列表。
    ///
    /// # 输入
    /// `command`：来自 ControlPlane 的 RunnerCommand
    /// 这个命令是版本化的 wire 类型，定义在 kiana-runner-protocol
    ///
    /// # 输出
    /// `Vec<RunnerEvent>`：事件列表，顺序代表协议事实顺序
    ///
    /// # 关键行为（fail-closed）
    ///
    /// **对所有命令的一致原则**：
    ///   * 不产生虚假的 Completed 事件
    ///   * 不伪造工具执行成功
    ///   * 明确报告「哪里不可用」，用稳定的错误码
    ///
    /// # 实现细节
    /// 返回列表中的顺序是协议事实顺序；调用方仍需把这些事件写入自己的 EventLog
    /// 本函数的返回值本身**不是**持久化证据——EventStore 才是。
    /// 这是因为 runner 只负责**产生**待记录的事实，不负责存储。
    ///
    /// # 副作用
    /// 无。所有计算都是纯的。
    pub async fn send(&self, command: RunnerCommand) -> Vec<RunnerEvent> {
        match command {
            // ─────────────────────────────────────────────────────────────
            // 【Start 命令】开启新 run
            //
            // 正常路径（KianaHarness）：
            //   Start → 组装消息 → 调模型 → 回 Deltas / Tool calls / 文本
            //
            // 本实现（ProtocolRunner）：
            //   Start → 报告「Started」（确认收到）
            //         → 报告「Failed: model_gateway_unavailable」（明确失败）
            //         → 不产生任何 Completed
            //
            // 为什么先 Started 再 Failed：
            //   这样 ControlPlane 能记录「run 曾经启动过」这个事实，
            //   之后看到 Failed 时不会困惑。这是审计追踪的一部分。
            // ─────────────────────────────────────────────────────────────
            RunnerCommand::Start { run_id, .. } => vec![
                RunnerEvent::Started { run_id },
                RunnerEvent::Failed {
                    run_id,
                    error: "model_gateway_unavailable".to_owned(),
                },
            ],

            // ─────────────────────────────────────────────────────────────
            // 【CapabilityResult 命令】Broker 回话
            //
            // 这个命令来自 Broker，表示一个能力请求的结果。
            // ProtocolRunner 无法理解「是否真的执行成功」，
            // 所以只能转发 result 本身的状态。
            //
            // 分支：
            //   result.success = true  → Completed（直通 output）
            //   result.success = false → Failed（提取 error 字段）
            // ─────────────────────────────────────────────────────────────
            RunnerCommand::CapabilityResult { run_id, result } => {
                if result.success {
                    // Broker 说「执行成功」，转发完整输出
                    vec![RunnerEvent::Completed {
                        run_id,
                        output: result.output,
                    }]
                } else {
                    // Broker 说「执行失败」，从 output 里提取错误信息
                    //
                    // 提取步骤：
                    //   1. output.get("error") → Option<serde_json::Value>
                    //   2. as_str()             → Option<&str>
                    //   3. unwrap_or(...)       → &str（fallback 到通用错误码）
                    //
                    // Fallback 到 "capability_failed" 的原因：
                    //   确保**总是**有一个错误码可返回，不会 panic 或产生 None
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

            // ─────────────────────────────────────────────────────────────
            // 【Continue 命令】对已有 run 追加一轮
            //
            // 正常路径（KianaHarness）：
            //   Continue → 查表找到对应 run → 继续模型循环 → 产出事件
            //
            // 本实现（ProtocolRunner）：
            //   Continue → 我没有 runs 表 → Failed: run_not_found
            //
            // 这是 fail-closed：而不是伪造一个「立即完成」的事件
            // ─────────────────────────────────────────────────────────────
            RunnerCommand::Continue { run_id, .. } => vec![RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            }],

            RunnerCommand::Resume { run_id } => vec![RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            }],

            // ─────────────────────────────────────────────────────────────
            // 【Inject 命令】往收件箱塞消息
            //
            // 正常路径（KianaHarness）：
            //   Inject → 存进 inbox → 下一个 model_step 时取走 → 加入消息序列
            //
            // 本实现（ProtocolRunner）：
            //   Inject → 我没有 runs 表 → Failed: run_not_found
            //
            // 同样是 fail-closed 的体现
            // ─────────────────────────────────────────────────────────────
            RunnerCommand::Inject { run_id, .. } => vec![RunnerEvent::Failed {
                run_id,
                error: "run_not_found".to_owned(),
            }],

            // ─────────────────────────────────────────────────────────────
            // 【Cancel 命令】取消运行
            //
            // 正常路径（KianaHarness）：
            //   Cancel → 标记取消信号 → 模型步在检查时发现信号 → 停止循环
            //
            // 本实现（ProtocolRunner）：
            //   Cancel → 无法真正停止（没有运行） → Failed（混入取消原因）
            //
            // 错误信息格式 "cancelled:{reason}"：
            //   这样可以区分「主动取消」和「其他失败」，
            //   便于日志和重试策略的判定
            // ─────────────────────────────────────────────────────────────
            RunnerCommand::Cancel { run_id, reason } => vec![RunnerEvent::Failed {
                run_id,
                error: format!("cancelled:{reason}"),
            }],
        }
    }
}

// ============================================================================
// 【测试】
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use kiana_domain::RunId;

    /// 验证核心行为：Start 命令绝不产生 Completed 事件
    ///
    /// 这个测试检验 fail-closed 原则的最基本层面：
    /// 即使模型网关不可用，也必须报告失败，不能假装成功。
    ///
    /// 测试流程：
    ///   1. 生成一个 run_id
    ///   2. 调用 ProtocolRunner::send(Start)
    ///   3. 检查事件列表：
    ///      - 第一个事件是 Started
    ///      - 最后一个事件是 Failed（特定错误码）
    ///      - 列表中**没有** Completed 事件
    #[tokio::test]
    async fn unavailable_model_gateway_fails_without_completed_event() {
        let run_id = RunId::new();
        let events = ProtocolRunner
            .send(RunnerCommand::start(run_id, "hello"))
            .await;

        // 检查事件序列的头和尾
        assert_eq!(events.first(), Some(&RunnerEvent::Started { run_id }));
        assert_eq!(
            events.last(),
            Some(&RunnerEvent::Failed {
                run_id,
                error: "model_gateway_unavailable".to_owned(),
            })
        );

        // 最关键的检查：不存在 Completed 事件
        assert!(!events
            .iter()
            .any(|event| matches!(event, RunnerEvent::Completed { .. })));
    }
}
