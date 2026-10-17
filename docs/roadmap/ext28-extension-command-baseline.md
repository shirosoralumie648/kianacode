# EXT-28 命令与 UI 合同基线

> 快照日期：2026-09-24。本文件记录 EXT-28 的 source slice 和 GitHub-only 验证接线；CI 结果未等待，也不提升 proof level。

## 范围

`ExtensionCommandRequest` 为 `extension.list`、`extension.inspect`、`extension.install`、
`extension.enable`、`extension.disable`、`extension.revoke` 和 `extension.rollback` 提供
版本化 DTO。只读 list/inspect 不携带 mutation 字段；状态改变必须同时绑定 actor、reason、
idempotency key 和 expected registry version。install/rollback 还必须绑定项目相对包路径和
最终 sha256。`ExtensionCommandError`、`ExtensionCommandReceipt` 和 response projection
拒绝未知字段并保留冲突、审批、权限和重试分类。

所有入口仍发送 generic `CommandRequest`，由 `DaemonHost → ControlPlane → Broker` 处理。
CLI、Workbench、Web 和 Desktop 只调用共享 `extension_projection` adapter，读取事件/receipt
投影，不解析包、不执行 Hook、不决定 scope 或审批。`ExtensionRegistry` 对 enable/disable
追加受 expected-version 和 idempotency 保护的 lifecycle event；拒绝在授权或 CAS 前不写事实。

## 证据块

```text
source_snapshot: origin/master=4e70396a plus EXT-28 isolated source slice
worktree_status: ext-28-extension-commands-20260924; typed domain/protocol/client contracts, ControlPlane command aliases, enable/disable lifecycle transitions, shared UI adapter, domain/protocol/core fixtures, workflow and roadmap overlays are scoped to this step; unrelated WIP is not staged
command_argv:
  git diff --check
  GitHub Actions: cargo fetch --locked; cargo fmt --all --check; cargo test -p kiana-domain --test ext28_extension_commands --locked -- --test-threads=1; cargo test -p kiana-protocol --test ext28_extension_command_wire --locked -- --test-threads=1; cargo test -p kiana-core --test ext28_extension_command_guard --locked -- --test-threads=1; cargo check --workspace --tests --locked
cwd·environment: /tmp/kiana-step-ext28; Linux x86_64; local tests/build/check/clippy/smoke deliberately not run; GitHub CI is the test authority and is not awaited
fixture·cassette: GitHub-only EXT-28 domain deny/allow/strict-serde fixtures, generic-envelope wire round-trip, and core source guard; no package, Hook or external connector cassette
exit_code: git diff --check only; remote fixture/build results are pending and intentionally unobserved
status change: EXT-28 source slice is implemented. Seven typed command names, structured error/receipt DTOs, mutation preconditions, exact command aliases, enable/disable registry transitions, and a shared non-executing UI adapter are wired through the existing ControlPlane route; roadmap remains 🔄 pending GitHub CI evidence.
proof-level change: feature_status=implemented; proof_level=source plus CI wiring; no local_behavior, durable, live or physical promotion
limitations: GitHub CI has not been observed; no local runtime tests were run; durable cross-process package install/recovery, approval UX, Hook execution, external connector effects, and real provider/package signing remain later slices
reviewer: Codex source review of versioned DTO bounds, deny-first mutation fields, actor/reason/idempotency/CAS binding, receipt-only UI projection, and no second execution loop; no local runtime test reviewer
```

## 2026-10-17 completion slice（CI-only）

上一节记录的是 2026-09-24 的初始 source slice；它承认一个未收口缺口：
`ExtensionCommandRequest` 缺少 approval/config/dependency/trust 绑定字段，daemon 的
`enable` 也不消费这些类型化快照（见 `CURRENT_STATUS.md` 2026-10-08 EXT-22 证据块的
limitations），因此 typed `extension.install` / `extension.enable` 事实上无法在产品路径上
通过 ControlPlane 规范化。本切片把这个缺口补成完整的「命令契约 + 权限错误 + 审计关联」：

1. **命令契约**（`kiana-domain/src/extension_commands.rs`）：`ExtensionCommandRequest`
   新增可选 `trust_revision`、`dependency_snapshot_digest`、`approval_id`、
   `config_snapshot_digest`、`config_snapshot`；`validate()` 逐动作要求绑定
   （`install`/`enable` 需 trust + dependency，`enable` 还需 approval + config snapshot 与
   自证摘要一致、同一 extension、同一 expected registry version），形状错误在本地拒绝
   （`extension_command_trust_revision_invalid`、
   `extension_command_dependency_snapshot_invalid`、
   `extension_command_config_snapshot_digest_invalid`、
   `extension_command_approval_id_invalid`、
   `extension_command_config_snapshot_digest_mismatch`、
   `extension_command_config_snapshot_extension_mismatch`、
   `extension_command_config_snapshot_registry_version_mismatch`），只读命令仍不得携带任何
   mutation 字段；`to_arguments()` 把绑定原样编码到共享 `extension.manage` 路由，
   且不发送 `mutation_digest`（该摘要只能由 ControlPlane 在注入服务端字段后铸造）。
