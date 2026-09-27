//! Kiana 控制面中位于策略判断之后的确定性 Gate（关卡）。
//!
//! # 这个文件在系统里的位置
//!
//! 一次工具调用要真正执行，必须连过三道独立的检查。本文件负责的是**第二道**：
//!
//! ```text
//! 模型请求执行某个工具
//!        ↓
//!    ① kiana-policy        策略层：算出「这个身份能不能做这件事」
//!        ↓  产出 PolicyDecision（Allow / Ask / Deny）
//! 【本文件 kiana-gates】   关卡层：把策略结论收敛成「控制面现在该做什么」
//!        ↓  产出 GateDecision（Allowed / AwaitingApproval / Denied）
//!    ③ kiana-capability-broker   执行层：真正调用工具
//! ```
//!
//! 上游：`kiana-core::ControlPlane` 在准备派发能力请求时调用 [`GateEngine::evaluate`]。
//! 下游：本文件不调用任何东西 —— 它是纯函数，没有 I/O、没有网络、没有副作用。
//!
//! # 为什么策略层和关卡层要分成两段
//!
//! 因为它们的**职责不同，失败方式也不同**。
//!
//! 策略层回答的是业务问题：「按角色、路径、权限档位，这个操作合不合规？」
//! 它需要读上下文、算权限，是有分量的一段逻辑。
//!
//! 关卡层回答的是控制问题：「策略已经给出结论了，控制面接下来允许做什么？」
//! 它是一次纯粹的**状态收敛（state convergence）**，没有判断，只有映射。
//!
//! 拆开的直接好处是**单调收紧（monotonic tightening）可以被结构性地保证**：
//! 本文件没有权限去"重新解释"策略意图，它唯一的自由度是拒绝。所以
//! 策略层说 Ask，这里绝不可能变成 Allowed。
//!
//! # 术语
//!
//! - **fail-closed（失败即关闭）**：遇到异常输入时选择拒绝，而不是猜测放行。
//!   这是本 crate 的默认立场 —— 详见 [`DefaultGateEngine::evaluate`] 里空授权 ID 的处理。
//! - **Gate（关卡）**：策略结论到执行许可之间的最后一道转换。名字来自
//!   "gate"（闸门），暗示它是**放行前**的检查点，而不是放行后的审计。
//! - **授权 ID（authorization_id）**：策略层签发的一次性身份标识，把这一次决策和
//!   后续的事件记录、审批回执绑定在一起。它为空就意味着这次决策不可审计。
//!
//! # 边界声明（不要越界理解）
//!
//! 本文件**不执行工具、不签发新权限、不重新猜测策略意图**。它证明的只是
//! 「映射规则正确」，不代表人工审批、持久化审计或外部副作用已经发生。
//!
//! # 上游契约
//!
//! `kiana-policy` 先根据上下文和能力请求给出 [`PolicyDecision`]；本 crate 再把该结果
//! 收敛为控制面可执行的 [`GateDecision`]。Gate 不执行工具、不签发新权限，也不重新猜测
//! 策略意图。它只能保留或收紧上游决定：`Ask` 必须继续等待审批，`Deny` 必须继续拒绝，
//! 只有带非空授权 ID 的 `Allow` 才能进入后续授权封装。
//!
//! 当前实现是纯函数式、无 I/O 的本地 Gate。它证明的是映射规则，而不是人工审批、
//! 持久化审计或外部副作用已经完成。

use kiana_domain::{GateDecision, PolicyDecision};

