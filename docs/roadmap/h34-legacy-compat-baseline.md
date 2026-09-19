# H34 旧协议、cassette 与入口迁移基线

> 快照日期：2026-09-19。本页记录 H34 的 source slice 与 CI-only 夹具；本地不运行测试，GitHub Actions 负责运行域、协议和 Core source guard。

| 项目 | 记录 |
|---|---|
| roadmap card | [`H34`](harness.md#step-h34) |
| feature_status | `implemented`（domain compatibility/checkpoint decision contract + existing route source guard；CI-only fixtures） |
| proof_level | `source`；本地只做格式与差异检查，GitHub Actions 运行聚焦夹具且不等待结果 |
| authority | Legacy adapters only classify compatibility; DaemonHost/ControlPlane/Runner remain the one execution spine and server IDs remain canonical |
| this step does | 旧 Run/Continue/Event/Checkpoint/Cassette 的 compatibility disposition、server-ID preservation、same-daemon route 和 unknown/incomplete checkpoint read-only/not-supported decision 已版本化；existing Continue/DaemonHost/Runner checkpoint/entrypoint path source-guarded |
| this step does not | 不静默 upcast unknown checkpoint、不随机填 Run/Turn/Invocation identity、不复制旧模型循环；真实旧 cassette 全量回归和每个 Desktop/legacy resident 入口 live 迁移仍留 H35/H36 |

## 1. Contract

`LegacyCompatibilityDecision` 明确 input/output schema、Compatible/MigrateReadOnly/NotSupported
与 server-ID/DaemonHost 保留情况。`LegacyCheckpointDecision` 只有已知
`kiana.harness-checkpoint.v1`、version=1 且 RunId+TurnId 完整时才允许 resume；未知 schema/version
或身份缺失只能只读迁移或拒绝，绝不调用 `RunId::new()` 猜恢复位置。

旧 Continue 仍是 additive compatibility command，路由复用现有 RequestEnvelope → DaemonHost →
ControlPlane → Runner；legacy cassette 只作为模型边界输入，不获得工具/权限旁路。

## 2. CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `known_checkpoint_with_complete_identity_is_resume_compatible` | known version and complete server IDs may be explicitly resumed |
| `unknown_checkpoint_version_is_not_resumed` | unknown version and incomplete identity are never auto-resumed |
| `legacy_continue_is_additive_and_preserves_one_daemon_route` | legacy Continue keeps server IDs and one DaemonHost route |
| `legacy_paths_remain_on_one_daemon_host_and_unknown_checkpoint_stays_closed` | existing Runner/Daemon/entrypoint route and checkpoint fail-closed source markers remain present |

## 3. Proof ceiling and handoff

H34 proof ceiling is `source`: compatibility decisions and unknown-checkpoint refusal are explicit.
Full old cassette corpus, all legacy resident/Desktop paths, cross-process migration and live provider
compatibility remain H35/H36/PD/provider evidence.
