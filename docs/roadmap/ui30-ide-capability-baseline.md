# UI-30 IDE editor/terminal capability boundary 基线

> 快照日期：2026-09-25。Rust 验收由 GitHub Actions 执行；本步骤不在本地运行测试、build、
> check、clippy 或 smoke，且不等待 CI 结果。

## Capability 与路径合同

`kiana-client/src/ide_capability.rs` 将 editor/terminal host 收敛为显示/输入 adapter。能力矩阵
复用 `UiHostCapability`，所有 advertised row 都是 `direct_effect=false`、`delegated_to_kiana=true`。
请求 schema 绑定 operation、workspace digest、relative path、path digest、expected revision、
idempotency key 和可选 server Artifact reference；绝对路径、`..`、空 component、NUL、过大 path
和 scope digest 漂移在产生 intent 前拒绝。

只读 open/read/patch preview、terminal output 只返回 bounded query intent；terminal output 有
256 KiB 上限。apply/cancel 使用 `IdeCapabilityPermit`，必须精确匹配 operation/workspace/path/
revision/owner，permit digest 合法、未过期且 `effect_allowed=true`；否则返回稳定拒绝，不生成
UiAction。成功 action 仍只有 `UiActionV1`，由 ControlPlane 再做 policy/approval/permit/CAS；adapter
不接文件系统、shell、terminal spawn、Broker 或 provider。

## CI-only fixture 与限制

`kiana-client/tests/fixtures/ui30-ide-capability.json` 与 `ui30_ide_capability.rs` 覆盖 capability
matrix、relative path、scope/revision/path digest、permit owner/expiry/effect、apply/cancel action、
output bound 和 source no-spawn boundary。`.github/workflows/ui30-ide-capability.yml` 运行 Rust
format、聚焦 adapter fixture 与 workspace test-target compile。

`feature_status=implemented`; `proof_level=source`。未接入真实 IDE/ACP host、filesystem artifact
reader、terminal process、ControlPlane permit mint/consume、durable TOCTOU/revision store、真实
provider/Broker/external effect、crash/reconnect physical timing 或 live/physical proof；CI 结果保持
pending/unobserved。
