# Goal — 走完 docs/roadmap.md 的全量 Step 队列

## 当前执行策略（2026-10-16，本轮用户授权；本页最新授权，覆盖下方全部历史策略）

本轮用户指示：按 `docs/roadmap.md` §1.1 逐个 Step 完成整个项目；**本地不运行任何测试**
（不跑 `cargo test/build/check/clippy`、smoke 或验收脚本），全部验证交给 GitHub Actions；
**每完成一个 Step 就合并并推送一次**，不等待 CI 结果；硬依赖满足且写入范围互不冲突的
Step 使用独立 worktree 并行。文件中与本次指示冲突的旧版限制性要求（包括 2026-10-06 的
“本地完整测试”授权与历史上“必须等待 CI 回执才能推进”的串行做法）按用户指示不适用于本轮。

执行模型：

1. 从 §1.1 顶部取“所有前置 ✅ 且自身未收口”的活动前沿（`python3 scripts/roadmap-goal.py`）。
2. 逐卡实现源码/夹具/守卫；**不做本地编译或测试**，提交信息 `step: <ID> <主题>`。
3. 每个 Step 一个提交：在 `step/<id>-<slug>-<yyyymmdd>` 分支提交，合回 `master`（`--no-ff`），
   推 `master` 与分支；CI 触发后立即转向下一个可并行 Step，不回读、不等待。
4. 状态回填：源码完成即更新 §1.1 行与 `CURRENT_STATUS.md` 证据块，evidence 记
   `CI pending`；未拿到绿色 CI 回执的一律不得写成 `durable/live/physical`，也不得把
   `local_behavior` 写成已验证。CI 红时按回执另开修复提交，不回滚本策略。
5. 并行：写入范围互不冲突的 Step 各自独立 worktree；`Cargo.toml`/`Cargo.lock`/`package.json`
   仍只由集成负责人修改。

本轮起点为 `master` 上的 carry-forward 提交（2026-10-13 WIP + 2026-10-04 goal 的 16 个未合并
Step 提交）；并行 worktree 基线以集成提交的哈希为准。可跟踪视图：
`python3 scripts/roadmap-goal.py --checklist`。

## 历史执行策略（2026-10-10，已被本轮覆盖）

本轮按用户要求重新创建 Codex active goal，持续完成全量 roadmap、完整测试、
控制 CPU/内存，并行推进独立工作；与这些要求冲突的旧版限制不适用。
源码基线为 `d02bcd5a308ecbe468212f90070cf6f07b8b8296` 加已有未提交 WIP，
开始时保存了二进制 diff 和工作树清单，保留已有成果。

当前可重算队列为 749 个 Step：354 个 ✅、395 个 🔄，52 个直接依赖就绪，
21 个完整依赖闭包就绪；98 个 ✅ 仍有开放前置。先处理最早的
`P0-A-01a` 所需完整回归，并修复阻塞完整测试的 `control_plane` 停滞。
索引完成数不代表本轮全量验证通过。

机器内存 7.5 GiB、16 个逻辑 CPU；历史单进程编译峰值约 4 GiB，EventLog
曾超过 5 GiB。因此只运行一个 Cargo 进程，`CARGO_BUILD_JOBS=1`、
`RUST_TEST_THREADS=1`、`RUST_MIN_STACK=33554432`，测试显式串行；使用
无调试信息、去符号、非增量的 test profile，采样可用内存、进程 RSS/CPU 和
系统 swap 计数。独立 agent 在分离 worktree 修复源码，编译测试统一串行执行。

本轮临时证据目录为 `/tmp/kiana-roadmap-20261010-evidence`；它不是持久证据仓库，
验收结果须在本账本和 `CURRENT_STATUS.md` 回填。本轮未授权自动提交、推送、
合并或发布；本地回执不替代远程 CI，未通过的卡片保持开放。

## 历史执行策略（2026-10-06，已被本轮覆盖）

本轮已创建 Codex active goal：按 `docs/roadmap.md` 逐步完成项目、完整测试、控制
CPU/内存，并行推进独立步骤。本轮用户明确要求完整测试，因此下文历史会话中的
“本地不运行测试/build/check/clippy/smoke”不适用于本轮；本地实际回执与远程 CI
回执分别记录，不互相冒充。历史授权提交推送不扩展为本轮自动提交、推送或合并授权。

