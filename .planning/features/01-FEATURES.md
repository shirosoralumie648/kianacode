# Phase 01 特性账本 — 现状基线与证据治理（已完成，回填）

**Created:** 2026-07-26（回填） | **父需求：** COD-01, DIF-11 | **主旅程：** coding.md §A（parity 索引权威）
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。本阶段 2026-07-26 完成，本文件为回填记录，验收引用真实产出。

### FEAT-01-01 — Claude Code public-parity ledger（冻结基线）
- 父需求: COD-01
- 领域旅程: coding.md §A 全部（journey.install-auth 等 15 条）
- 描述: 用户可查看带冻结日期的 CC 公开对齐账本，每条能力有 proof level、gap 与差异决策。
- 验收:
  - [source] 15 journeys / 49 capabilities 冻结于版本化不可变 revision，来源字节 hash 锚定 ✓
  - [source] 每条 capability 带 required_proof_level/target_phase/coverage_state/difference ✓
- Verifier: governance selector 校验 + 01-VERIFICATION.md
- 当前基线: implemented — `docs/agent-program/kiana-completion/governance/public-baselines/cc-public-2026-07-15.json`
- 设计引用: DESIGN-INDEX T2 governance/
- 依赖: None
- 状态: complete

### FEAT-01-02 — 38-reference 治理矩阵（Adopt/Adapt/Reject）
- 父需求: DIF-11
- 领域旅程: core.release-lifecycle（治理输入面）
- 描述: 维护者可检查 38/38 reference 的 live source、license、决策、owner、test、risk 与 evidence。
- 验收:
  - [source] repository-registry + capability-decisions + evidence 四 family 全量、fail-closed、可离线重算 ✓
  - [source] 每项拒绝含理由；license 不兼容项拒绝代码复用 ✓
- Verifier: freeze/check-drift/refresh 工具链 + 01-VERIFICATION.md
- 当前基线: implemented — governance 四 family + `scripts/freeze-capability-governance.py`
- 设计引用: DESIGN-INDEX T2 governance/、reference_audit/
- 依赖: FEAT-01-01
- 状态: complete

### FEAT-01-03 — 治理监督器与进度诚实性报告
- 父需求: DIF-11
- 领域旅程: core.release-lifecycle
- 描述: 进度报告区分 source/local/target/user-value proof，模块/stub/测试数量不能报告为产品完成。
- 验收:
  - [local_behavior] Rust supervisor 对 public-baseline 与 reference-governance 切片产出 receipt，离线、限时、脱敏 ✓
- Verifier: supervisor receipts + 01-14 gap-closure（fresh-process < 25 s 双样本）
- 当前基线: implemented — kiana-capability-governance-supervisor crate
- 设计引用: DESIGN-INDEX T1 capability-governance-supervisor-design
- 依赖: FEAT-01-01, FEAT-01-02
- 状态: complete
