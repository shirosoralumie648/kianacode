# Kiana 全仓源码注释统一任务清单

**创建时间**：2026-10-08  
**目的**：建立单一事实来源，记录每个源文件的注释状态，区分"有注释"与"深度处理"与"质量审查"  
**更新策略**：每个工作批次完成后、上下文压缩后恢复时更新本清单

---

## 第一部分：统计口径定义

### 注释状态级别

| 级别 | 定义 | 验收标准 |
|------|------|--------|
| `未处理` | 文件存在，未添加教学级注释 | 原始代码，可能有极少量行内注释 |
| `扫描中` | Agent 正在阅读并理解源码 | 进行中，中间产物 |
| `注释中` | Agent 正在添加教学级注释 | 进行中，中间产物 |
| `待审查` | 注释已完成，等待独立质量审查 | 所有注释完成，待二阶段审查 |
| `审查中` | 独立审查 Agent 正在检查注释质量 | 进行中，中间产物 |
| `已通过` | 注释质量通过审查，且编译、格式检查通过 | 最终状态：可维护 |
| `有待改进` | 审查发现问题，需回炉修改 | 反馈给注释 Agent 修改 |
| `已提交` | 文件变更已提交到 git | 里程碑标记 |

### 深度处理定义

**已深度处理** = 满足以下全部条件：
1. Agent 逐行阅读源码，理解完整控制流
2. 添加了**函数级注释**（参数、返回值、用途）
3. 添加了**逻辑块注释**（条件分支、循环、状态转移）
4. 特殊语法有说明（async/await、unsafe、trait impl 等）
5. 通过格式检查（`cargo fmt --check`）
6. 通过编译检查（`cargo check --package`）

### 质量审查定义

**已审查并通过** = 满足以下全部条件：
1. 独立 Agent 以只读方式检查源码与注释配对
2. 验证注释解释了内部复杂逻辑，不只是外观用途
3. 验证 Rust 所有权、生命周期、并发、unsafe 的说明准确
4. 验证没有未依据的设计意图推断
5. 验证注释完整覆盖边界情况和错误路径
6. 验证没有机械重复的无信息量注释
7. 发现问题数 = 0，或已全部修复

---

## 第二部分：当前恢复状态（截至 2026-10-15）

### 本轮范围与判定

- HEAD：`d02bcd5a308ecbe468212f90070cf6f07b8b8296`，dirty worktree，暂存区为空。
- 第一片为 Runner 8 个文件的注释纠错；随后按当前清单继续独立的 P3-D Skills facade 注释片。
  两片均只改注释和进度记录，没有改运行逻辑、测试、roadmap、CURRENT_STATUS、Cargo/package
  manifest 或 lockfile；未 stage、commit、push、stash 或回滚。
- **“非文档 token 相同”只证明没有引入代码 token 变化，不等于注释已独立审查通过。**
- 两轮 workflow 均未返回有效审查报告：`wf_55cdf97f-5ff` 4 个 scope 失败，
  `wf_11645907-57b` 2 个 scope 失败。journal 只有 started/failed，没有审查结果。
  不能沿用旧文档的“94%/100%”“可立即提交”等结论。
- 单 reviewer `wf_7ba1cad7-b42` 已完整读取 8 个 Runner 文件并返回 4 条注释问题，
  集中在 retry.rs（2 条）、stream_normalizer.rs、state_driver.rs；后经修正并由 `wf_2aac667c-3ff`
  定点复核确认 4/4 fixed、剩余发现 0。
- P3-D Skills facade 独立审查 `wm8mdnpto` 完整读取 lib.rs，发现 3 条说明不准确；
  修正后 `wp9qvgyqs` 定点复核确认 3/3 fixed、剩余发现 0。两片均未提交。

### Batch 3 Phase 1：可隔离的注释修正

