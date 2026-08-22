# Roadmap: Kiana

## Overview

当前执行里程碑是 **v0.2 Runnable Local Agent MVP**：在已有 `kiana` 二进制上收出一条纵向可跑通的本地 coding agent 黄金路径，而不是继续铺原 Phase 3–24 的横切地基。

Phase 1 证据治理已经完成。Phase 2 工具链实现已落地，但人审/签名/`dist/` 门禁仍开着，**不阻塞** Phase 3。Phase 3–6 把 CLI/TUI、一个真实 provider、read/edit/shell、trust fail-closed、可见失败和执行证据收成用户能演示的切片。完整 1.0（三包、全入口、云、企业、38-reference）不是本里程碑范围。旧 24 阶段设计文档已删除。

## Phases

**Phase Numbering:**

- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions marked as INSERTED
- Phases 1–2 are historical from the previous train. Phases 3–6 are the current v0.2 MVP.

- [x] **Phase 1: 现状基线与证据治理** - 冻结公开行为和 38-reference 的可审计基线。 (completed 2026-07-26)
- [ ] **Phase 2: 可复现工具链与依赖收敛** - 让构建输入和发布就绪状态可复现、可检查。（人审门禁仍开，不阻塞 v0.2）
- [ ] **Phase 3: 黄金路径能跑通** - 用户在已信任仓库上用 CLI/TUI 完成一次真实 provider + read/edit/shell 任务。
- [ ] **Phase 4: 会话可恢复、可取消、失败可见** - 黄金路径可以继续、取消，并且失败原因明确。
- [ ] **Phase 5: 信任与权限让 MVP 能用且 fail-closed** - 未信任或被拒绝的写/执行不会静默发生。
- [ ] **Phase 6: 任务确实执行过的证据** - 用户能区分“模型声称完成”和“工具确实跑过”。

## Phase Details

### Phase 1: 现状基线与证据治理

**Goal**: 用户和维护者可以用冻结日期、来源和证据判断公开能力与 reference 覆盖，而不是依赖功能数量或乐观描述。
**Depends on**: Nothing (first phase)
**Requirements**: COD-01, DIF-11
**Success Criteria** (what must be TRUE):

  1. 用户可以查看带冻结日期的 Claude Code public-parity ledger，并为每个公开旅程找到验证结果或明确差异决策。
  2. 维护者可以检查 38/38 reference 的 live source、license、Adopt/Adapt/Reject、owner、test、risk 与 evidence，且任何拒绝都有理由。
  3. 进度与审计报告能区分 source、local、target 和 user-value proof，不会把模块、stub、mock 或测试数量报告成产品完成。

**Plans**: 14/14 plans complete

- [x] 01-13-PLAN.md
- [x] 01-14-PLAN.md — *gap_closure: close CR-01 — achieve reliable fresh-process production headroom (both samples < 25 s)*

**Wave 1**

- [x] 01-01-PLAN.md
- [x] 01-02-PLAN.md

**Wave 2** *(blocked on Wave 1 completion)*

- [x] 01-03-PLAN.md
- [x] 01-04-PLAN.md
- [x] 01-05-PLAN.md

**Wave 3** *(blocked on Wave 2 completion)*

- [x] 01-06-PLAN.md

**Wave 4** *(blocked on Wave 3 completion)*

- [x] 01-07-PLAN.md

**Wave 5** *(blocked on Wave 4 completion)*

- [x] 01-08-PLAN.md

**Wave 6** *(blocked on Wave 5 completion)*

- [x] 01-09-PLAN.md

**Wave 7** *(blocked on Wave 6 completion)*

- [x] 01-10-PLAN.md

**Wave 8** *(blocked on Wave 7 completion)*

- [x] 01-11-PLAN.md

**Wave 9** *(blocked on Wave 8 completion)*

- [x] 01-12-PLAN.md

### Phase 2: 可复现工具链与依赖收敛

**Goal**: 用户和发布维护者可以复现构建并透明判断产品离发布就绪还缺什么。
**Depends on**: Phase 1
**Requirements**: DIF-12
**Success Criteria** (what must be TRUE):

  1. 发布维护者可以从固定的工具链、lockfile 和依赖策略复现同一构建输入，并解释依赖或工具版本差异。
  2. 用户和管理员可以查看并导出 local/external blockers、平台、签名、SBOM、license 与 acceptance readiness，敏感信息保持脱敏。
  3. 缺少目标环境或用户证据的能力会保持阻塞状态，不能被标记为 complete、production-ready 或 1.0。

**Plans**:

- [x] `02-01-PLAN.md` — Wave 1 toolchain file, build-inputs schema, CI `@master` (reconstructed 2026-08-22 from `c0bd383` / `02-01-SUMMARY.md`)
- [x] `02-02-PLAN.md` — Waves 2-3 CI build-inputs, dual-audience blockers, SBOM user export, license summary (reconstructed 2026-08-22 from `0eaaa9e`+`05cc81a` / `02-02-SUMMARY.md`)

Verification: `02-VERIFICATION.md` separates local_behavior from CI/signing evidence. The Phase 2 checkbox stays unchecked until `human_verify_mode: end-of-phase`. v0.2 MVP 不把该人审门禁当作 Phase 3 的前置条件。

### Phase 3: 黄金路径能跑通

