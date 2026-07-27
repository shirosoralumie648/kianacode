# Phase 24 特性账本 — 1.0 全量收敛与发布证明

**Created:** 2026-07-26 | **父需求：** CORE-16 | **主旅程：** core.release-lifecycle
**规则：** 见 docs/superpowers/specs/2026-07-26-kiana-planning-system-enhancement-design.md §4。只有全部 local/external blockers 归零并取得目标用户/云 owner/企业 owner 验收后才可标记 1.0。

### FEAT-24-01 — 全平台发布物生命周期证明
- 父需求: CORE-16
- 领域旅程: core.release-lifecycle
- 描述: Linux/macOS 原生与 Windows/WSL 用户可以安装、升级、回滚、迁移和卸载正式 artifact，失败后恢复到已验证版本。
- 验收:
  - [target_environment] 三平台安装/升级/回滚/迁移/卸载全链在真实目标环境通过；失败恢复验证
- Verifier: 平台生命周期证据（真实环境截图/日志）
- 当前基线: partial — 本地发布机制与 lifecycle smoke 完备；真实目标环境全链缺失
- 设计引用: DESIGN-INDEX Phase 24 行（project_os 36；commercial-release-readiness）
- 依赖: FEAT-19-03
- 状态: pending

### FEAT-24-02 — 签名/SBOM/provenance 全量证明
- 父需求: CORE-16
- 领域旅程: core.release-lifecycle
- 描述: 每个发布物有可验证 signature、checksum、SBOM、license/dependency scan、provenance、release notes、doctor 与脱敏 support bundle，所有声明链接到 release proof。
- 验收:
  - [target_environment] 签名/SBOM/provenance 全链（本地机制已有）在真实 CI/CD release workflow 执行；distribution channels（GitHub Releases/Homebrew/winget/enterprise bundle）全部非 blocked
- Verifier: commercial-release-blockers → local_blocking=0 + external_blocking=0
- 当前基线: partial — 本地签名/SBOM/manifest/blocker 合同完备；remote/signing/channel 全部为 Still Blocking（commercial-release-readiness 清单）
- 设计引用: commercial-release-readiness；FEAT-02-04/05/06
- 依赖: FEAT-02-04, FEAT-24-01
- 状态: pending

### FEAT-24-03 — 黄金旅程目标环境证据汇总
- 父需求: CORE-16
- 领域旅程: core.release-lifecycle
- 描述: Coding/Research/Daily 三 pack、全部产品入口、Official Cloud 与 Enterprise 的黄金旅程、跨入口/跨租户负面测试、故障注入和 backup restore 均以目标环境证据通过。
- 验收:
  - [user_value] 所有 journeys/ 下的旅程（core/coding/research/daily/surfaces/cloud/enterprise）取得其 required_proof_level 证据；无旅程仍在 gap 状态
- Verifier: 旅程证据汇总表（所有旅程 gap=closed）
- 当前基线: partial — 大量旅程仍为 proof_level=source；各 pack 大量旅程基线 none/partial
- 设计引用: .planning/journeys/ 七份账本
- 依赖: FEAT-19-05, FEAT-21-04, FEAT-23-04
- 状态: pending

### FEAT-24-04 — Blockers/P0P1 归零与三方验收
- 父需求: CORE-16
- 领域旅程: core.release-lifecycle
- 描述: local/external blockers 与 P0/P1 均为零；目标用户、cloud owner 与 enterprise owner 完成验收后版本才可标记 1.0/complete/production-ready。
- 验收:
  - [user_value] `kiana release blockers --json` local_blocking=0 且 external_blocking=0；三方验收签字记录
- Verifier: blocker 报告 + 验收签字文件
- 当前基线: none — 外部 blocker 清单在 commercial-release-readiness "Still Blocking" 节
- 设计引用: commercial-release-readiness；proof-templates；MILESTONES M6
- 依赖: FEAT-24-01, FEAT-24-02, FEAT-24-03
- 状态: pending
