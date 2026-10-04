# Goal — 走完 docs/roadmap.md 的全量 Step 队列

## 当前执行目标（2026-10-04）

Ruflo mission：`msn_da9280fe41928026f08c5ef9`（state=`planned`，plan digest=`sha256:abad432d0dbea409b6ed6ea3769a7c64f493b218d021c1a4620e0f6707bcd022`）。

按 `docs/roadmap.md` §1.1 的当前全量队列推进，不把历史候选分支或未观察的 CI 当作完成证据。当前队列登记为 761 个 Step：351 个 ✅、398 个 🔄、12 个 ⏳；每次领取前重新解析当前 `master` 的依赖和状态。第一批活动前沿为 `ER-01`、`H05`、`CO-06`、`OA-02`、`OA-03`、`PD-01`、`SC-07`；`CM-02` 在 `ER-01` 合并后领取。互不写同一边界的活动前沿可使用独立 worktree 并行，合并时逐个审 diff。

硬规则：本地不运行 `cargo test`、`cargo build`、`cargo check`、`cargo clippy` 或项目 smoke；GitHub Actions 是测试/编译权威，推送后不等待结果。每完成一个 Step，使用 `step/<id>-<slug>-<yyyymmdd>` 分支，完成 source slice、证据块和 roadmap 状态回填后独立提交，合并到 `master` 并推送 `origin/master`；不把 `source` 或 CI 触发写成 `local_behavior`、`durable`、`live` 或 `physical`。用户当前指示覆盖本文件及其他文档中的旧限制。


## 队列现状（2026-09-27 盘点）

登记 611 行（历史盘点）：✅ 321 / 🔄 241 / ⏳ 49。当前数字以本页“当前执行目标”和 `docs/roadmap.md` 为准。

| 前缀 | 未收口 | 🔄 | ⏳ |
|---|---|---|---|
| DEP | 42 | 24 | 18 |
| CO  | 40 | 39 | 1 |
| UI  | 38 | 38 | 0 |
| INT | 28 | 28 | 0 |
| EQ  | 25 | 25 | 0 |
| BQ  | 21 | 14 | 7 |
| SC  | 16 | 4 | 12 |
| NM  | 16 | 16 | 0 |
| SW  | 14 | 14 | 0 |
| PD  | 11 | 4 | 7 |
| AUT | 11 | 11 | 0 |
| ER  | 9 | 9 | 0 |
| CAP | 5 | 1 | 4 |
| CM/EXT/CP | 13 | 13 | 0 |
| CI  | 1 | 1 | 0 |

## 阶段

### 历史阶段 0：解 CI（已完成；不再作为本次本地验证门）

- [x] 盘点工作树，发现三件在制品：CI 合并（664 个 workflow → `ci.yml`）、DEP-18 半成品、一批 rustfmt 修复
- [x] 发现 **32 个 tracked `.rs` 未 rustfmt-clean** → `cargo fmt --all --check` 必红，每次 CI 都红
- [x] 跑 `cargo fmt --all` 修掉 32 个文件（`3ee38492`）
- [x] CI 合并（删 664 个、留 41 个、统一 `ci.yml`）+ allowlist 反转规则（`08552ada`）
- [x] DEP-18 事件契约落地（`a3eceba9`）
- [x] 文档锚点归位（`5a0d0d78`）
- [x] **发现 master 长期编译不过**：统一 CI 第一次跑 `cargo check --workspace`，炸出大面积编译错误。
      在干净 worktree 检出 HEAD 复现，确认是既有断裂而非本次引入。
- [x] 逐 crate 修复编译（本地只跑 `cargo check`，不跑任何测试）：
      kiana-domain 68→0、kiana-quality 114→0、kiana-client 3→0、kiana-core 7→0、
      kiana-provider 8→0、kiana-query 5→0、kiana-runner 1→0、kiana-capability-broker 2→0、
      kiana-protocol 3→0、kiana-workflow 1→0、kiana-commands ✅、tools/services/eventlog/tasks/
      screens/modifiers ✅；kiana-daemon 与 kiana-entrypoints 收尾中
- [ ] 完整 `cargo check --workspace --offline` 归零后，**一次性**提交推送（中间态都编译不过，拆开无意义）
- [ ] 推送后按 CI 结果回填 roadmap/CURRENT_STATUS

### 编译修复的根因分类（供后续 step 复用）

| 根因 | 次数 | 典型症状 |
|---|---|---|
| Cargo.toml 漏声明依赖 | 3 | `unresolved import` / `cannot find attribute`，一次报上百个 |
| 给持有 `serde_json::Value` 的类型 derive `Eq` | 5 | `Value` 只有 `PartialEq`；应去掉外层的 `Eq`，不是给 `Value` 加 |
| 调用了从未实现的方法 | 2 | `UiCursor::validate()`、`ArtifactId::as_str()` |
| glob 重名导出 | 3 组 | `WorkspaceFileSnapshot`、`CostLedger`、`MAX_MODEL_ID_BYTES`；**改名，不删** |
| 缺 `Eq`/`Ord` derive（含级联） | 13 处 | 成本链最深三级：`ReceiptCostBreakdown→CostBreakdown→CostBreakdownKind→CostEstimate` |
| 漏 import（常量已存在） | 11 | 先 grep 确认常量在哪，别急着新建 |
| 字符串比较 `String` vs `&String` | 9 | 补一次解引用 `*value` |
| 所有权/生命周期 | 6 | 借出后再用；按语义调整顺序，别用 clone 糊 |
| 真实逻辑 bug | 1 | `ui_store.rs` 清理过期项后仍读该集合 |

