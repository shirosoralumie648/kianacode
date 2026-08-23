# Roadmap

完整产品阶梯见 `DESIGN.md`。公司编排见 `COMPANY.md`。git 证据见 `PROCESS.md`。逐期剧本见 `PHASES.md`。
历史 GSD Phase 1–24 作废。下面「当前里程碑」才是可执行阶段。

## 版本阶梯（后版本未打开）

- [x] **v0.2 Runnable Local Agent**
- [x] **v0.3 Trusted Workbench + planning/executing + one symposium**
- [x] **v0.4 Coding pack baseline** — P1-READ skipped (shell search)
- [x] **v0.5 Five departments + six-layer RAG**
- [x] **v0.6 Parallel builders, same core** — path locks; SDK/IDE/worktree remain later
- [ ] **v1.0 Personal complete product** ← 当前（Phase 1 已绿；REL-03 closeout 未开）
- [ ] **v1.x Team / enterprise**

## 当前里程碑：v1.0

v0.6 打开条件已满足（ORCH-04）。v1.0 是安装/文档/P0 核心路径，不是企业、不是 TUI 像素对等、不是 SDK/IDE 新入口。JointSymposium 仍冻结。

- [x] **Phase 1: Personal install lifecycle + USER.md** — REL-01, REL-02

### Phase 1: Personal install lifecycle + USER.md

**Goal:** 临时 `INSTALL_DIR` 能安装/升级/回滚/恢复/卸载；`USER.md` 写真实可跑命令。
**Requirements:** REL-01, REL-02 (thin)
**Success Criteria:**

1. install → `--version` 可用。
2. 再装同一二进制 → checksum 不变。
3. 拷回备份 → checksum 回到安装时。
4. 删掉再装 → 又可用。
5. uninstall 二进制不在；再 uninstall 幂等。
6. `USER.md` 写 trust / run / symposium / packet / review。
7. 不是 packaged tarball / `~/.local/bin` / REL-03 再做工具。证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/19-VERIFICATION.md`。闸门：`scripts/v10-personal-lifecycle-smoke.sh`。

## 已完成：v0.6

- [x] **Phase 1: Parallel builders + path locks** — ORCH-04

### Phase 1: Parallel builders + path locks

**Goal:** 同一 `DaemonHost` 上两个 packet Builder 独立 session 并行写盘；活锁重叠 fail-closed；packet `path_allow` 交集 apply_patch。
**Requirements:** ORCH-04
**Success Criteria:**

1. 两个不交叠 `path_allow` 的 spawn 都写出文件；两个 `session_id`；两个 `work_packet_id`。
2. 活路径锁重叠（含前缀）→ `path_lock_conflict`；空 `path_allow` 锁 `*`；锁在 spawn 返回时释放。
3. packet `path_allow` 之外的 apply_patch → `packet_path_denied`，不写盘。
4. 顺序空 packet 仍可 spawn。默认 Builder 仍写 `GOLDEN_PATH.txt`。
5. 不是 worktree / Integrator / Queen / 新 CLI / SDK/IDE。证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/18-VERIFICATION.md`。同核证明在 in-process DaemonHost。

## 已完成：v0.5

- [x] **Phase 1: Five department objects** — initiating/planning/executing/monitoring/closing
- [x] **Phase 2: Six-layer RAG ACL** — JSONL 分库 + `memory.search`/`write`
- [x] **Phase 3: User-visible compact + resume-after-compact** — LONG-02
- [x] **Phase 4: Department symposiums** — SYMP-04

### Phase 1: Five department objects

**Goal:** 五个 PMP 过程组同时作为 `DepartmentSpec` 控制面对象存在。默认工人仍是执行部 Builder。
**Requirements:** DEPT-02
**Success Criteria:**

1. Catalog 含 `initiating` / `planning` / `executing` / `monitoring` / `closing`；字段含 mission、artifacts、gates、rag_collection 名。
2. 默认 run 仍是 `role_id=builder`、`department_id=executing`，仍能写 `GOLDEN_PATH.txt`。
3. `sponsor` 只能写 `charter/`；`closer` 只能写 `lessons/`；src 都 `role_path_denied`。
4. 不招满 COMPANY.md 每部门全部角色。不实现 `memory.search`。不格式化 `cli.rs`。
5. 证明级别 `local_behavior`。P1-READ skipped。

