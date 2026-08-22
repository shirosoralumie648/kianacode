# Roadmap

完整产品阶梯见 `DESIGN.md`。公司编排见 `COMPANY.md`。git 证据见 `PROCESS.md`。逐期剧本见 `PHASES.md`。
历史 GSD Phase 1–24 作废。下面「当前里程碑」才是可执行阶段。

## 版本阶梯（后版本未打开）

- [x] **v0.2 Runnable Local Agent**
- [x] **v0.3 Trusted Workbench + planning/executing + one symposium**
- [ ] **v0.4 Coding pack baseline** ← 当前
- [ ] **v0.5 Five departments + six-layer RAG**
- [ ] **v0.6 Extra surfaces, same core**
- [ ] **v1.0 Personal complete product**
- [ ] **v1.x Team / enterprise**

## 当前里程碑：v0.4

- [x] **Phase 1: Reviewer ≠ author** — `monitoring/reviewer` 新 session；写 `gate/REVIEW.json`
- [ ] **Phase 2: Coding pack matrix draft** — `docs/coding-pack-matrix.md`（先文档）
- [ ] **Later: MCP / skills / provider / readonly tools** — 矩阵签字后再开

### Phase 1: Reviewer ≠ author

**Goal:** Reviewer 与 Builder 不得同一 `session_id`。监控部最小编制出现；门是确定性收据对照，不跑模型。
**Requirements:** REV-01
**Success Criteria:**

1. CLI 是 `kiana run --review <author_session_id>`；不能和 `--symposium` / `--packet` / `--continue` / `--cancel` / `--receipt` 混用；带 prompt → `review_prompt_conflict`。
2. 目录增加 `monitoring/reviewer`；tools 空；sandbox `read-only`；不能 `apply_patch` src。
3. Reviewer 是新 session；与作者相同 → `review_author_session_denied`；作者必须是 Builder；找不到收据 → `review_author_not_found`。
4. 编排器读作者收据，写 `gate/REVIEW.json`（`kiana.review-packet.v1`）；不复制 Builder transcript；不调模型。
5. 证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/9-VERIFICATION.md`。证明级别 `local_behavior`。CLI 是 `kiana run --review`；省略 `--role` 即 reviewer。同核证明在 in-process DaemonHost；跨进程 CLI 靠持久 `session_id` / `run.receipt`。

## 已完成：v0.3

- [x] **Phase 1: Role catalog + policy** — `planning/pm`、`planning/architect`、`executing/builder`；policy 认 role
- [x] **Phase 2: Independent Builder spawn** — packet 是唯一输入；新 session，不复制编排器 transcript
- [x] **Phase 3: One bounded symposium** — PM+Architect，硬顶轮次，产出 DecisionRecord + 一个 WorkPacket
- [x] **Phase 4: Eval + install** — cassette 黄金路径 + 临时 install demo；TUI 保持 park

### Phase 1: Role catalog + policy

**Goal:** RoleSpec/DepartmentSpec 成为派工单位。PM/Architect 不能 `apply_patch` src。默认 `kiana run` 仍是 Builder。
**Requirements:** DEPT-01, ORCH-02, ROLE-01..03
**Success Criteria:**

1. 目录至少有 `planning/pm`、`planning/architect`、`executing/builder`；字段含 tools、sandbox、path_allow、prompt_hash、can_convene。
2. 默认 `kiana run` 收据仍是 `role_id=builder`、`department_id=executing`，且仍能写盘。
3. `--role pm` 对 src `apply_patch` fail-closed，文件不出现。
4. PM 对 `plan/`（或 `charter/` / `packet/`）的 `apply_patch` 可以写盘。
5. 未知 role → `role_unknown`。证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/5-VERIFICATION.md`。证明级别 `local_behavior`。CLI 仍是 `kiana run`；`--role` 可选；部门从目录推断。PM `apply_patch` 只能写 `charter/` `plan/` `packet/`。

### Phase 2: Independent Builder spawn

**Depends on:** Phase 1
**Requirements:** ORCH-01, ORCH-03
**Success Criteria:**

1. 派 Builder = 新 session；不复制编排器 transcript。
2. Work packet 是工人唯一输入。
3. 不把 TeamCreate/SendMessage 接成产品总线。

**Plans:** 已执行。验证：`.planning/phases/6-VERIFICATION.md`。证明级别 `local_behavior`。CLI 是 `kiana run --packet <path>`；packet 不能和 prompt / `--continue` / `--cancel` / `--receipt` 混用。同核证明在 in-process DaemonHost。

### Phase 3: One bounded symposium

**Depends on:** Phase 2
**Requirements:** SYMP-01, SYMP-02, SYMP-03
**Success Criteria:**

1. 一场规划会：PM+Architect，硬顶轮次，私有 session + 黑板。
2. 产出 DecisionRecord + 一个 WorkPacket；Builder 默认不列席。
3. anti-meeting 可跳过开会、直接异步包。

**Plans:** 已执行。验证：`.planning/phases/7-VERIFICATION.md`。证明级别 `local_behavior`。CLI 是 `kiana run --symposium [--anti-meeting]`；不能和 `--packet` / `--continue` / `--cancel` / `--receipt` 混用。Chair 必须是 PM。同核证明在 in-process DaemonHost：私有 session + 黑板，Builder 不列席。

### Phase 4: Eval + install

**Depends on:** Phase 1（可与 2–3 并行准备，但不替代公司内核）
**Requirements:** WB-01, WB-02, WB-03
**Success Criteria:**

1. fixture + cassette：文件出现且收据可指。
2. `install.sh` / release smoke 跑 v0.2/v0.3 demo。
3. TUI 保持 park 或书面再迁。

**Plans:** 已执行。验证：`.planning/phases/8-VERIFICATION.md`。证明级别 `local_behavior`。闸门是 `scripts/harness-golden-smoke.sh` + `scripts/v03-workbench-smoke.sh`（临时 `INSTALL_DIR`）。不是 `scripts/release-smoke.sh`，不是 live provider。

## 已完成：v0.2

- [x] Phase 1: CLI golden path
- [x] Phase 2: Session continue / cancel / visible failure
- [x] Phase 3: Durable receipts
- [x] Phase 4: TUI parked

## 后版本（不要现在 plan/execute）

| 版本 | 打开条件 | 需求前缀 |
|---|---|---|
| v0.4 | v0.3 绿；公开行为矩阵草稿签字 | CODE / REV |
| v0.5 | v0.4 核心路径可用 | LONG / DEPT / SYMP / MEM |
| v0.6 | v0.5 resume 真能用 | SURF2 |
| v1.0 | v0.6 至少 SDK 同核；安装升级过关 | REL |
| v1.x | v1.0 个人产品已发布 | ENT |
