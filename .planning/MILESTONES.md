# Kiana 发布列车（MILESTONES）

**Created:** 2026-07-26（规划体系增强）
**规则：** 发布列车叠加在 24 阶段依赖链之上，不改变阶段编号与依赖。每趟列车的退出门禁只引用具名旅程（`.planning/journeys/`）与 proof level（见 REQUIREMENTS.md 验收词汇表），禁止"基本完成"类自由文本。Alpha/Beta 门禁不降级（产品总纲 §11.1）：每次发布必须写明支持范围、已知限制、迁移风险和数据兼容性。

## 列车总览

| 列车 | 名称 | 覆盖阶段 | 性质 |
|------|------|----------|------|
| M0 | Walking Skeleton | 3-6（横切最薄链路） | 内部可演示 |
| M1 | Coding Alpha | 7-10 | 限定用户 Alpha |
| M2 | Coding Beta + 生态 | 11 + 16 | 公开 Beta |
| M3 | Research/Daily Alpha | 12-15 | 能力包 Alpha |
| M4 | Surfaces Beta | 17-19 | 全入口 Beta |
| M5 | Cloud/Enterprise RC | 20-23 | 商业 RC |
| M6 | 1.0 | 24 | 正式发布 |

## M0 — Walking Skeleton（内部可演示地基）

**目的：** 把第一个纵向可演示成果从 Phase 10 提前到 Phase 6 附近，证明地基贯通，暴露契约错误。
**形态：** 不是新 phase。Phase 3-6 的特性账本中标记 `[M0]` 的最薄子集 + `core.walking-skeleton` 旅程。
**退出门禁：**

- `core.walking-skeleton` @ local_behavior：一个真实请求走完 typed event → session 持久化 → policy decision → 单工具执行 → typed result，全程可从 EventLog 重放；CLI 单入口演示。
- `core.event-replay` @ local_behavior：重启后重放同一事件序列得到一致投影。

**已知限制声明模板：** 单 provider（fake/Anthropic 任一）、单工具、无 pack 深度、无恢复语义承诺。

## M1 — Coding Alpha（限定用户）

**目的：** 第一个真实用户价值：单仓理解 + 安全编辑 + 验证闭环。
**前提阶段：** Phase 7（provider）、8（workflow/evidence）、9（受限多 Agent 与扩展）、10（Coding 公开基线）达标。
**退出门禁：**

- `coding.repo-onboarding` @ local_behavior、`coding.safe-edit-undo` @ local_behavior、`coding.verify-loop` @ local_behavior、`coding.review-findings` @ local_behavior。
- governance parity 中 target_phase ≤ 10 的 capability 全部达到其 required_proof_level 或记录显式差异决策。
- `core.workflow-evidence` @ local_behavior + `core.recovery` @ local_behavior（DIF-01/02 的 Alpha 承诺面）。
- NFR-01 初始性能预算生效（fresh-process 冷启动 < 25 s 回归门禁）。

**已知限制声明模板：** CLI/TUI 单入口、Linux 优先、无 IDE/Desktop、生态（MCP 全传输/Chrome/CI）在 M2。

## M2 — Coding Beta + 生态（公开 Beta）

**前提阶段：** Phase 11（生态/自动化/远程）、16（Terminal/Headless/MCP 产品闭环）达标。
**退出门禁：**

- `coding.ecosystem-automation` @ target_environment（MCP 传输互操作、Chrome/CI 回执）。
- `surfaces.cli-tui` @ local_behavior、`surfaces.headless-sdk` @ local_behavior、`surfaces.mcp-interop` @ local_behavior。
- governance parity 中 target_phase ≤ 16 的 capability 全部达标或记录显式差异（含解除 `cc.cli.forward-subagent-text` 的版本冲突 block）。
- NFR-02 可观测性基线 @ local_behavior；NFR-04 竞品导入 @ local_behavior。

## M3 — Research/Daily Alpha

**前提阶段：** Phase 12-15 达标。
**退出门禁：**

- `research.evidence-graph` @ local_behavior、`research.experiment-run` @ local_behavior、`research.verifier` @ local_behavior。
- `daily.approval-inbox` @ local_behavior、`daily.connector-center` @ target_environment（至少 calendar+email 两类真实 connector）、`daily.verifier-receipts` @ target_environment。
- DIF-03 result_unknown 语义在 Daily 外部写入路径 @ target_environment。

## M4 — Surfaces Beta（全入口）

**前提阶段：** Phase 17-19 达标。
**退出门禁：**

- `surfaces.ide` @ user_value、`surfaces.desktop` @ user_value、`surfaces.web-appserver` @ local_behavior、`surfaces.cross-surface-continuity` @ target_environment。
- `surfaces.platform-matrix` @ target_environment（Linux/macOS 原生 + Windows/WSL 生命周期）。
- NFR-03 i18n、NFR-05 可访问性、NFR-06 打磨质量门 @ 各自要求等级。
- governance parity 全部 49 capability 达标或显式差异决策（此后 parity 只做 drift 维护）。

## M5 — Cloud/Enterprise RC

**前提阶段：** Phase 20-23 达标。
**退出门禁：**

- `cloud.encrypted-sync` @ target_environment、`cloud.remote-worker` @ target_environment、`cloud.tenant-isolation` @ target_environment（跨租户负面测试）、`cloud.billing-entitlement` @ target_environment。
- `enterprise.install-airgap` @ target_environment、`enterprise.rbac-policy` @ target_environment、`enterprise.audit` @ target_environment、`enterprise.data-dr` @ target_environment（演练证据）。

## M6 — 1.0

Phase 24 的成功标准即 M6 门禁（ROADMAP 为准，MILESTONES 仅索引）：全平台发布物生命周期、签名/SBOM/provenance、全域黄金旅程目标环境证据、local/external blockers 与 P0/P1 归零、目标用户/云 owner/企业 owner 验收。

---
*每趟列车发布时，在本文件追加该次发布的：版本号、日期、门禁核对表（逐条旅程+proof 链接到 evidence）、已知限制声明。*
