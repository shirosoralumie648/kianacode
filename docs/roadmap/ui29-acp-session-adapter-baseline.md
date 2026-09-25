# UI-29 ACP/IDE session adapter 基线

> 快照日期：2026-09-25。Rust 验收由 GitHub Actions 执行；本步骤不在本地运行测试、build、
> check、clippy 或 smoke，且不等待 CI 结果。

## 协议与 session ownership

`kiana-client/src/acp.rs` 定义 `kiana.acp-initialize.v1`、`acp.v1`/`acp.v2`、session reference
和 permission DTO。initialize 只协商 bounded capabilities，响应明确 `direct_effect=false`；
session reference 绑定 server-owned owner digest、session ID、authority epoch 和 feed sequence。
`new_session`/`resume_session` 只在 owner、session、epoch 匹配时成功，未知 protocol、foreign
owner、旧 epoch 和没有安装 feed handler 的 replay/resume 都拒绝。

## UI contract mapping

`AcpProjection` 只允许三种既有协议对象：`UiSnapshotV1`、`UiFeedFrameV1`、`UiActionV1`，并调用
各自的 server/client contract validation。prompt、permission decision 和 cancel 生成带
`command_id`、idempotency key、expected epoch/cursor、payload digest 的 `UiActionV1`；它们是
ControlPlane intent，不是本地执行请求。permission 保存 bounded pending reference，过期或重复
决定 fail-closed。

feed update 先检查 frame schema、instance/epoch/cursor，再按 sequence 接受、标记 replay 或返回
gap；terminal 后的 late update 返回 `acp_update_after_terminal`。disconnect 只变成 Degraded 并
卸载 handler，不提交 cancel；显式 resume 要求重新安装 handler 并返回 `snapshot_required=true`。

## CI-only fixture 与限制

`kiana-client/tests/fixtures/ui29-acp.json` 与 `ui29_acp_adapter.rs` 覆盖协议协商、session owner/
epoch、typed prompt/permission/cancel、permission expiry、listener-before-replay、duplicate/gap/
terminal/late update 和 no-direct-effect source boundary。`.github/workflows/ui29-acp-session.yml`
运行 `cargo fmt --all --check`、聚焦 adapter fixture 和 workspace test-target compile。

`feature_status=implemented`; `proof_level=source`。未接入真实 ACP/IDE socket、editor/terminal
host、live session transport、durable cross-process session lease、真实 provider/Broker effect、
crash/reconnect physical timing 或 live/physical proof；CI 结果保持 pending/unobserved。
