# H18 持久 Inbox、ACK 与原子消费基线

> 快照日期：2026-09-18。本页记录 Runner Inbox 的输入身份、目标边界、幂等接收、claim ledger 与 checkpoint 接缝；本地不运行测试，夹具仅由 GitHub Actions 执行。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H18`](harness.md#step-h18) |
| feature_status | `implemented`（Runner checkpoint/幂等 Inbox/ACK DTO；ControlPlane 入口 EventLog 接线已由 H19 接通） |
| proof_level | `source`；本地仅做格式与 workspace test-target 静态编译，GitHub Actions 负责 fixtures |
| authority | InputId/receipt facts are server-owned; Runner only stores a bounded queue and never parses input text as authority |
| this step does | InboxMessage 绑定 InputId、source、target、target turn、received sequence；队列 bounded backpressure、duplicate/claimed dedupe、InputReceipt ACK 与 claim ledger；HarnessCheckpoint 序列化/恢复 inbox，claim 在下一安全 step/turn 边界消费 |
| this step does not | 不在 Runner 直接执行命令或唤醒第二模型循环；当前 inject/steer 仍是同进程 helper，尚未由所有入口经 ControlPlane 原子写 `input.accepted/claimed`，大 payload Artifact 和跨进程 projector 留待 H19/PD |

## 1. Contract

输入先获得 InputId 和有界 received sequence，再按 NextTurn/NextStep 入队；重复 InputId 返回
原队列/claimed 语义而不复制文本，队列满或 claim ledger 满返回 backpressure。消息 source、
target turn 和文本长度在入队时校验，claim 前再次由 Runner 检查 target turn，跨 turn steering
fail-closed。

`InputReceipt` 是可序列化的 ACK/consumed 投影，包含 Run、source、target、sequence 和
accepted/duplicate/claimed disposition。Inbox 及 claimed ledger 随 HarnessCheckpoint 保存，
恢复后不丢失 accept order，也不会因重放再次消费同一 InputId；上层仍需在 ControlPlane 事务中
把 accepted/claimed 回执写入 EventLog，不能把内存队列当作最终事实。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `crash_between_claim_and_request_does_not_lose_input` | Inbox/claim receipt/checkpoint 结构保留输入事实，Runner 不静默丢弃 |
| `duplicate_input_is_consumed_once` | 同 InputId 只进入一次队列，claim 后重发返回 Claimed |
| `cross_turn_steering_is_rejected` | target_turn_id 与当前 Turn 不一致时 fail-closed |
| `queued_followups_survive_restart_in_accept_order` | checkpoint round-trip 保留两级队列和接收序号 |

## 3. Proof ceiling and handoff

H18 proof ceiling 为 `source`：typed InputId/ACK、bounded queues、duplicate/claim ledger、
target-turn fence 和 checkpoint round-trip 已建立。H19 已补上 additive steer/inject 接线；跨入口
accepted/claimed 的完整原子事务、steering/reconnect 期间 mailbox、Artifact-backed payload、跨
进程恢复和 live/physical proof 仍留待后续 H/PD/ER。
