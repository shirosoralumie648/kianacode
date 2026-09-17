# CP-10 approval facts 与单次消费基线

> 快照日期：2026-09-17。本页记录审批决定、单次消费和 expected-version 的 strict source/CI
> 边界；不把显示投影或一次编译写成完整跨进程恢复证明。

| 项目 | 记录 |
|---|---|
| roadmap card | [`CP-10`](control-plane.md#step-cp-10) |
| feature_status | `implemented`（decision/consumption facts、journal CAS、version-bound protocol） |
| proof_level | `source`；本地不运行测试，GitHub Actions 负责 fixtures |
| authority | EventLog-backed JournalApprovalStore；Core dispatch 在同一 read-set 中消费 Approved |
| this step does | strict decision/consumption facts、command idempotency、expected-version conflict、Approved 与 Consumed 分离、pending list version projection |
| this step does not | 不声称旧内存文件适配器等价 durable authority、不支持 standing/input approval、不声称跨进程 effect exactly-once 或 external/live/physical proof |

## 1. Contract

`kiana-domain/src/approval_journal.rs` 新增 `ApprovalDecisionFact` 和
`ApprovalConsumptionFact`。二者只携带 approval/request/command/authority identities、expiry、
expected aggregate version 与 digest；严格 schema、unknown-field、主体/authority/时间/版本
校验和 decision→consumption 链接校验 fail-closed。`ApprovalDecisionRecord` 暴露 decision 与
consumption digest，供状态回执和重放投影使用。

`JournalApprovalStore` 将 typed fact 嵌入 `approval.approved`/`approval.denied` 与
`approval.consumed` 事务事实，fold 会验证 event command、subject request/hash、authority
version、expiry 和前序 aggregate version。approve/deny 竞争继续由 EventStore transition CAS
决定唯一赢家；同一 command 由 Core 先重放已记录决定，不重新生成执行权。`prepare_consumption`
仍只准备 Approved→Consumed，实际提交与 execution permit 在 dispatch read-set 中完成。

协议的 `ApprovalDecisionRequest.expected_version` 是可选的 server-observed OCC assertion；旧
adapter 在收到它时显式返回 unsupported，Journal adapter 对未决记录拒绝 stale version。pending
view 仅在可读记录时显示该版本，available decisions 仍由服务器固定为 approve/deny。

## 2. CI-only fixture catalog

| Fixture | Assertion |
|---|---|
| `approval_decision_and_consumption_are_distinct_strict_facts` | 两种 fact schema/digest 独立，Approved→Consumed version 链严格成立 |
| `approval_facts_reject_replay_widening_and_unknown_fields` | raw payload/unknown field 与篡改 version fail-closed |
| `cp10_approval_facts_use_one_journal_cas_and_keep_approval_separate_from_dispatch` | source guard 固定单一 journal CAS、replay、expected-version、无 standing 绕过 |

## 3. Proof ceiling and handoff

CP-10 proof ceiling 为 `source`：事实形状、CAS 输入和回执边界已固定，但仍需 CP-13 将
consumption、budget、lease、permit 统一原子化；旧 `MemoryApprovalStore` 仅兼容测试/迁移输入，
volatile secret payload、Human Inbox/外部 approver authentication、跨进程 crash/recovery、
Broker effect confirmation 与 external/live/physical proof 未宣称。
