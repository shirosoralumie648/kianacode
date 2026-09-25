# UI-37 用户文档、模块图和操作 runbook 基线

> 快照日期：2026-09-25。文档链接/status guard 由 GitHub Actions 执行；本步骤不在本地运行测试、
> build、check、clippy 或 smoke，且不等待 CI 结果。

## Deliverables

- [`docs/ui-entrypoints-runbook.md`](../ui-entrypoints-runbook.md)：真实 CLI/TTY/Web/Desktop/ACP-IDE
  命令和当前边界、失败/恢复/Unknown、trust/sandbox/token/origin、proof-level 和限制。
- [`docs/module-map.md`](../module-map.md) UI-37 section：入口到 versioned client/protocol、DaemonHost、
  ControlPlane、Harness/Broker、EventLog、Receipt/UiSnapshot/Feed/Artifact projection 的唯一调用图。
- `scripts/verify-ui37-docs.sh`：检查文件/link anchors、DaemonHost/ControlPlane/Unknown/source proof
  口径和禁止 overclaim；不运行产品命令、不改变事实、不生成 evidence 通过记录。

文档明确：UI-26–36 source/CI slices 不是 local_behavior/durable/live/physical；ACP/IDE 仅 fake/source
adapter；Desktop metadata/attach/notification 不等于生产安装或 OS 物理效果；任何未知结果先查原
command/Receipt 或 reconcile。

`feature_status=implemented`; `proof_level=source`。未证明干净 checkout 的完整跨入口 runtime、真实
browser/PTY/Electron/ACP、durable recovery、provider/connector/live/physical 和 release UAT；CI guard
结果保持 pending/unobserved。

The guard rejects any `proof_level=durable`, `proof_level=live` or `proof_level=physical` token in the
runbook, rather than relying on the claim appearing beside `feature_status` on the same line.
