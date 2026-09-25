# UI-32 deny-first 安全路径集成基线

> 快照日期：2026-09-25。验证只在 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI 结果。

## Deny matrix

`kiana-entrypoints/tests/fixtures/ui32-deny-first.json` 固定入口拒绝：foreign session、bad
token/origin、foreign Electron sender/channel、expired/revoked approval、stale cursor/revision、
missing action envelope、payload digest/scope widening、unknown IPC envelope、IDE dot path、cancel race with unknown effect、indirect injection。每一项带稳定
error code、`effect_count=0` 和 `requery_original`/`safe_retry`/`do_not_retry`/`reconcile` follow-up；
Unknown 不改写为成功，且不盲目 retry。

`ui32_deny_first.rs` 只读取 fixture 与入口源边界，检查 Web action claim/owner/auth mutation、
CLI/Workbench cancel/parity 以及 DaemonHost/ControlPlane/EventLog/result_unknown 共享主路径。它不
替代真实 runtime integration；其目的是让 GitHub CI 在入口代码变化时先拒绝 source/fixture 漂移。

## CI-only 证据与限制

`.github/workflows/ui32-deny-first.yml` 运行 Rust formatting、串行 UI-32 fixture/source guard 和
workspace test-target compile。后续真实 HTTP/browser/Electron/PTY、effect counter、EventLog append、
approval race、provider/connector、跨进程恢复和 physical/live proof 仍需 UI-33、UI-38 及 ER/PD/SC
门禁；CI 结果保持 pending/unobserved。Workflow path filter 同时包含当前 CM-36
`kiana-domain/src/memory_workbench.rs`，fresh remote run 会覆盖 repository-wide fmt dependency。

`feature_status=implemented`; `proof_level=source`。