/// 将策略结果转换为控制面 Gate 结果的可替换接口。
///
/// 【作用】
/// 定义「策略结论 → 控制面下一步动作」这一步的唯一入口。控制面只依赖这个 trait，
/// 不依赖具体实现，因此将来可以替换 Gate 策略而不用改控制面。
///
/// 【调用者】
/// `kiana-core::ControlPlane` 在派发能力请求（CapabilityRequest）之前调用 [`GateEngine::evaluate`]。
/// 上游传入的是 `kiana-policy` 刚算出的 [`PolicyDecision`]。
///
/// 【不变量 —— 实现者必须遵守】
/// 实现必须满足**单调收紧（monotonic tightening）**原则：
///
/// 1. 不得把 [`PolicyDecision::Ask`] 转成允许 —— Ask 的语义是"停下来等人类审批"，
///    关卡层没有权限替人类做这个决定；
/// 2. 不得把 [`PolicyDecision::Deny`] 转成允许 —— 同理，策略层已经拒绝，
///    下游没有"翻案"权限；
/// 3. 不得替换上游签发的授权 ID —— 授权 ID 是审计链的锚点，一旦被换掉，
///    这次决策就和事件记录对不上号了。
///
/// 换句话说：Gate 只能**保留或收紧**上游决定，永远不能放宽。
/// 这条约束如果被破坏，整个授权体系就失去了意义 —— 因为一个可以随意
/// 把 Deny 变成 Allow 的关卡，等于不存在。
///
/// 【为什么这个 trait 保持同步（非 async）】
/// Gate 判断只依赖已经计算好的**不可变策略快照**，不应该在这里引入网络调用或长 I/O。
/// 保持同步有两个实际好处：一是控制面的派发路径上不会多出一个可挂起的点，
/// 二是拒绝路径的测试可以完全确定性复现，不依赖时序。
pub trait GateEngine: Send + Sync {
    /// 评估一份策略决定，返回控制面下一步应采取的 Gate 状态。
    ///
    /// 【作用】
    /// 把策略层的结论收敛成控制面可以直接执行的动作。
    ///
    /// 【输入】
    /// - `policy`：上游 `kiana-policy` 算出的策略结论。可能是
    ///   `Allow { authorization_id }`、`Ask { reason }` 或 `Deny { reason }`。
    ///
    /// 【输出】
    /// 三选一的 [`GateDecision`]：
    /// - [`GateDecision::Allowed`] —— 可以继续进入授权与 Broker 流程；
    /// - [`GateDecision::AwaitingApproval`] —— 挂起，等待独立审批流程显式恢复；
    /// - [`GateDecision::Denied`] —— 拒绝，附带一个稳定的原因字符串。
    ///
    /// 【副作用】
    /// 无。实现不应写文件、发网络请求或修改任何状态。
    ///
    /// 【失败情况】
    /// 遇到缺失或畸形的授权证据时应 **fail-closed（失败即关闭）** —— 也就是选择拒绝，
    /// 而不是猜测放行。具体来说，`Allow` 携带的授权 ID 为空或全是空白时，
    /// 无法把这次决策与事件账本绑定，因此必须当作拒绝处理（见 [`DefaultGateEngine`] 的实现）。
    ///
    /// 【重要边界】
    /// 返回 [`GateDecision::Allowed`] 只代表请求可以继续进入授权与 Broker 流程，
    /// **并不代表副作用已经执行**。真正的执行发生在 `kiana-capability-broker`，
    /// 那之后还要再写事件回执（Receipt）。初学者最容易在这里误解：
    /// Allowed ≠ 已经执行。
    fn evaluate(&self, policy: &PolicyDecision) -> GateDecision;
}

#[derive(Clone, Copy, Debug, Default)]
/// 产品主路径使用的确定性 Gate 实现。
///
/// 【作用】
/// [`GateEngine`] 的默认实现。名字里的 "Default" 指的是**当前产品主路径唯一使用的那个**，
/// 不是"备用的、暂时能跑的那个"。
///
/// 【为什么是无状态单元结构体】
/// 类型本身无状态（没有任何字段），因此可以按值复制并在多个请求间共享；
/// 所有决定完全由输入的 [`PolicyDecision`] 决定。
///
/// 这一点对可验证性很关键：因为无状态，**同一个输入永远得到同一个输出**，
/// 所以拒绝路径可以做稳定测试（见文件末尾的 tests 模块），事件记录也可以
/// 直接引用这次判定而不用担心"事后状态变了导致结论无法复现"。
///
/// 【并发安全】
/// 零字段意味着天然满足 `Send + Sync`，不需要任何锁或原子操作。
/// 可以放心地在控制面的并发派发路径上共享同一个实例。
pub struct DefaultGateEngine;

