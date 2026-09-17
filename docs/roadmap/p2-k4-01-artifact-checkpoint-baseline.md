# P2-K4-01 Artifact 版本与编辑级 undo 基线

> 快照日期：2026-09-18。本文记录 workspace checkpoint/preview/restore 的编辑级边界；运行时验收由 GitHub Actions 负责，本地不运行测试。

## Checkpoint contract

`WorkspaceCheckpoint` 是受控项目内的不可变文件快照，绑定 project/actor/session/role、可选 WorkPacket/run、transcript offset、invocation id、reason、path allow、workspace revision（快照文件列表 digest）、data epoch 和每个文件的 contents/mode。ControlPlane 在用户输入前和非只读写 invocation 前捕获 checkpoint，并将 `workspace.checkpoint_created` 写入 EventLog；路径、敏感内容、大小、UTF-8、hardlink/symlink 和文件 metadata 在 descriptor-relative 读取时 fail-closed。

`workspace.checkpoint.preview` 只重新读取当前文件并返回 current/target revision、changes、writes_performed=false；它不写盘、不执行工具、不消耗 approval。`workspace.checkpoint.restore` 要求 operator/trusted context、exact checkpoint event snapshot、project/company/path/data epoch、current revision 和 approval/policy/gate/permit 重新授权，实际写入复用现有 apply_patch lock/precondition/descriptor-relative transaction，完成后再次读取并验证目标 revision。

## Restore invalidation

restore 在任何写入前调用 `prepare_checkpoint_restore`：失效 project approvals、停止 project runs、建立 data/authority fence；旧 runner context 和旧 approval 不能继续沿用。成功后 `finish_checkpoint_restore` 追加 `workspace.restored`，绑定 checkpoint/revision/request，并明确 `old_approvals_invalidated=true`、`runner_context_invalidated=true`。stop、revision、scope、company、data epoch 或 post-read 不确定时返回 structured error/Unknown，不伪造 restore 成功。

## CI-only 验收

`checkpoint_restore_invalidates_approvals_and_runner_context` core source guard 覆盖 checkpoint fields、capture-before-write/input、preview read-only、restore revision/TOCTOU/path/data/company fences、approval/run invalidation 和 EventLog completion；既有 H03/P1-H03/ER checkpoint fixtures 作为回归来源。

```text
cargo fmt --all --check
cargo test -p kiana-core --test p2_k4_01_checkpoint --locked -- --test-threads=1
cargo test -p kiana-core --test p1_h03_path_containment_guard --locked -- --test-threads=1
cargo test -p kiana-core --test er07_replay_guard --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- checkpoint facts/JSONL 与 apply_patch transaction 已接线，但跨进程 durable CheckpointService、ArtifactStore version graph、power-loss atomicity、projector checkpoint 和 backup/restore 仍由 PD/ER/DEP 负责。
- 当前 scope/approval invalidation 是 restore 前的控制面步骤；外部 provider/MCP effect、业务 acceptance、自动 recovery 或 live/physical undo 结果仍需独立 receipt/reconciliation。
- 编辑级 undo 只覆盖纳入快照的文本文件和 mode；未声明路径、外部系统、数据库、生成 artifact 或未知副作用不会被声称可回滚。

