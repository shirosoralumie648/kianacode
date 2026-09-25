# UI-28 共享静态资产、版本和生产打包基线

> 快照日期：2026-09-25。Electron/Node 与 Rust 验收由 GitHub Actions 执行；本步骤不在本地
> 运行测试、build、check、clippy 或 smoke，且不等待 CI 结果。本地只做目标 JavaScript 语法和
> manifest verifier 静态检查。

## Manifest 与版本合同

`contrib/desktop/asset-manifest.json` 使用 `kiana.desktop-assets.v1`，绑定 app version
`0.1.0`、`kiana.protocol.v1`、`kiana.ui.v1`、MIT OR Apache-2.0 license、runtime nonce CSP
模式和 `unsafe-inline`/`unsafe-eval` 禁止项。每个 packaged asset 都有相对路径、字节数、SHA-256
和 cache policy；hash 命名/不可变 package asset 可用一年 immutable cache，动态/metadata 文件
使用 no-store。

`contrib/desktop/lib/asset-manifest.js` 拒绝绝对路径、`..` escape、重复项、symlink、hash/size
漂移、未知字段、非法 schema/version、`.env`/key material 路径和 private-key/常见 live credential
marker。`package.json` 的 Electron `build.files` 必须明确包含 `asset-manifest.json`、main/preload/
welcome 和 `lib/**/*`，并拒绝 secret/token glob；包内不新增 Web bearer 或 provider credential。

## CSP、cache 与构建边界

Web 页面继续由 `kiana-entrypoints` 用 per-process runtime nonce 填充 inline style/script；现有
response CSP 只允许 nonce、self connect 和 object/base/form/frame 限制，不降级为 unsafe inline/eval。
manifest verifier 提供 SHA-256 CSP helper 与 cache policy helper；它只读取 package/source bytes，
不生成 daemon、模型、Broker 或 ControlPlane 执行路径。Electron package 通过同一 allowlist 承载
manifest，生产/开发 URL 和 worker attach 仍由 UI-24/25 的 loopback/sidecar fence 管理。

## CI-only fixture 与限制

`contrib/desktop/tests/fixtures/ui28-assets.json` 与 `ui28_assets.test.js` 覆盖 manifest schema/
unknown/hash/size/duplicate/path/symlink/secret/CSP/package allowlist/cache/SHA-256 cases；
`.github/workflows/ui28-desktop-assets.yml` 运行 Node syntax、`scripts/verify-desktop-assets.js`、
deny-first Node contract、Rust formatting 和 `cargo check -p kiana-entrypoints --tests --locked`。

`feature_status=implemented`; `proof_level=source`。未证明 Electron-builder clean/package/DEB
产物、离线安装/升级/rollback、签名/透明日志、真实 browser cache、OS packaging、source-map发布
策略、跨平台打包或 live/physical 结果；CI 结果保持 pending/unobserved。
