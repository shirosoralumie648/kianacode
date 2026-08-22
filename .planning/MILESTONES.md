# Kiana 发布列车（MILESTONES）

**Created:** 2026-07-26（规划体系增强）  
**Recut:** 2026-08-22 — 当前执行列车改为 v0.2 纵向 MVP，不再把未完成的 24 阶段横切链当作下一趟车。

**规则：** 每趟列车的退出门禁必须引用可观察行为或具名旅程 + proof level。禁止“基本完成”类自由文本。Alpha/Beta/1.0 门禁不降级：每次发布必须写明支持范围、已知限制、迁移风险和数据兼容性。

## 当前列车

| 列车 | 名称 | 覆盖阶段 | 性质 |
|------|------|----------|------|
| **v0.2** | Runnable Local Agent MVP | Phase 3–6 | 当前执行：本地可跑通的 coding agent |
| Phase 1 | 证据治理 | 1 | 已完成（历史） |
| Phase 2 | 可复现工具链 | 2 | 实现已落地；人审门禁并行停放，不阻塞 v0.2 |

## v0.2 — Runnable Local Agent MVP（当前）

**目的：** 先有一个用户能真正跑起来的本地 agent，而不是先铺完契约/状态/RuntimeHost 横切地基。

**形态：** 新的可执行阶段 3–6。建立在已经接线的 `kiana -p` / `kiana run` / `kiana tui`、一个真实 provider、read/edit/shell 和 ProjectTrust 之上，把它们收成一条可演示、可恢复、失败可见、有执行证据的黄金路径。

**退出门禁（全部 @ local_behavior，除非另标）：**

- `PATH-01`–`PATH-04`：用户在已信任仓库上用 `kiana -p` 或 `kiana run`，以及 `kiana tui`，走完一次真实 provider + read/edit/shell 任务；缺 auth/trust/provider 时失败可见。
- `SESS-01`–`SESS-03`：用户可以列出/恢复/继续最近会话，取消进行中的 run，并看到明确失败原因。
- `TRUST-01`–`TRUST-03`：未信任项目在写/执行前 fail-closed；权限档位拒绝可见；黄金路径不能靠 CLI 开关绕过 ProjectTrust。
- `EVD-01`–`EVD-03`：跑完后能检查工具调用和改动文件，并能区分“模型声称完成”与“工具确实执行过”；重启后收据仍在。

**已知限制（必须写进任何 Alpha 声明）：**

- 单真实 provider，不承诺多 provider 能力协商超集
- 表面限于 CLI print/run + 产品 TUI（`kiana tui`）+ 可选 REPL
- 不包含 IDE / Desktop / Web / Cloud / Enterprise
- 不包含 Research / Daily pack 深度
- 不包含 38-reference 产品完成或已签名 `dist/`
- Phase 2 人审/签名/SBOM 缺口保持阻塞，不能用本列车关闭

**下一步：** Coding 深度（MCP/git/browser、更长任务、workflow）、然后 Research/Daily、其他入口、云/企业、1.0 证明。

## 后续列车（north star，非当前执行）

这些曾按原 24 阶段横切链编号。内容仍有效，作为后续里程碑输入，**不是 v0.2 的完成标准**。

| 列车 | 原名称 | 原覆盖 | 现在的位置 |
|------|--------|--------|------------|
| parked-M0 | Walking Skeleton | 原 Phase 3–6 横切最薄链路 | 被 v0.2 纵向黄金路径取代；旧 schema/state/RuntimeHost 工作推迟 |
| parked-M1 | Coding Alpha | 原 Phase 7–10 | v0.3 候选：更长任务、provider 协商、coding 仓库闭环 |
| parked-M2 | Coding Beta + 生态 | 原 Phase 11 + 16 | MCP/自动化/远程/Headless 产品化 |
| parked-M3 | Research/Daily Alpha | 原 Phase 12–15 | 第二、第三能力包 |
| parked-M4 | Surfaces Beta | 原 Phase 17–19 | IDE / Desktop / Web / 跨入口 |
| parked-M5 | Cloud/Enterprise RC | 原 Phase 20–23 | 官方云与企业自托管 |
| parked-M6 | 1.0 | 原 Phase 24 | 仅目标环境 + 用户验收后才允许 1.0 措辞 |

旧退出门禁原文见 git 历史中的本文件（`eae099d` / `bc48332` 之前的 24 阶段列车）。特性账本仍在 `.planning/features/03-FEATURES.md`–`24-FEATURES.md`。被停放的 Phase 3 研究在 `.planning/parked/v1.0-north-star/`。

## 历史记录

### v1.0 横切地基（未完成，2026-07-15 – 2026-08-22）

**计划过但未作为执行主线继续：** 24 阶段 horizontal foundation，M0 Walking Skeleton = 原 Phase 3–6。

**实际落地：**

- Phase 1 完成（2026-07-26）：public-baseline 与 38-reference 证据治理
- Phase 2 实现落地、计划/摘要在 2026-08-22 重建；verification = `human_needed`

**为何停：** 用户要求先有能跑通的 MVP agent，再扩展成通用助手。继续横切会推迟第一条可演示黄金路径。