**Plans:** 已执行。验证：`.planning/phases/14-VERIFICATION.md`。同核证明在 in-process DaemonHost。

### Phase 2: Six-layer RAG ACL

**Goal:** Company/Department/Role/Project/User/Instance scratch 分库存在；`memory.search`/`write` 走 daemon broker，按 knowledge_grants 过滤；命中进收据。
**Requirements:** MEM-01, MEM-02, MEM-03, MEM-04
**Success Criteria:**

1. 六层 JSONL 分库，不混库。project/department/role/scratch 在项目内；company/user 在 `$KIANA_HOME`。
2. 模型工具是 `memory.search` / `memory.write`；执行在 daemon broker。请求带 `role_id` + `department_id`。
3. 命中写入收据 `memory_hits`；无来源 `verified=false`。instance scratch 默认不晋升。
4. Builder 不能读 `user-private` 或 `planning:unreleased-debate`；不能写 project 记忆。
5. 不把 `kiana memory` CLI / 向量库 / `rag_collection` 名当成完成。不格式化 `cli.rs`。证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/15-VERIFICATION.md`。同核证明在 in-process DaemonHost。

### Phase 3: User-visible compact + resume-after-compact

**Goal:** 超预算历史 compact 对用户可感知；compact 之后同一 DaemonHost 仍可 continue 并写盘。
**Requirements:** LONG-02
**Success Criteria:**

1. 超预算 run 的收据含 `compact.applied=true`、token 计数、`summary_present=true`。
2. 未超预算 run 不得声称 compact（`applied=false`, `count=0`）。
3. compact 之后 same-host continue + workspace-write 仍能写出 `GOLDEN_PATH.txt`。
4. pause 仍是现有 cancel。不新增 CLI 开关，不格式化 `cli.rs`。
5. 不是 `/compact` slash、不是 `kiana-query` 引擎、不是跨进程 transcript 恢复。证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/16-VERIFICATION.md`。同核证明在 in-process DaemonHost。

### Phase 4: Department symposiums

**Goal:** Initiating / Planning / Executing / Monitoring / Closing 都能开本部门有界会。主席是该部门 `can_convene` 角色，不是永远 PM。
**Requirements:** SYMP-04
**Success Criteria:**

1. 五部门都能开会；规划会仍是 PM+Architect，Builder 不列席，anti-meeting 仍写 `plan/DECISION.json` + `packet/TASK.json`。
2. 非规划会只写该部门决策文件，不产 Builder WorkPacket：initiating → `charter/DECISION.json`；executing → `receipt/DECISION.json`；monitoring → `gate/DECISION.json`；closing → `lessons/DECISION.json`。
3. Builder 可 convene 执行部；Architect 主席仍 `symposium_chair_must_be_pm`；跨部门出席 → `joint_symposium_frozen`。
4. 决议不自动 `memory.write`。不新增 CLI 开关；CLI `--symposium` 仍只许 PM。不格式化 `cli.rs`。
5. 不是 JointSymposium、不是招满角色、不是 Librarian。证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/17-VERIFICATION.md`。同核证明在 in-process DaemonHost。

## 已完成：v0.4

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

### Phase 2: Coding pack matrix draft

**Goal:** 对 Claude Code **公开**核心行为做审计表；先文档后代码。dump 工具目录名不是 P0。
**Requirements:** CODE-01
**Success Criteria:**

1. `docs/coding-pack-matrix.md` 存在。
2. 列齐全：公开行为、公开来源、Kiana 现状、owner、测试/证据、许可证、是否本期。
3. P0 是核心路径（写盘/trust/session/收据/失败可见/Reviewer + 文档化的 MCP/skills/hooks/provider 缺口），不是 50 个 dump 工具。
4. 冻结项写明：TeamCreate/SendMessage、TUI 迁核、五部门/RAG、Desktop/Web、chrome/computer-use、拆 `cli.rs`。
5. 下一实现站写死为 CODE-02（MCP client 经 daemon）。本 Phase 不写 MCP 代码。

**Plans:** 已执行。验证：`.planning/phases/10-VERIFICATION.md`。证明级别 `local_behavior`（文档）。矩阵是签字草稿；v0.4.2 起才允许实现 CODE-02。

### Phase 3: MCP client through daemon

**Goal:** 受信 Builder 经 `DaemonHost` 调一个本地 stdio MCP。模型只见 `mcp`；执行是 `mcp.call`。未信任 deny；Reviewer/PM 不能调。
**Requirements:** CODE-02（PATH-03 扩展为 `shell` + `apply_patch` + `mcp`）
**Success Criteria:**

1. 配置是 `KIANA_MCP_SERVERS_JSON`；不新增 `kiana run` 开关；不格式化 `cli.rs`。
2. 只 stdio。HTTP / SSE / WS → `mcp_transport_unsupported`。
3. 未信任 → `project_untrusted`。角色无 `mcp` → `role_tool_denied`。Safe/read-only → Ask。trusted + Balanced + Builder → Allow。
4. 禁止把 `kiana-tools/mcp_tool.rs` 或 `kiana-entrypoints/src/mcp.rs`（Kiana 当 MCP server）标成完成。
5. 证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/11-VERIFICATION.md`。证明级别 `local_behavior`。同核证明在 in-process DaemonHost。HTTP 仍 fail-closed。