**教训**：几百个错误通常只有几类根因。先归因再动手，不要逐个错误修。
另外 `cargo clean` 对这类错误无效（不是陈旧 rmeta）；判断「是不是我搞坏的」用
`git worktree add --detach /tmp/x HEAD` 在干净 HEAD 上复现，比反复 clean 快。

### 阶段 1：按依赖顺序清 ⏳ 队列

**进度（2026-09-28）**：队列 611 行 = ✅ 321 / 🔄 254 / ⏳ 36。已收口 DEP-18..22、DEP-27 验签、
PD-26/27/29/31、BQ-24、SC-37/38/39。

下一批（14 个无阻塞）：SC-32、BQ-25、BQ-26、BQ-27、DEP-23、PD-30、PD-32、PD-34、
CAP-30~33、CO-48。22 个被上游阻塞，随前置完成解锁。

波次顺序：DEP-23（restore activation）→ DEP-24/25/26 → PD-30/32/34（故障与平台矩阵）
→ CAP-30~33（平台适配）→ BQ-25/26/27（账单安全与故障注入）→ SC-32 → CO-48。

**已发现但未解决的设计问题（需后续 step 拍板）**：仓库现存三套传播层词表互不一致——
PD-26 卡片（facts/artifact/memory/index/cache/checkpoint）、ER-29 的
`DataPropagationTarget::ALL`（9 项，不同顺序）、`kiana-core/src/deletion.rs` 的 `DELETION_TARGETS`
（9 项，字母序，含 external/backup 不含 facts）。映射前必须先定哪套是 canonical。

### 阶段 2：清 🔄 队列

241 张 🔄 是「已写源码、待远端验证或后续加固」。每张按卡片退出条件补齐，不能靠 source-only 证据升 ✅。

## 固定节奏

复现 → 分类 → 最小修复 → 审 diff + 证据 → 提交推送 → 回填完成态。
不等 CI。红了下一轮再修。

## 2026-09-28 进展

### 队列现状

`docs/roadmap.md` §1.1 当前登记 761 行：✅ 351 / 🔄 398 / ⏳ 12。前置全为 ✅ 且自身未收口的行，按每次领取前的当前解析结果确定；本页旧的 2026-09-28 数字仅保留为历史记录。
CAP-31/32/33 的剩余项是真实平台行为（Seatbelt / Job Object / runsc），Linux CI 无法验证，
不能诚实收口，保持未完成。

### 本轮完成

| 项 | 内容 | 提交 |
|---|---|---|
| SC-32 | 修正 6 个不可达的负向夹具（错误码不存在 / 游标取值错误 / 长度守卫抢先命中） | `b45df29d` |
| EQ-43 | `quality_promote_fixtures.rs`，17 个拒绝优先夹具，含卡片验收名 | `4e1ba6b4` |
| EQ-45 | `quality_feedback_fixtures.rs`，8 个夹具，钉住 feedback 只引用不改写 | `cc587325` |
| EQ-46 | `quality_drift_fixtures.rs`，11 个夹具，含 `drift_alert_does_not_change_route_or_grant` | `b01e222d` |
| 文档守卫 | `validate-doc-references.sh` + 豁免清单 + 接入 ci.yml | `8cfc6cbf` |
| 更正 | 撤回「NM-04..NM-16 无实现」的错误断言 | `dcf34731` / `5f96b629` |

### 文档引用漂移（已量化）

`08552ada` 合并并删除 704 个 per-step workflow 后，证据块仍在指名它们：718 份文档中 **401 条**
引用无法解析，其中 365 条是被删的 lane。已归档到 `scripts/ci/doc-reference-exemptions.txt`
并加 CI 守卫，清单不得增长。

剩余 14 条源码引用是**写错 crate**（模块存在）：`kiana-core/src/notifications.rs` 实为
`kiana-domain/src/notifications.rs`；`kiana-daemon/src/notification_projector.rs` 实为
`kiana-core/src/notification_projector.rs`；`kiana-daemon/src/health.rs` 实为
`kiana-core/src/health.rs`。4 条无同名对应，20 条是散文示例文件名。

**教训**：basename 索引必须取自 `git ls-files`。`.claude/worktrees/` 下的陈旧 checkout 里有
从未合入 master 的同名文件，用文件遍历会让守卫把引用误判为已解析——这正是守卫要防的缺陷。

### 错误码纪律

本轮先烧掉一次提交：SC-32 夹具断言了源码里不存在的错误码和取不到的游标。之后的每个夹具
都机械校验过：把测试里所有 snake_case 字面量与全仓源码比对，未命中的必须解释。
并行 worker 也必须独立复核，不能只信报告。

## 提交约定

- 一个 Step 一个提交，message `step: add <ID> <主题>`。
- 显式 `git add <路径>`，**绝不 `git add -A`**。
- `docs/roadmap.md` 只由本会话维护，同行一并提交。
- 分支 `step/<id>-<slug>-<yyyymmdd>`，合回 master 后再推。
