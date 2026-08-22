# Requirements: Kiana

**Defined:** 2026-07-15
**Recut:** 2026-08-22 — v1 requirements are the v0.2 runnable local agent MVP only. The old 104-item 1.0 ledger was deleted.
**Core Value:** Kiana 必须在覆盖 Claude Code 公开核心能力的基础上，更可靠地完成真实长任务，并用可验证证据和可恢复状态证明任务确实完成。
**Current milestone:** v0.2 Runnable Local Agent MVP

## User Stories

1. 作为本地开发者，我在一个已信任的仓库里用一句话描述任务，Kiana 用真实模型和工具把活干完，并在 CLI 或 TUI 里把过程流出来。
2. 作为同一用户，我可以第二天继续上次会话，也可以中途取消；失败时我看得到原因，而不是一个假成功。
3. 作为同一用户，我希望没信任的项目不能被随便改/执行，但我已经信任的项目上黄金路径仍然能用。
4. 作为同一用户，我能打开一张收据，看到这次到底调用了哪些工具、改了哪些文件，而不是只相信模型说“已经完成”。

## Acceptance Vocabulary (Proof Levels)

沿用既有四级，本里程碑默认只声称 `local_behavior`：

| Level | Meaning | Allowed claim |
|-------|---------|----------------|
| `source` | 规划、矩阵、设计、冻结基线 | 不是产品完成 |
| `local_behavior` | 本仓库/本机可观察行为 | v0.2 退出门禁 |
| `target_environment` | 真实目标环境证据 | 本里程碑不声称 |
| `user_value` | 目标用户验收 | 本里程碑不声称 |

`gaps_found`、`sample_only`、测试数量和过期规划表都不是完成。

## v1 Requirements

Requirements for milestone **v0.2**. Each maps to roadmap phases 3–6.

### Golden Path

- [ ] **PATH-01**: 用户可以在已配置的一个真实 provider（Anthropic、OpenAI-compatible 或 Ollama 之一）下，对已信任本地仓库运行 `kiana -p "<task>"` 或 `kiana run [--json] "<task>"`，看到工具调用流和最终结果
- [ ] **PATH-02**: 用户可以用 `kiana tui` 在同一仓库、同一 provider 上完成与 PATH-01 同等的黄金路径（产品 TUI 为 `kiana tui` / `kiana-screens`）
- [ ] **PATH-03**: 用户可以让该黄金路径实际执行仓库内的 read、edit 和 shell（或等价受控执行），并且改动不逃出被信任的项目边界
- [ ] **PATH-04**: 用户在缺少 API key、provider 或项目信任时看到明确失败；命令不得假装成功、空转成功或无限挂起

### Session Continuity

- [ ] **SESS-01**: 用户可以通过 `kiana session list`、`kiana -c` 和 `kiana -r <id>` 找到并继续最近一次黄金路径会话
- [ ] **SESS-02**: 用户可以取消一次进行中的黄金路径 run，并看到“已取消”而不是成功完成；取消后不再继续发起工具调用
- [ ] **SESS-03**: 用户可以在 CLI/TUI/JSON 输出中看到认证失败、provider 错误、权限拒绝或工具错误的原因，而不是空白成功

### Trust and Permissions

- [ ] **TRUST-01**: 用户可以对目标仓库建立 ProjectTrust 后跑通黄金路径；未信任仓库在 edit/shell 之前 fail-closed，并提示如何信任
- [ ] **TRUST-02**: 用户可以使用 permission profile（至少 read-only / workspace / ask 之一）运行同一任务；被拒绝的工具调用可见，read-only 不会写出文件
- [ ] **TRUST-03**: 用户不能通过普通 CLI 开关绕过 ProjectTrust 或硬拒绝；deny 始终优先

### Run Evidence