2. **audit 关联**：`ExtensionCommandReceipt` 增补可选 `mutation_digest`/`approval_id`，
   并新增 `ExtensionCommandReceipt::from_response_output`，把 daemon 已提交的
   `command_receipt` 解码并重新自证；`kiana-entrypoints` 共享 adapter 暴露
   `command_receipt`/`command_receipt_json`，四入口只投影服务端回执。daemon 幂等重放路径
   现在也返回同一形状的 `command_receipt`（`replayed=true`、同一 `event_id`），而不是退回
   generic lifecycle envelope。
3. **权限错误与拒绝路径**：
   - domain：本地拒绝矩阵（只读携带 mutation 字段、缺绑定、摘要漂移、批准 id 非法、
     inspect 缺 id、package 路径逃逸、expected version 0）。
   - `kiana-daemon` 库夹具 `ext28_commands_are_permission_bound_and_the_receipt_resolves_to_the_committed_event`：
     read-only list 无审批、零事实；read-only 风险的变更 → `extension_approval_risk_required`；
     无 operator → `extension_operator_required`；cell 上下文 → `extension_operator_required`；
     缺 expected version → `extension_expected_registry_version_required`；以上均不追加事实；
     被接受的 `disable` 恰好追加一条 CAS 事实，回执 `event_id` 指向该事件
     （`stream_version`、`kind`、`action` 逐项核对）。
   - `kiana-daemon` 库夹具 `ext28_replayed_commands_return_the_same_receipt_and_event_without_a_second_fact`：
     同 key 同 payload 重放返回原 `event_id` + `replayed=true` 的类型化回执且不追加第二事实；
     同 key 改 payload → `extension_idempotency_payload_mismatch`。
   - `kiana-entrypoints/tests/ext28_extension_commands.rs`（真实 `DaemonHost` + 共享
     adapter，`KIANA_HOME` 隔离 + 文件内 `ENV_LOCK` 串行）：
     `untrusted_project_refuses_every_command_before_authority_and_leaks_nothing`（七个命令
     一律 `Denied`/`project_untrusted`、`output: null`、不回显 extension id/包路径；
     零 lifecycle 事实）与
     `trusted_project_keeps_reads_free_and_state_changes_approval_bound`（本地 DTO 拒绝先于
     传输；actor 缺失 → `Blocked`/`extension_operator_required`；缺绑定 →
     `extension_mutation_fields_required` 且不回显 payload；CLI/Workbench/Web/Desktop 读到同一
     `snapshot_id`/`generation`/`snapshot_digest` 且不产生回执/事实；合法变更停在
     `AwaitingApproval`/`external_side_effect_requires_approval`，零 lifecycle 事实）。
   - `kiana-core/tests/ext28_extension_command_guard.rs` 新增
     `command_binding_and_receipt_projection_stay_on_the_shared_route`（DTO 绑定键与共享路由
     白名单一致、拒绝码、回执解码、重放同形状）与
     `ext28_fixtures_stay_wired_to_the_manual_lane_and_the_auto_shards`（夹具名、四入口标签、
     拒绝码与 lane 命令逐字绑定）。
4. **CI 接线**：新增手动 lane `.github/workflows/ext28-extension-commands.yml`
   （`workflow_dispatch`-only、`permissions: contents: read`、fmt 门 + daemon `--lib --no-run`
   + domain/protocol/core/daemon/entrypoints 五个 target）。自动覆盖仍是统一 `ci.yml` 的
   `kiana-domain-s3/4`（`ext28_extension_commands`）、`kiana-core-s4/6`
   （`ext28_extension_command_guard`）与 `kiana-protocol`/`kiana-daemon`/`kiana-entrypoints`
   整 crate 分片（`targets: null`，自动涵盖 `ext28_extension_command_wire`、
   `extensions::tests::ext28_*` 与新的入口 target）；未改 `Cargo.toml`/`Cargo.lock`/
   `package.json` 或 `scripts/ci/test-shards.json`。

本切片仍不提升 proof level：local_behavior 需要 GitHub Actions 回执，本会话未运行任何本地
测试/构建，CI 回执未等待。四个模型可见工具面（`shell`/`apply_patch`/`mcp`/`memory.search`/
`memory.write`）与唯一执行脊柱（`DaemonHost → ControlPlane → Broker → EventStore`）不变。
