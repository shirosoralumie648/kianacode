# EQ-07 evaluation ports baseline

> 快照日期：2026-09-17。本页记录质量端口分层；本地不运行测试，运行时夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EQ-07`](../roadmap.md#step-eq-07) |
| feature_status | `implemented`（EvalStore/FixtureStore/TraceSource/ArtifactReader/Judge/MetricsSink/Clock traits） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | quality domain contracts → `kiana-ports` traits → future quality/core adapters |
| this step does | typed quality object persistence methods, opaque fixture ref+scope bytes, committed run/event cursor source, immutable artifact reader, bounded judge JSON and metrics result sink, deterministic clock |
| this step does not | 不实现任何 store/adapter、文件系统/provider/network、normalizer/runner、Judge 模型调用、outbox 或 promotion authority；EQ-08+ 负责 |

## 2. Port rules

`EvalStore` 只携带 domain `QualityArtifact`、EvalDataset/Suite/Case/GoldenTrace 和 typed IDs，默认 unsupported 返回 `PortError`；未来实现必须保留 digest/revision/CAS 语义。`FixtureStore` 以 opaque `fixture_ref` 与 server-derived `scope_digest` 读取 bytes，不能接收 PathBuf 或 operator workspace。

`TraceSource` 只返回指定 RunId/cursor 后的 committed `RuntimeEvent`；stream delta 不可成为事实。`ArtifactReader` 使用已有 immutable `ArtifactRef`；`Judge` 只返回 JSON finding，不能访问 provider/Broker 或改变 candidate；`MetricsSink` 只写 EvalCaseResult projection；`Clock` 是同步纯值边界。所有 trait 依赖方向向下，不暴露 daemon/provider 私有类型。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `quality_ports_are_free_of_daemon_or_provider_types` | compile-only fake adapters 实现全部端口，默认 unsupported error 明确，无 PathBuf/provider/network |
| `quality_port_traits_remain_below_core_and_have_no_provider_or_filesystem_dependency` | source guard 锁定 domain→ports 分层与 no execution loop |

`.github/workflows/eq07-quality-ports.yml` 在 GitHub runner 执行 ports fixture、core source guard、fmt 和 domain/ports/protocol/core test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- 端口存在不代表有生产 adapter、durable CAS、跨进程恢复、路径隔离或真实 source/fixture ownership；默认方法刻意 fail-closed。
- `Judge` contract 不证明 LLM judge 质量或解释可信；任何 score/finding 都不能直接 Promote/rollback、修改 policy/Grant/Receipt/Acceptance/Outcome。
- FixtureStore/TraceSource/ArtifactReader 尚未接入 DaemonHost/ControlPlane；EQ-08/09/12/13/14/16、EQ-17+ 和 ER/PD/SC 负责受控读取、normalization、evidence、replay 与发布门。