| 文件（均位于 `kiana-runner/src/`） | 本轮核实并修正的说明 | 当前状态 |
|---|---|---|
| `lib.rs` | 11 个模块；ProtocolRunner 不是 harness 的包装层；移除不存在的方法和接线描述 | 已通过、未提交 |
| `budget.rs` | scope 按项目/角色共享；真实 reserve/settle/release 流程；67,108,864 token 单位；unknown 用量；无账本自动持久恢复 | 已通过、未提交 |
| `retry.rs` | 3 次 attempt 含首次；request_count 实为调用方尝试计数；unavailable 由 harness 映射；3200ms 基础退避与 Retry-After/jitter | 已通过、未提交 |
| `progress.rs` | ProgressInput 是结构体；observe 返回决策；当前 harness 仅保存/校验 tracker，未调用 observe | 已通过、未提交 |
| `inbox.rs` | Duplicate/Claimed 返回值；旧序号与本次正文的区别；Cancel 独立；claim/收据错误无自动回滚 | 已通过、未提交 |
| `model.rs` | 两个本地实现；默认流式仅回调非空文本；脚本只接受数组或 outputs 数组，不自动包装单对象 | 已通过、未提交 |
| `state_driver.rs` | 已读完终态分支；便捷方法丢弃意图；步数校验；时间检查不在 DriverInput；取消请求不等于停止 | 已通过、未提交 |
| `stream_normalizer.rs` | 每 attempt 一个 accumulator；超限拒绝；stop 可来自最终输出；usage 允许增长而非相等；补齐不等于账本已同步 | 已通过、未提交 |

依据来自当前实现，不是为代码补新功能：预算看 `BudgetLedger` 与
`KianaHarness::{budget_scope,effective_budget,invoke_model,checkpoint,restore}`；
进度看 `kiana-domain/src/progress_stall.rs` 与 harness 的 tracker 引用；
重试看 `kiana-domain/src/retry_policy.rs`；默认流式看 `kiana-ports/src/model.rs`。

### 明确隔离：包含非注释变更的文件

以下文件与 HEAD 比较时非文档 Rust token 不同，本轮只读取、不修改：

| 文件 | 已确认的隔离原因 |
|---|---|
| `kiana-runner/src/protocol_runner.rs` | 新增 Resume 分支；不能作为旧“9 文件纯注释批次”整体提交 |
| `kiana-runner/src/compact.rs` | 压缩失败传播及历史裁剪逻辑变化 |
| `kiana-runner/src/harness.rs` | 模型循环、取消等既有行为变化 |
| `kiana-runner/src/tools.rs` | 工具翻译相关非文档 token 变化 |
| `kiana-core/src/dispatch.rs` | stop metadata、result_unknown 与 authority stream version 变化 |
| `kiana-core/src/approvals.rs` | 删除运行时 eprintln!，不是纯注释 |
| `kiana-eventlog/src/jsonl.rs` | 新增重复 JSON key 拒绝解析逻辑 |

`dispatch_delivery_tests.rs`、`automation_dispatch.rs` 及其他 WIP 未做完整质量审查，
同样保持原样。不为推进注释而自动 stash、拆行为补丁或清理工作树。

### 历史 Batch 1–2：保留范围，不继承完成声明

`d02bcd5a` 确认包含 `kiana-eventlog/src/retention_store.rs` 与 `artifact_store.rs`
的历史注释提交；这不证明它们的当前 dirty 版本通过审查。
其他文件的本批提交归属与独立审查证据未在本轮重建，全部按**待核实**处理：

- Core：`audit.rs`、`authority.rs`、`artifacts.rs`、`audit_projection.rs`、
  `incident_projection.rs`、`invocation_projection.rs`、`audit_projection_commit.rs`、
  `authority_read_model.rs`。
- EventLog：`lib.rs`、`writer_queue.rs`、`workflow_queue.rs`、`stream.rs`、
  `notification_outbox.rs`、`memory_store.rs`、`notification_dedup.rs`、`integrity.rs`、
  `credential_rotation.rs`、`memory.rs`、`ledger_source.rs`、`audit_contract.rs`、
  `governance_contract.rs`。

旧汇总的 23/32/35 文件、94% 完成度、覆盖率和行数不作为当前验收指标；
“历史提交存在”“当前 diff 纯注释”“当前注释质量通过”是三个独立事实。

### 验证回执

- 修改前：Runner library 68/68；本轮 6 个聚焦测试 target 共 10/10。
- 修改后：8/8 文件与 HEAD、修改前快照的非文档 Rust token 均一致。
- `cargo fmt --all --check`、`git diff --check`、
  `cargo check -p kiana-runner --locked --offline` 均通过。
- Runner 全 crate 回归：**134 passed（68 unit + 66 integration）、0 failed**；
  5 个原有 `ignore` doctest 未执行，不计入通过数。
- 补充 `cargo doc -p kiana-runner --no-deps --locked --offline`：成功；修正 model.rs 中
  `Vec<ModelOutput>` 缺少 Markdown 反引号的警告，非文档 token 仍相同。
  保留隔离文件 protocol_runner.rs 旧 `ignore` 示意图产生的 Rustdoc 解析警告；
  文档构建不等于 ignored doctest 已执行。
