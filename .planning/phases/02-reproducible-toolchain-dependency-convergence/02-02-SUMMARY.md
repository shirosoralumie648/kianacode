---
phase: 02-reproducible-toolchain-dependency-convergence
plan: "02"
status: complete
commits:
  - 0eaaa9e  (Wave 2)
  - 05cc81a  (Wave 3)
duration: ~40min
tasks_completed: 6
files_changed: 7
---

# Plan 02-02 Summary (Waves 2 & 3)

## Wave 2 — CI build-inputs 生成 + blocker 报告扩展

### CI 构建输入记录步骤
- `.github/workflows/release-smoke.yml` 新增 `Record build inputs` 步骤：在 `cargo fetch --locked` 之后，记录实际 `rustc` 版本、`Cargo.lock` SHA-256、Git HEAD 至 `dist/build-inputs.json`（schema: `kiana.build-inputs.v1`）。
- 使用 `rustc --version | awk '{print $2}'` 捕获实际解析版本（可靠 fallback，不依赖 action 输出名）。

### blocker 报告新增三项 build-test 检查
- `toolchain.rust-toolchain-file`：验证 `rust-toolchain.toml` 存在且含 `channel` 字段 → 现状：**satisfied**
- `toolchain.build-inputs`：验证 `dist/build-inputs.json` 存在且含7 required 字段 → 现状：**blocking**（需要 CI 运行生成）
- `sbom.present`：验证 `dist/sbom.cdx.json` 存在且非空 → 现状：**blocking**（需要运行 generate-sbom.sh）
- Total checks 升至 **20**

### --md 双受众输出（D-08..D-10）
- bash arg parser 新增 `--md` 和 `--audience=maintainer|user` 标志
- Python 新增 `render_user_markdown()` 函数：category 级别计数，无路径/主机名/详细信息
- `--md` 默认 maintainer（完整 render_handoff_markdown）；`--md --audience=user` 产出脱敏摘要
- 验证：`bash scripts/commercial-release-blockers-report.sh --md` 和 `--md --audience=user` 均正常

## Wave 3 — SBOM 签名 + license 摘要 + 新检查

### scripts/sign-sbom.sh（D-12）
- 产出 `dist/sbom.cdx.user.json`：bom-ref 中 `path+file://` 替换为 `{name}@{version}`，本地 `externalReferences` 移除
- 可选：`KIANA_SIGNING_COMMAND` 存在时对 `dist/sbom.cdx.json` 签名，产出 `dist/sbom.cdx.json.sig`
- 无签名命令时明确说明并以 0 退出

### scripts/generate-license-summary.sh（D-13）
- 支持 `--json`/`--md`/`--out`/`--md-out` 四种输出模式
- 从 `cargo metadata --locked --offline` 枚举全部 520 个包，按名称排序，workspace/registry 分类
- schema: `kiana.license-compliance-summary.v1`（已添加至 `docs/schemas/`）
- 验证：`--json` 输出解析正确，520 包，0 个 UNKNOWN license

### blocker 报告新增两项检查（总计 22 项）
- `sbom.signed`：`dist/sbom.cdx.json.sig` 是否存在 → blocking（需要 signing command）
- `license.compliance-summary`：`dist/license-summary.json` 是否存在 → blocking（需要运行脚本）

## Phase 2 成功标准核对

| # | 成功标准 | 状态 |
|---|----------|------|
| 1 | 发布维护者可以从固定工具链、lockfile、依赖策略复现同一构建输入 | **local_behavior 满足**：rust-toolchain.toml + build-inputs.json schema + CI 步骤 |
| 2 | 用户/管理员可查看并导出 blockers、签名、SBOM、license、acceptance readiness | **local_behavior 满足**：22 项检查、--md/--audience=user、license summary、sign-sbom |
| 3 | 缺少目标环境或用户证据的能力保持阻塞 | **满足**：toolchain.build-inputs、sbom.signed、license.compliance-summary 均为 blocking |

## 仍在阻塞的检查（预期）

Phase 2 不处理外部阻塞，以下按设计保持 blocking：
- `toolchain.build-inputs`：需要真实 CI 运行生成 dist/build-inputs.json
- `sbom.signed`：需要配置并运行签名命令
- `license.compliance-summary`：需要在构建流程中运行 generate-license-summary.sh
- 外部检查（source.remote、signing.release-artifacts 等）：Phase 24 处理

## What Comes Next

**Phase 3: 契约与 Schema 基线**
