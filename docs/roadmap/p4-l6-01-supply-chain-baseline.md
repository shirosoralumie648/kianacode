# P4-L6-01 扩展供应链基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Install and upgrade verification

`ExtensionRegistry::verify` 在任何安装/升级/回滚状态事实之前执行严格
`ExtensionManifest::validate`，随后用 daemon 启动时配置的 publisher/key allowlist 验证
Ed25519 签名，再按规范化路径和大小上限重算 package content hash。签名覆盖 canonical
unsigned manifest，故 publisher、license、effect、required capabilities、network policy、
requires、迁移和回滚引用的改写都会使签名失效；未经信任的 key、错误算法、坏签名、hash
漂移或缺失引用均 fail-closed。

通过验证的包才会写入不可变本地缓存；真正的安装/升级/revoke/rollback 仍需 operator、
风险审批、owner/project scope、registry version CAS 和 idempotency key，并追加
`extension.lifecycle` EventLog。Receipt 记录 package/content hash、signature_verified、
license、capability diff、migration/rollback reference 和前一版本；缓存写入或包下载本身
不能使扩展 active。Rollback 只能选择 EventLog 中已有且未撤销的 package snapshot，不能
凭路径或模型参数恢复任意文件。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-daemon --lib extension_signature_is_verified_before_install --locked -- --test-threads=1
cargo test -p kiana-domain --test p4_l6_01_supply_chain --locked -- --test-threads=1
cargo test -p kiana-core --test p4_l6_01_supply_chain --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- 当前信任根来自本地 daemon 环境配置，尚无生产 KMS/透明日志、签名分发、SBOM/advisory
  quarantine 或跨进程 registry store；这些留 EXT-20..31、SC-25..30、DEP-39。
- Migration 只读取并校验 stateless JSON，不执行脚本；非 Skill component 仍为 staged，
  不声称扩展安装已完成业务能力验证或外部/live/physical 效果。