- Runner 收口时只变更 Runner 8 个源码注释与本清单，其余 806 个既有 dirty/untracked 文件哈希未变。
- 后续 P3-D 切片从一个与 HEAD 完全一致的 `kiana-skills/src/lib.rs` 开始；现新增该源码 diff，
  并更新本清单和 `SOURCE_INVENTORY.tsv`。目前相对最初 815 个路径，10 个既有路径内容改变，
  新增 1 个此前干净的 Skills 源文件路径；805 个其他既有 dirty/untracked 文件保持原样。
  HEAD 和空暂存区均未改变。
- 独立 Runner 注释审查与四条修正意见的定点复核已完成；P3-D Skills facade 独立审查及三条修正复核亦完成。未跑完整 workspace、release gate 或 live provider。
  保留 domain/runner 既有 unused import、dead-code 等 warning，没有顺手修代码。

```text
source_snapshot: HEAD d02bcd5a308ecbe468212f90070cf6f07b8b8296 + dirty worktree; before.json / after.json
worktree_status: 8 Runner annotation files + this manifest changed; all other WIP, HEAD and index preserved
command_argv: CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 RUST_MIN_STACK=33554432 cargo test -p kiana-runner --locked --offline --no-fail-fast -- --test-threads=1
command_argv: CARGO_BUILD_JOBS=1 RUST_TEST_THREADS=1 cargo check -p kiana-runner --locked --offline
command_argv: cargo fmt --all --check; git diff --check; token-check/target/debug/annotation-token-check --strip-docs BEFORE AFTER (8 files, against HEAD and before snapshot)
command_argv: CARGO_BUILD_JOBS=1 cargo doc -p kiana-runner --no-deps --locked --offline
cwd·environment: repository root; Linux; offline/locked dependencies; one Cargo job and one test thread; no network/provider call requested
fixture·cassette: existing Runner unit/integration fixtures; 134 passed, 0 failed; 5 existing ignored doctests
exit_code: 0 for Runner tests/check/doc, fmt/diff and eight-file token equality checks
status_change: independent review covered 8 files; all 4 comment findings corrected and targeted verification confirmed fixed; uncommitted
proof-level_change: none; no roadmap card or product capability promoted
limitations: first two broad Runner workflow audits failed; both independent comment reviews and targeted correction verification succeeded; one preexisting protocol_runner Rustdoc warning; no workspace/release/live validation
reviewer: Runner integration session + independent review wf_7ba1cad7-b42 + targeted verifier wf_2aac667c-3ff (4/4 fixed, no remaining findings); Skills review wm8mdnpto + targeted verifier wp9qvgyqs (3/3 fixed, no remaining findings)
```

### P3-D Skills facade（`kiana-skills/src/lib.rs`）

- 候选文件在本片开工前与 HEAD 内容完全一致，560 行；源码审计完整读完，并确认没有
  依赖该文件注释文本或行位置的 source guard。
- 本片错误修正：catalog `SkillCatalogEntry`/search 仅返回元数据，但 `load_all_skills*` 已在 loader 阶段
  把正文写入 `Command.content`；`load_skill_body` 只对预加载正文计预算并复制，`activate_skill` 不读正文。
- path 条件项会尝试进入 legacy store，但 `store_conditional_skill` 丢弃内部 Result，不能保证已存储。
- catalog 构建错误内部以空 catalog 兜底，公开 Vec/cache 内容实际是空命令列表，不是返回 catalog DTO。
- 测试环境证据：直接使用当前 HOME 时 3 个 target 有 8 个失败（25 passed），失败均包含
  `RootSymlink("/home/shirosora/.claude/skills/baoyu-slide-deck")`；这反映当前用户 skill 目录
  的 symlink 夹具干扰，不能算作本片代码回归。把 HOME、KIANA_HOME、plugins 与 XDG config
  隔离到 `/tmp`、同时显式复用现有 RUSTUP_HOME/CARGO_HOME 后，未改源码基线 **33/33 passed**，
  注释修改后仍 **33/33 passed**。仅替换 HOME 而未复用 rustup home 的第一次尝试在测试启动前
  因离线下载 toolchain metadata 失败，不计为测试结果。
