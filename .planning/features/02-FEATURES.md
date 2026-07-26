# Phase 02 特性账本 — 可复现工具链与依赖收敛

**Created:** 2026-07-26 | **父需求：** DIF-12 | **主旅程：** core.release-lifecycle
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。FEAT 与 `02-CONTEXT.md` 决策 D-01..D-14 对齐；执行中不得静默改验收。

### FEAT-02-01 — Rust toolchain 固定与升级策略（D-01..D-04）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: 发布维护者可以从固定 toolchain 频道复现构建输入，升级策略成文可查。
- 验收:
  - [local_behavior] `rust-toolchain.toml`（stable 频道 + rustfmt/clippy/rust-src）生效，本地与 CI 读同一文件
  - [local_behavior] 升级策略写入 toml 注释 + docs/ 维护文档；CI 改用 `dtolnay/rust-toolchain@master`
- Verifier: 命令证据（rustup show、CI run）
- 当前基线: none — 仓库尚无 rust-toolchain.toml；CI 用 @stable
- 设计引用: DESIGN-INDEX Phase 2 行；02-CONTEXT.md canonical_refs
- 依赖: None
- 状态: pending

### FEAT-02-02 — 构建输入可复现证明（D-05..D-07）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: 任何人用 toolchain 版本 + Cargo.lock + 源码 hash 三个输入得到相同编译产物集合，CI 自动记录。
- 验收:
  - [local_behavior] `cargo fetch --locked` 通过；`dist/build-inputs.json` 固定 schema（toolchain_channel/version、cargo_lock_hash、source_hash、build_timestamp、ci_run_id）
  - [target_environment] CI job 生成该文件并写入 job summary；Cargo.lock 变更经 PR diff 可见
- Verifier: build-inputs.json + CI run 链接
- 当前基线: none — 无 build-inputs 合同
- 设计引用: 02-CONTEXT.md specifics
- 依赖: FEAT-02-01
- 状态: pending

### FEAT-02-03 — 双受众发布就绪报告（D-08..D-10）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: 维护者看完整技术细节，用户/管理员看脱敏摘要；全维度显示，缺失项标"需要操作"。
- 验收:
  - [local_behavior] 同一报告命令支持 `--json`/`--md`；用户版只显示 toolchain/dependencies/signing/sbom/license/platform/acceptance 七类的通过/阻塞/需要操作状态并脱敏路径主机名
  - [local_behavior] 全维度不静默：缺失维度显式 `需要操作`
- Verifier: 报告样例 + schema 校验
- 当前基线: partial — `commercial-release-blockers-report.sh` 已有维护者完整视图与 handoff；无脱敏用户版
- 设计引用: DESIGN-INDEX T2 commercial-release-readiness；kiana-commercial-release-blockers.v1.schema
- 依赖: None
- 状态: pending

### FEAT-02-04 — 发布签名链接入（D-11）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: 本地签名工具链 + Sigstore/cosign 生成满足现有 blocker 检查的签名证明。
- 验收:
  - [local_behavior] 生成 `dist/proofs/signing/release-signature.json` 满足 `kiana-release-signature.v1` schema；无效签名被拒
- Verifier: `release-signature-verification-smoke.sh`
- 当前基线: partial — 签名合同/验证 smoke 已实现；无 cosign 接入与真实签名器
- 设计引用: DESIGN-INDEX T2；02-CONTEXT.md specifics（ruflo 签名参考）
- 依赖: None
- 状态: pending

### FEAT-02-05 — SBOM 签名与脱敏用户版（D-12）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: SBOM 本身进入签名链，并生成去内部路径、保留包名/版本/license 的用户可读版本。
- 验收:
  - [local_behavior] `dist/sbom.cdx.json` 被同一签名链签名；脱敏版无内部路径且 schema 有效
- Verifier: 签名验证 + 脱敏断言
- 当前基线: partial — SBOM 生成已有（generate-sbom.sh）；无签名与脱敏版
- 设计引用: DESIGN-INDEX T2 compliance
- 依赖: FEAT-02-04
- 状态: pending

### FEAT-02-06 — License 合规摘要（D-13）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: 用户可直接审查的独立 license 摘要，与 compliance-audit 完整报告并存。
- 验收:
  - [local_behavior] 可导出 JSON 与 Markdown；覆盖全部依赖 license 与例外说明
- Verifier: 导出样例 + compliance-audit 交叉核对
- 当前基线: partial — compliance-audit.sh/deny.toml 已有完整报告；无用户摘要
- 设计引用: DESIGN-INDEX T2
- 依赖: None
- 状态: pending

### FEAT-02-07 — Acceptance readiness 证据可见性（D-14）
- 父需求: DIF-12
- 领域旅程: core.release-lifecycle
- 描述: 目标环境测试与用户验收两类证据可见可追踪；缺证据能力保持阻塞，不能标 complete/production-ready/1.0。
- 验收:
  - [local_behavior] 报告展示两类证据的存在/缺失与指向；缺失时对应能力状态为 blocked
- Verifier: 报告断言 + proof-templates 状态检查
- 当前基线: partial — product-acceptance/entitlement/release-ops proof 合同已有；未纳入统一可见性报告
- 设计引用: DESIGN-INDEX T2 proof-templates
- 依赖: FEAT-02-03
- 状态: pending