**Goal**: 用户在一个已信任的本地仓库上给出任务，Kiana 用一个真实 provider 和 read/edit/shell 完成一次可见的 agent 回合。
**Depends on**: Phase 1
**Does not wait for**: Phase 2 human closeout / signed `dist/`
**Requirements**: PATH-01, PATH-02, PATH-03, PATH-04
**Success Criteria** (what must be TRUE):

  1. 用户可以在已配置的一个真实 provider 下运行 `kiana -p "<task>"` 或 `kiana run [--json] "<task>"`，看到工具调用流和最终结果，而不是只看到编译/测试通过。
  2. 用户可以用同一仓库、同一 provider 在 `kiana tui` 里完成同等黄金路径（产品 TUI 是 `kiana tui` / `kiana-screens`，不是独立 `kiana-tui` 应用冒充产品 UI）。
  3. 该任务路径实际调用仓库内的 read、edit 和 shell（或等价受控执行），并且改动落在被信任的项目边界内。
  4. 缺少 API key、provider、或项目信任时，命令以明确错误退出或在 TUI 显示明确失败，不得假装成功、空转或无限挂起。

**Plans**: TBD — 先 `$gsd-discuss-phase 3`，不要预先写 `03-*-PLAN.md`
**UI hint**: yes
**Intended slug**: `03-golden-path-runnable-agent`

### Phase 4: 会话可恢复、可取消、失败可见

**Goal**: 黄金路径不是一次性射击：用户可以继续上次工作、停掉正在跑的任务，并读懂失败。
**Depends on**: Phase 3
**Requirements**: SESS-01, SESS-02, SESS-03
**Success Criteria** (what must be TRUE):

  1. 用户可以用 `kiana session list`、`kiana -c` 和 `kiana -r <id>` 找到并继续最近一次黄金路径会话，恢复后仍能看到先前任务上下文。
  2. 用户可以取消一次进行中的 `kiana -p` / `kiana run` / TUI 回合；取消后进程停止继续发起工具调用，界面显示已取消而不是成功完成。
  3. 认证失败、provider 错误、权限拒绝、工具错误会以可读原因出现在 CLI/TUI/JSON 输出中，不得被截断成空白成功。

**Plans**: TBD
**Intended slug**: `04-session-resume-cancel-visible-failure`

### Phase 5: 信任与权限让 MVP 能用且 fail-closed

**Goal**: 黄金路径在已信任仓库上能干活，但未信任或被拒绝的写/执行不会发生。
**Depends on**: Phase 3
**Requirements**: TRUST-01, TRUST-02, TRUST-03
**Success Criteria** (what must be TRUE):

  1. 用户可以对目标仓库建立 ProjectTrust 后跑通黄金路径；未信任仓库在 edit/shell 之前 fail-closed，并告诉用户如何信任。
  2. 用户可以选择 `--permission-profile`（至少 read-only / workspace / ask 之一）跑同一任务；被拒绝的工具调用可见，read-only 不会写出文件。
  3. 黄金路径不能通过普通 CLI 开关绕过 ProjectTrust 或硬拒绝；deny 始终优先。

**Plans**: TBD
**UI hint**: yes
**Intended slug**: `05-trust-permissions-mvp`

### Phase 6: 任务确实执行过的证据

**Goal**: 用户能证明这次任务真正跑过，而不是只拿到一段模型自称完成的文字。
**Depends on**: Phase 3, Phase 4
**Requirements**: EVD-01, EVD-02, EVD-03
**Success Criteria** (what must be TRUE):

  1. 一次黄金路径结束后，用户可以列出本次 run 的工具名称、关键参数/路径和终端状态。
  2. 用户可以列出本次 run 实际改动的文件（含“无文件改动”的明确陈述），从而区分“模型声称已改”和“工作区确实变化”。
  3. 用户在重启进程后仍能打开同一收据/证据；收据不会被下一次 run 静默覆盖成无法追溯的状态。

**Plans**: TBD
**Intended slug**: `06-run-evidence-receipt`

## Milestone Index

| 里程碑 | 名称 | 覆盖阶段 | 性质 |
|--------|------|----------|------|
| Phase 1 | 证据治理 | 1 | 已完成 |
| Phase 2 | 可复现工具链 | 2 | 停放在人审门禁，不阻塞 v0.2 |
| **v0.2** | Runnable Local Agent MVP | 3–6 | **当前执行** |
| later | Coding 深度 / 其他包 / 入口 / 商业 / 1.0 | 未规划 | 以后再说 |

列车说明见 [MILESTONES.md](MILESTONES.md)。当前规划说明见 [`docs/planning-current.md`](../docs/planning-current.md)。

## Progress

**Execution Order:**
Phase 1 is closed. Phase 2 stays open at the human gate and does not block Phase 3. Execute 3 → 4 → 5 → 6 for v0.2. `skip_discuss: false`, so Phase 3 starts with `$gsd-discuss-phase 3`, not a prewritten plan file.

| Phase | Plans Complete | Status | Completed | Milestone |
|-------|----------------|--------|-----------|-----------|
| 1. 现状基线与证据治理 | 14/14 | Complete | 2026-07-26 | historical |
| 2. 可复现工具链与依赖收敛 | 2/2 | Implementation landed; verification open | - | parked human gate |
| 3. 黄金路径能跑通 | 尚未规划 | Not started | - | v0.2 |
| 4. 会话可恢复、可取消、失败可见 | 尚未规划 | Not started | - | v0.2 |
| 5. 信任与权限让 MVP 能用且 fail-closed | 尚未规划 | Not started | - | v0.2 |
| 6. 任务确实执行过的证据 | 尚未规划 | Not started | - | v0.2 |
