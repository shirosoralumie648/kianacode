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
