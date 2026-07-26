# Phase 2: 可复现工具链与依赖收敛 - Context

**Gathered:** 2026-07-26
**Status:** Ready for planning

<domain>
## Phase Boundary

本阶段建立 Kiana 的可复现构建基础和透明发布就绪报告体系：锁定 Rust toolchain 版本策略、定义构建输入可复现的证明方式、扩展 `commercial-release-blockers-report.sh` 使 local/external blockers、平台、签名、SBOM、license 和 acceptance readiness 对维护者（完整技术视图）和用户/管理员（脱敏摘要）均可查看和导出。

本阶段满足 `DIF-12`。不修复具体功能缺口，不实现产品能力层，不替代目标环境或用户验收证据——只让"缺少证据"的阻塞状态变得可见和可操作。

</domain>

<decisions>
## Implementation Decisions

### Rust Toolchain 固定策略

- **D-01:** 新建 `rust-toolchain.toml`，`channel = "stable"`（不锁定具体版本号）。升级策略：每次只升一个 point 版本，发布后至少等几周再升，遵循 grok-build 模式。
- **D-02:** components = `["rustfmt", "clippy", "rust-src"]`（完整套件，支持 rust-analyzer）。
- **D-03:** 升级策略写入 `rust-toolchain.toml` 注释 + `docs/` 独立维护文档（两者均维护）。
- **D-04:** CI `release-smoke.yml` 将 `dtolnay/rust-toolchain@stable` 替换为 `dtolnay/rust-toolchain@master`，后者自动读取 `rust-toolchain.toml` 中的频道。

### 可复现构建的范围与证明

- **D-05:** "可复现"定义为**构建输入可复现**：toolchain 版本 + `Cargo.lock` + 源码 hash 三者固定，任何人用这三个输入能得到相同的编译产物集合。不要求 bit-for-bit 二进制一致。
- **D-06:** 验证由 **CI 自动执行**：`cargo fetch --locked` 成功 + 记录实际 toolchain 版本 + 源码 hash → 写入 `dist/build-inputs.json`（固定 schema）+ CI job summary。
- **D-07:** `Cargo.lock` 发生变更时 PR diff 自动可见（利用现有 git diff），作为漂移告警机制，不需要额外脚本。

### Blocker / 发布就绪报告

- **D-08:** 报告面向**两个受众**，内容不同：
  - **维护者**：完整技术细节（当前 `commercial-release-blockers-report.sh` 输出）
  - **用户/管理员**：脱敏摘要（路径/主机名脱敏，隐藏未公开能力的具体差距，只展示 blocker 数量和类别）
- **D-09:** 导出格式：`--json` 和 `--md` 两种，同一报告命令支持两个 flag。
- **D-10:** 报告**全维度显示**（local blockers、external blockers、平台矩阵、signing、SBOM、license、acceptance readiness）。缺失项不静默：标记为 `需要操作`，明确暴露空洞。

### SBOM、许可证和签名链

- **D-11:** 发布签名采用**本地签名工具链 + Sigstore/cosign 接入**，生成 `dist/proofs/signing/release-signature.json`（满足现有 `signing.release-artifacts` blocker 检查，`kiana-release-signature.v1.schema.json` 已定义）。
- **D-12:** SBOM（`dist/sbom.cdx.json`）需要：（1）对 SBOM 本身用同一签名链签名；（2）生成面向用户的脱敏版本（去掉内部路径，保留包名/版本/license）。
- **D-13:** 新建**独立 license 合规摘要**：可导出为 JSON 或 Markdown，供用户直接审查，与 `compliance-audit.sh` 的完整报告并存。
- **D-14:** Acceptance readiness 记录两类证据：（1）目标环境测试通过记录；（2）用户验收记录。Phase 2 使这些证据类型**可见和可追踪**，不替代实际的证据收集（实际收集是后续 phase 的任务）；缺少证据时，相关能力保持阻塞状态，不能被标记为 complete/production-ready/1.0。

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### 现有签名与合规基础设施
- `scripts/commercial-release-blockers-report.sh` — 现有 blocker 报告脚本，Phase 2 在此基础上扩展
- `scripts/compliance-audit.sh` — 现有合规审计脚本，生成 SBOM 并执行 license 检查
- `scripts/generate-sbom.sh` — CycloneDX JSON SBOM 生成脚本
- `docs/schemas/kiana-release-signature.v1.schema.json` — 发布签名证明契约（已定义）
- `docs/schemas/kiana-commercial-release-blockers.v1.schema.json` — 发布 blocker 报告 schema
- `.github/workflows/release-smoke.yml` — 现有 CI release smoke，Phase 2 需要修改 toolchain Action

### 发布就绪文档
- `docs/commercial-release-readiness.md` — 现有发布就绪门禁文档（参考 signing/blocker 格式）

### 参考项目 toolchain 模式
- `reference/grok-build/rust-toolchain.toml` — stable 频道 + 手动升级策略注释（D-01 采用的模式）
- `reference/codex/codex-rs/rust-toolchain.toml` — 精确版本锁定参考
- `reference/ECC/ecc2/rust-toolchain.toml` — 次要版本锁定参考

</canonical_refs>

<specifics>
## Specific Ideas

- `dist/build-inputs.json` 的 schema 应包含：`toolchain_channel`、`toolchain_version`（CI 运行时记录实际版本）、`cargo_lock_hash`、`source_hash`、`build_timestamp`、`ci_run_id`
- 用户脱敏摘要的 blocker 类别包括：`toolchain`、`dependencies`、`signing`、`sbom`、`license`、`platform`、`acceptance` — 只显示每类的通过/阻塞/需要操作状态，不展示具体技术细节
- Sigstore/cosign 接入参考 `reference/ruflo/scripts/smoke-neural-trader-backtest-signing.mjs` 了解已有使用模式

</specifics>

<deferred>
## Deferred Ideas

- bit-for-bit 二进制可复现（需要 `RUSTFLAGS=-Zremap-path-prefix` 等复杂配置）— 推迟到确认有需求
- 自动依赖升级机器人（Dependabot/Renovate 接入）— 推迟，Phase 2 只做固定和记录
- 多区域分发签名 — 推迟到 cloud 发布 phase

</deferred>

---

*Phase: 02-reproducible-toolchain-dependency-convergence*
*Context gathered: 2026-07-26*