- [ ] **EVD-01**: 一次黄金路径结束后，用户可以列出本次 run 的工具名称、关键路径/参数和终端状态
- [ ] **EVD-02**: 用户可以列出本次 run 实际改动的文件，或得到明确的“无文件改动”；由此区分模型声称与工作区事实
- [ ] **EVD-03**: 用户在重启进程后仍能打开同一张收据/证据，且它不会被下一次 run 静默覆盖成无法追溯

## v2 Requirements

Deferred. Not in the current roadmap.

### Later product scope

完整 1.0（三包、全入口、云、企业、38-reference）不在本里程碑。旧 104 项账本已删除，不再作为执行依据。

## Out of Scope

Explicitly excluded from **v0.2**. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| 把原 Phase 3–24 横切地基当作本里程碑 | 会再次推迟第一条可跑黄金路径 |
| IDE / Desktop / Web / App Server 产品闭环 | 后续表面里程碑；本里程碑只收 CLI print/run + `kiana tui` |
| Official Cloud / Enterprise | 商业层不能冒充 MVP 完成 |
| Research / Daily pack 深度 | 先证明 coding 黄金路径 |
| 多 provider 能力协商超集（Gemini/OpenRouter 全矩阵） | MVP 只需一个真实 provider，差异必须显式失败而不是假装等价 |
| 38-reference 产品完成或 1.0 措辞 | Phase 1 只完成了 source 治理；不能升级 |
| 签名发布物、完整 SBOM 签名、目标环境验收 | Phase 2 人审门禁，不在 v0.2 关闭 |
| 用编译通过、测试数量或 `legacy_edges_remaining` 变化代替黄金路径 | 不是用户可观察完成 |
| 复制专有源码/品牌 | 既有产品约束 |
| 本里程碑启用 TDD/RED-GREEN 作为执行前置 | 项目规则 `tdd_mode: false` |

## Acceptance Criteria

- PATH 要求必须在真实 provider 下、针对真实本地仓库观察到工具调用，而不是 fake provider 自测冒充用户价值。
- Trust 负面路径（未信任、read-only、deny）必须实际阻断副作用。
- Evidence 必须能在重启后再次打开。
- 任何 VERIFICATION 只能写 `local_behavior`；不得把 local 升成 target_environment 或 1.0。

## Definition of Done

v0.2 完成当且仅当：

1. PATH/SESS/TRUST/EVD 共 13 项都有对应 phase 的实现与 local_behavior 验证
2. 已知限制写进用户文档（单 provider、CLI/TUI、非 1.0）
3. Phase 2 仍保持未关闭，除非另有人审
4. 未把 parked 104 项重新算进本里程碑完成率

## Traceability

Which phases cover which **current** requirements. Parked 1.0 IDs stay in Appendix A and are not mapped to phases 3–6.

| Requirement | Phase | Status |
|-------------|-------|--------|
| COD-01 | Phase 1 | Complete |
| DIF-11 | Phase 1 | Complete |
| DIF-12 | Phase 2 | Pending (human gate; not blocking v0.2) |
| PATH-01 | Phase 3 | Pending |
| PATH-02 | Phase 3 | Pending |
| PATH-03 | Phase 3 | Pending |
| PATH-04 | Phase 3 | Pending |
| SESS-01 | Phase 4 | Pending |
| SESS-02 | Phase 4 | Pending |
| SESS-03 | Phase 4 | Pending |
| TRUST-01 | Phase 5 | Pending |
| TRUST-02 | Phase 5 | Pending |
| TRUST-03 | Phase 5 | Pending |
| EVD-01 | Phase 6 | Pending |
| EVD-02 | Phase 6 | Pending |
| EVD-03 | Phase 6 | Pending |

**Coverage:**
- current milestone v1 requirements: 13 total (PATH 4 + SESS 3 + TRUST 3 + EVD 3)
- historical closed/parked on the live roadmap: COD-01, DIF-11, DIF-12
- mapped to current phases: 13
- unmapped current v1: 0

---
