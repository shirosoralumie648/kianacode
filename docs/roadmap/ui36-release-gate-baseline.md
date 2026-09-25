# UI-36 生产构建、安装和发布前 smoke 基线

> 快照日期：2026-09-25。该步骤的构建/发布前 gate 只允许 GitHub Actions 执行；本地不运行
> 测试、build、check、clippy 或 smoke，且不等待 CI 结果。

## CI-only release gate

`scripts/verify-ui36-release-gate.sh` 在非 GitHub Actions 环境主动返回 `remote_ci_required`，避免
本地工作树被误报成发布证明。GitHub job 依次验证 UI-28 desktop asset manifest、`cargo fmt --all
--check`、锁定依赖下 `cargo build --bin kiana --locked`，再扫描 `contrib/desktop`/`dist`/scripts
中的 private-key、常见 live credential 和 bearer marker。构建步骤只产生 artifact/build evidence，
不改变 ProjectTrust、session、approval、runner 或 ControlPlane 状态。

Workflow 仍保持 loopback/sidecar/typed client 边界；安装、upgrade/rollback、空目录、权限不足、
端口占用、daemon crash 和真实 Electron attach 的实机 smoke 需要后续 CI/UAT 夹具，不能从一次
`cargo build` 推导出安装/生产/live/physical 成功。

## 证据与限制

`.github/workflows/ui36-release-gate.yml` 是本步骤唯一运行入口。`feature_status=implemented`、
`proof_level=source`；未证明实际 Electron-builder/DEB 安装、离线升级/回滚、artifact signing/
provenance、真实桌面/OS、provider/Broker/external effect、跨平台 packaging 或 live/physical
结果；CI 结果保持 pending/unobserved。