本轮基线为 `c9b8360fd87a5d3809da84cf73b8ba27247b85b6` 加现有未提交启动修复。
重新盘点为 749 个 Step，354 个 ✅、395 个 🔄、52 个索引 ready；其中 98 个 ✅
仍有未收口前置，必须逐项核验依赖与证据，不能按索引声称当前全量通过。
`scripts/roadmap-goal.py` 现在检查依赖环，并单独报告这些完成项的开放前置。

执行时只启动一个 Cargo 编译进程，初始 `CARGO_BUILD_JOBS=1`、
`RUST_TEST_THREADS=1`、`RUST_MIN_STACK=33554432`；daemon/control-plane
测试显式 `--test-threads=1`。单 job 不限制链接器内部线程，仍需监控整个进程树。
独立源码审计并行；源码写者使用独立 worktree，集成负责人维护共享状态账本。
测试日志、精确退出码与资源回执写入独立证据目录，失败先复现分类，再最小修复。
本轮证据目录：`/tmp/kiana-roadmap-20261005-evidence`；执行中的命令不记为通过。

## 历史执行目标记录（2026-10-04；本轮执行策略以上方 2026-10-06 授权为准）

Goal ID：`goal-kiana-roadmap-completion-20261004`。Ruflo 持久任务：
`task-1791102524181-0iivvl`。目标是完成 `docs/roadmap.md` §1.1 的全部 Step，
包括实现、回归、GitHub CI 验收和证据收口；当前状态为 **执行中**。

本次源码基线为 `38cf305c839ece27595a2f28bacdd1fbdab36559`。该次盘点历史值为：
**749 个 Step，351 个 ✅、398 个 🔄，51 个前置均已标 ✅ 的活动前沿**。
本页旧记录中的“761 / 12 个 ⏳”与当前总队列不符，仅保留为历史记录。
这些数字是 roadmap 索引状态，不代表当前提交已全量通过 CI。

以下是 2026-10-04 会话的历史约定；其中本地禁测已被本轮用户明确要求完整测试所取代，
其他历史记录不视为当前授权：

- 本地只读源码、审 diff、编辑和格式化；不运行测试、build、cargo check/clippy、smoke、
  项目二进制或测试/验收脚本。编译、静态门、单测、集成、故障注入和平台验证均在 GitHub Actions。
- 每次领取前重新读取总队列。按明确硬依赖推进，最多同时处理三个互不冲突的 Step；
  每个写者使用独立 worktree，协调者统一修改 roadmap、状态账本、共享 manifest 和 lockfile。
- 优先修复阻塞多个 Step 的 CI 失败。每步完成源码后审查、提交并推送工作分支，触发相应 CI；
  等待期间继续独立任务，读取回执后修复失败。历史的“推送后不等待结果”不能替代验收。
- 只有完整退出条件满足、对应源码提交的必需 CI 成功、证据块齐备后才更新 ✅。
  CI 触发、source guard 通过、fake/cassette 或仅定义类型均不能升级为 durable/live/physical。
- 有失败、缺凭据、缺目标平台或未完成验收时保留 🔄 并记录具体阻塞；
  不删断言、不跳过测试、不缩小退出条件，也不把失败任务计为完成。
- 全部 Step 结束后仍须完成当前统一 CI、发布门、三入口一致性及目标平台验收。
  需要真实账户/设备的步骤按 roadmap 的 opt-in 与证据要求执行，记录缺失材料。

当前并行工作：PD-10 修复终态冲突稳定错误码；CI 工作流修复 CAP-20 的 YAML 和自动触发冲突；
协调者建立可重算的 goal 清单并核对 ER-01 等活动前沿的当前 CI。
仓库原有 PD-14 workflow 的未跟踪 WIP 保留在原工作树，不作为本次提交的一部分。

查看/恢复：`python3 scripts/roadmap-goal.py` 显示当前统计与可领取步骤；
`python3 scripts/roadmap-goal.py --checklist` 将当前队列同步到 Ruflo 的本地 checklist；
`ruflo task status task-1791102524181-0iivvl` 查看持久任务。
清单是派生视图，完成事实仍以 `CURRENT_STATUS.md` 绑定的提交和 CI 回执为准。

历史 Ruflo mission `msn_da9280fe41928026f08c5ef9` 曾记录为 `planned`；
当前可用 Ruflo CLI 未暴露 mission 命令，本次不把该历史记录当作已启动的执行器。


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

源码定位 → 分类 → 最小实现/修复 → 审 diff + source 证据 → 工作分支提交推送 → GitHub CI → 读取回执并修复 → 回填完成态。
CI 运行期间推进其他独立 Step；没有对应成功回执时保持 🔄。旧的“不等 CI，红了下一轮再修”只保留为历史做法。

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