- `cargo check -p kiana-skills --locked --offline`、`cargo doc -p kiana-skills --no-deps --locked --offline`、
  `cargo fmt --all --check`、`git diff --check` 均通过；最终修正后的 token checker 仍与 HEAD 相同。

- 独立注释审查 `wm8mdnpto` 完整读过该文件，发现 3 条不准确说明；本轮修正正文加载/披露边界、
  conditional store 失败语义和 catalog 空结果类型。定点复核 `wp9qvgyqs` 确认 3/3 fixed、
  remaining_findings=[]。
- 回执文件：`skills-lib-before.rs`、`skills-lib-head.rs`、`skills-lib-before.sha256`、
  `skills-tests-before.log`、`skills-tests-before-isolated.log`、`skills-tests-after.log`、
  `skills-check-after.log`、`skills-doc-after.log`，均位于 `/tmp/kiana-annotation-recovery-20261014-SaieSf/`。

---

证据目录（本机临时文件）：`/tmp/kiana-annotation-recovery-20261014-SaieSf/`。
其中 `before.json` 保存既有 dirty/untracked 路径的内容哈希，`head.txt` 与
`index-before.patch` 保存 HEAD/暂存区，8 个 `.rs` 保存修改前副本；
`independent-review.json` 保存已完成的八文件审查报告；
`token-check/` 是仓库外构建的 proc-macro2 检查器，`token-baseline.log`、
`token-after.log` 记录分类和修改前后对比。它保留字符串、字符、标点及非文档属性，
只忽略注释对应的 doc 属性；检查器自测覆盖 lifetime、raw string、嵌套注释及应被识别的
代码/字符串/属性变化。这是限定范围的词法证据，不是任意程序的语义等价证明。

恢复时先重新核对 HEAD 与文件哈希。旧 `READY_TO_COMMIT.md`、`ACTION_NOW.md`、
`READY_FOR_REAL_COMMIT.md`、`FINAL_SESSION_DECISION.md` 等只作历史记录，
其中通配符 add、自动提交、工作树将变干净的建议已经失效；不能直接照做。

---

## 第三部分：待处理文件清单

以下保留历史候选顺序，行数、状态与工时均待重新核对，不是当前领取许可。
开始任何新文件前须先分类当前 diff；第二部分隔离项不得按此表直接推进。

### 优先级顺序（Q4 路线图依据）

#### 优先级 1：关键路径（执行引擎与决策）

这些文件直接影响 Kiana 的执行模型和授权决策。

| 优先级 | 文件 | 行数 | Crate | 状态 | 估算工时 |
|--------|------|------|-------|------|--------|
| P1-A | `kiana-core/src/capabilities.rs` | 1,200+ | kiana-core | 未处理 | 6-8h |
| P1-B | `kiana-core/src/lifecycle.rs` | 1,500+ | kiana-core | 部分完成* | 4-6h |
| P1-C | `kiana-runner/src/harness.rs` | 1,800+ | kiana-runner | 未处理 | 8-10h |
| P1-D | `kiana-runner/src/tools.rs` | 1,400+ | kiana-runner | 未处理 | 6-8h |
| P1-E | `kiana-daemon/src/model_client.rs` | 1,600+ | kiana-daemon | 部分完成* | 4-6h |

*注*：`lifecycle.rs` 与 `model_client.rs` 已有一轮注释但未独立审查通过

#### 优先级 2：存储与数据（历史完成状态待核实）

| 优先级 | 文件 | 行数 | Crate | 状态 | 估算工时 |
|--------|------|------|-------|------|--------|
| P2-A | `kiana-eventlog/src/event_store_core.rs` | 400+ | kiana-eventlog | 工作区改动 | 4-5h |
| P2-B | `kiana-eventlog/src/journal_core.rs` | 550+ | kiana-eventlog | 工作区改动 | 5-6h |
| P2-C | `kiana-eventlog/src/jsonl.rs` | 1,200+ | kiana-eventlog | 工作区改动 | 6-8h |

#### 优先级 3：外围能力与适配（Provider、Query、Skills）

| 优先级 | 文件 | 行数 | Crate | 状态 | 估算工时 |
|--------|------|------|-------|------|--------|
| P3-A | `kiana-provider/src/request.rs` | 800+ | kiana-provider | 已部分处理 | 3-4h |
| P3-B | `kiana-provider/src/response.rs` | 600+ | kiana-provider | 工作区改动 | 3-4h |
| P3-C | `kiana-query/src/context.rs`（不存在） | — | kiana-query | **失效：清单路径不存在；`context_inputs.rs` 已有注释记录，不擅自替换目标** | — |
| P3-D | `kiana-skills/src/lib.rs` | 560 | kiana-skills | 独立审查及 3 条修正复核均通过；非文档 token 相同；隔离测试 33/33 通过；未提交 | 2-3h |