impl GateEngine for DefaultGateEngine {
    /// 【核心流程】
    /// 对策略结论做一次穷尽匹配（`match`），四条分支分别对应三种策略结论
    /// 加上"允许但证据缺失"这一种畸形情况：
    ///
    /// | 策略结论 | Gate 结论 | 理由 |
    /// |---|---|---|
    /// | `Allow` + 非空授权 ID | `Allowed` | 正常放行，授权 ID 原样传递 |
    /// | `Allow` + 空/空白授权 ID | `Denied` | 无法审计，fail-closed |
    /// | `Ask` | `AwaitingApproval` | 保留等待，绝不就地放行 |
    /// | `Deny` | `Denied` | 保留策略层给出的稳定原因 |
    ///
    /// 【为什么用穷尽 match 而不是 if/else 链】
    /// `PolicyDecision` 是枚举（enum）。用穷尽匹配可以在编译期强制处理所有变体 ——
    /// 上游将来新增一个变体，这里会**编译失败**，而不是悄悄走进某个兜底分支。
    /// 这正是"fail-closed"在类型层面的体现：漏处理等于构建不过。
    ///
    /// 【副作用】
    /// 无。`GateDecision` 是纯值类型，这里只做构造，不碰任何外部状态。
    fn evaluate(&self, policy: &PolicyDecision) -> GateDecision {
        match policy {
            // 正常路径：Gate 只能传递策略已经签发的授权 ID，不能在这里生成一个替代值。
            //
            // ⚠ 注意：这里写的是 `authorization_id.clone()` 而不是 `Default::default()`
            // 或任何新生成的 ID。授权 ID 是审计链的锚点 —— 事件账本、审批回执
            // 都会引用它。如果 Gate 在这里"补一个" ID，审计链就断在这里了：
            // 事件里记的是 Gate 造的 ID，策略层签发的是另一个 ID，
            // 两者对不上，任何事后追溯都会得出错误结论。
            //
            // `.trim().is_empty()` 而不是 `.is_empty()`：要同时挡住空字符串和
            // 纯空白字符串。因为 `"   "`（三个空格）同样无法审计，
            // 它通过了 `.is_empty()` 检查。
            PolicyDecision::Allow { authorization_id } if !authorization_id.trim().is_empty() => {
                GateDecision::Allowed {
                    authorization_id: authorization_id.clone(),
                }
            }
            // "允许但没有授权身份"无法审计或绑定后续请求，因此按拒绝处理。
            //
            // ⚠ 这里看起来"奇怪" —— 策略层说 Allow，Gate 却返回 Denied。
            // 这不是 bug，而是 fail-closed 的直接体现：策略层可能因为上游数据
            // 缺失而给出一个空的授权 ID，Gate 的职责就是在执行前把它拦住。
            //
            // ⚠ 不要把这里改成"照样放行"或"返回 Allowed 但 ID 为空"。
            // 后果是：一次没有授权身份的副作用会真实执行，却无法追溯是谁批准的、
            // 依据哪次决策批准的。授权体系的可审计性就断在这里。
            //
            // 原因字符串用下划线命名（"authorization_id_required"）而不是自然语言，
            // 因为它会原样进入事件账本和 CLI/UI 展示，属于**稳定契约**：
            // 改成别的拼写会让依赖该字符串的测试和下游解析一起失效。
            PolicyDecision::Allow { .. } => GateDecision::Denied {
                reason: "authorization_id_required".to_owned(),
            },
            // Ask 保持为暂停状态，等待独立审批流程显式恢复，不能就地放行。
            //
            // "就地放行"指的就是把 AwaitingApproval 直接当成 Allowed 用掉。
            // Ask 的语义是"这个操作超出当前权限，需要人点头"，
            // 恢复的唯一合法途径是走完审批流程并由审批结果显式驱动，
            // 绝不能由关卡层自行决定跳过。
            PolicyDecision::Ask { reason } => GateDecision::AwaitingApproval {
                reason: reason.clone(),
            },
            // 保留策略层给出的稳定原因，供事件账本和入口层展示同一事实。
            //
            // 关键在于**原样传递**（`reason.clone()`）而不是重新生成一句描述。
            // 策略层给出的 reason 是经过测试的稳定错误码（如 "role_tool_denied"），
            // 如果 Gate 把它改写成人话，事件账本里记下的就变成了另一个字符串，
            // 下游按码分类的逻辑（比如统计"因角色被拒的次数"）会全部失效。
            //
            // 也是本 crate 里唯一一处"把上游信息透传下去"的地方 ——
            // 它保证了策略层和事件层看到的是同一个事实。
            PolicyDecision::Deny { reason } => GateDecision::Denied {
                reason: reason.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_never_becomes_allowed() {
        assert!(matches!(
            DefaultGateEngine.evaluate(&PolicyDecision::Ask {
                reason: "approval_required".to_owned(),
            }),
            GateDecision::AwaitingApproval { .. }
        ));
    }

    #[test]
    fn empty_authorization_id_fails_closed() {
        assert!(matches!(
            DefaultGateEngine.evaluate(&PolicyDecision::Allow {
                authorization_id: String::new(),
            }),
            GateDecision::Denied { .. }
        ));
    }

    #[test]
    fn deny_reason_is_preserved_and_valid_authorization_id_is_unchanged() {
        // Gate 只能收紧上游决定，拒绝原因和已经签发的授权 ID 都必须原样保留。
        let denied = DefaultGateEngine.evaluate(&PolicyDecision::Deny {
            reason: "role_tool_denied".to_owned(),
        });
        assert_eq!(
            denied,
            GateDecision::Denied {
                reason: "role_tool_denied".to_owned(),
            }
        );

        let allowed = DefaultGateEngine.evaluate(&PolicyDecision::Allow {
            authorization_id: "policy:req-1".to_owned(),
        });
        assert_eq!(
            allowed,
            GateDecision::Allowed {
                authorization_id: "policy:req-1".to_owned(),
            }
        );
    }
}