### Phase 4: Skills / hooks on harness

**Goal:** 受信 Builder 在 harness 第一条 System 消息里看见一条项目 Skill；未信任项目 Skill 不出现；PreToolUse 能在 broker execute 前拦住副作用。
**Requirements:** CODE-03（PATH-03 仍是 `shell` + `apply_patch` + `mcp`）
**Success Criteria:**

1. Skills 是上下文，不是第四个模型工具。Daemon 用 `kiana-skills::load_all_skills_with_trust` 注入 `Start.instructions`。
2. 用户 / `KIANA_HOME/skills` / bundled 未信任仍可见。项目 `.claude/skills` 与 `.kiana/skills` 必须 Trusted。
3. 一个能拦的 hook：PreToolUse。policy Allow 之后、broker execute 之前。`Block` → `hook_blocked:...`；Ask → `hook_ask_unattended`。配置复用现有 hook env。不新增 `kiana run` 开关；不格式化 `cli.rs`。
4. 禁止把 `kiana-tools` SkillTool 或全套 post/stop/session hook 标成完成。
5. 证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/12-VERIFICATION.md`。证明级别 `local_behavior`。同核证明在 in-process DaemonHost。

### Phase 5: Provider degrade on harness

**Goal:** 不支持 tools 的 provider profile 必须机器可读失败，禁止假成功、禁止写盘。
**Requirements:** CODE-04
**Success Criteria:**

1. 产品证明走 in-process `DaemonHost::with_env_harness()`；不新增 `kiana run` 开关；不格式化 `cli.rs`。
2. `ProviderModelClient` 把 `UnsupportedCapability` 映射成 `error.code()`（`unsupported_tools`）。
3. `KIANA_PROVIDER=fake` + `KIANA_FAKE_MODEL=fake-text-only`：run `Failed`，error 含 `unsupported_tools`，`GOLDEN_PATH.txt` 不出现。
4. 禁止把 live Anthropic/OpenAI/Ollama 或 `unsupported_streaming` 标成完成。Harness 仍发 `stream: Some(false)`。
5. 证明级别 `local_behavior`。

**Plans:** 已执行。验证：`.planning/phases/13-VERIFICATION.md`。证明级别 `local_behavior`。同核证明在 in-process DaemonHost。下一刀仅当矩阵仍需要 P1-READ，否则 v0.5。

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
| v0.6 | v0.5 resume 真能用 | ORCH-04 并行 Builder；SURF2 SDK/IDE 后开 |
| v1.0 | v0.6 并行 Builder 同核已绿；安装升级过关 | REL |
| v1.x | v1.0 个人产品已发布 | ENT |