#### 优先级 4：入口与可选面（CLI、Web、MCP、TUI）

| 优先级 | 文件 | 行数 | Crate | 状态 | 估算工时 |
|--------|------|------|-------|------|--------|
| P4-A | `kiana-entrypoints/src/cli.rs` | 800+ | kiana-entrypoints | 部分处理 | 4-5h |
| P4-B | `kiana-entrypoints/src/web.rs` | 600+ | kiana-entrypoints | 未处理 | 3-4h |
| P4-C | `kiana-entrypoints/src/workbench.rs` | 1,200+ | kiana-entrypoints | 未处理 | 6-7h |
| P4-D | `kiana-entrypoints/src/runner.rs` | 500+ | kiana-entrypoints | 未处理 | 2-3h |

---

## 第四部分：进度追踪与持久化策略

### 每个工作单元完成后的检查清单

```
□ Agent 逐行阅读源码并理解完整逻辑
□ Agent 添加函数级、逻辑块级注释
□ 特殊语法（async/unsafe/trait）已注释
□ cargo fmt --all --check 通过
□ cargo check -p <crate> 通过
□ git diff --check 无错误
□ 本清单状态更新为"待审查"
□ 独立审查 Agent 检查注释质量
□ 审查 Agent 发现 0 个问题或已全部修复
□ 本清单状态更新为"已通过"
□ 用户明确授权提交时，逐文件复核后执行 git commit
□ 本清单状态更新为"已提交"
```

### 上下文压缩后的恢复策略

1. 阅读本清单（ANNOTATION_TASK_MANIFEST.md）的"当前状态"
2. 检查 `git status` 找出工作区改动
3. 根据优先级和前置依赖决定下一批工作
4. 必要时运行 `cargo check --workspace --keep-going` 确认基线
5. 从下一优先级项开始执行

### 清单更新规则

- **每批工作完成时**：更新相应文件的"状态"、"审查"、"提交"列
- **发现阻塞时**：在"备注"列记录原因（工作区改动、前置依赖、编译错误等）
- **审查发现问题时**：标记"有待改进"，记录问题类型，回炉修改
- **批次周期性报告**：生成 COMMENT_PROGRESS_BATCH*.md，引用本清单数据

---

## 第五部分：质量审查标准

### 注释应该解释什么

✅ **必须**：
- 函数的用途、参数含义、返回值含义
- 复杂逻辑块的目的（"为什么做这个判断"）
- 条件分支的后果（"如果 X 成立会怎样"）
- 循环处理的范围和终止条件
- 状态转移的触发条件
- 错误来源、传播路径、恢复方式
- async/await 的任务关系
- unsafe 的内存安全条件
- 共享状态与锁的目的

❌ **不应该**：
- 只重述代码字面意思（"这行创建一个 Vec"）
- 明显的函数名已说明的用途（`validate_input` 不需要"校验输入"）
- 推测性的设计意图（"可能是为了"）
- 只有代码才能验证的约束（源代码本身即是真实）

### 审查检查清单

```
□ 注释解释了内部逻辑，不只是表象
□ 变量定义说明了数据来源和用途
□ 条件分支说明了判断目的和后果
□ 循环说明了处理范围和终止条件
□ 函数调用说明了参数如何构造、结果如何使用
□ async/await 说明了任务关系
□ 并发代码说明了锁、信号、共享状态
□ unsafe 块有 SAFETY 注释说明内存条件
□ 错误处理说明了来源、路径、恢复
□ 状态机说明了转移条件
□ 没有机械重复的无信息量注释
□ 所有注释有源码依据，没有未确认的推断
□ 特殊情况和边界情况被涵盖
```

---

**当前状态**（2026-10-15）：
- Runner 8 个文件已通过独立审查和 4 条修正复核。
- P3-D `kiana-skills/src/lib.rs` 已通过独立审查和 3 条修正复核；隔离测试 33/33 通过。
- 7 个已识别混合文件隔离不动；P3-C `kiana-query/src/context.rs` 不存在，已标失效。
- 其他 WIP 保持不动；全部注释变更未提交，提交仍需明确授权。

