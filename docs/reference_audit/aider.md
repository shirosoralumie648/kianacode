# aider Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/aider` 是 Kiana 本地代码修改闭环的首要参考，尤其是 repo map、git safety、editable/read-only files、diff/edit formats、lint/test repair、AI diff、undo、watch 和 benchmark。Kiana 不应照搬 Python edit-format protocol 或 prompt 文案，应把行为合同重做成 Rust tool runner 和 RuntimeEvent。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| CLI / scripting | `aider` 支持交互和脚本参数、model/config/git flags | `reference/aider/aider/main.py`, `reference/aider/aider/args.py`, `reference/aider/aider/commands.py`, `reference/aider/aider/website/docs/usage.md` | argparse 解析后构建 Coder、IO、Repo、Model | `kiana-entrypoints/src/cli.rs` | 部分 | Python CLI 参数很多，Kiana 只移植核心工作流 |
| Repo map | 用户可让模型理解大型 repo 的符号和文件结构 | `reference/aider/aider/repomap.py`, `reference/aider/tests/basic/test_repomap.py`, `reference/aider/tests/fixtures/sample-code-base-repo-map.txt`, `reference/aider/aider/website/docs/repomap.md` | tree-sitter/ctags 风格收集符号，按 token budget 输出 | `kiana-query/src/repo_map.rs`, `kiana-commands/src/context.rs` | 是 | tree-sitter 依赖和语言覆盖需按 Rust 生态重做 |
| Git safety / dirty state | 修改前后保护用户改动，显示 diff，支持 undo | `reference/aider/aider/repo.py`, `reference/aider/tests/basic/test_repo.py`, `reference/aider/aider/website/docs/git.md` | Git repo 封装 dirty state、commit/diff/undo 逻辑 | `kiana-commands/src/diff.rs`, `kiana-commands/src/checkpoint.rs` | 是 | 不复制 Python git command wrappers |
| Edit formats | 模型输出 search/replace、unified diff、whole-file 等格式 | `reference/aider/aider/coders/search_replace.py`, `reference/aider/aider/coders/udiff_coder.py`, `reference/aider/aider/coders/editblock_coder.py`, `reference/aider/tests/basic/test_udiff.py` | Coder 根据 model/edit format 解析 patch 并应用 | Kiana 已走 tool-use `Edit`/`Write` | 概念借鉴 | Kiana 不应切到 prompt-only edit protocol |
| Lint/test repair | 修改后运行 lint/test，失败时反馈给模型修复 | `reference/aider/aider/linter.py`, `reference/aider/aider/run_cmd.py`, `reference/aider/tests/basic/test_linter.py`, `reference/aider/aider/website/docs/usage/lint-test.md` | 发现检查命令、执行、摘要失败输出、再进入修复轮 | `kiana-commands/src/checks.rs`, `kiana-commands/src/review.rs`, `kiana-entrypoints/src/runner.rs` | 是 | 检查命令必须隔离，避免 mutating active tree |
| Benchmark/eval | SWE-bench 和 benchmark harness 用于比较模型/版本 | `reference/aider/benchmark/benchmark.py`, `reference/aider/benchmark/swe_bench.py`, `reference/aider/benchmark/README.md` | benchmark scripts 驱动任务、记录结果、生成统计 | future Kiana eval | 是 | 不要把 benchmark 作为默认 CI，成本高 |

## 3. 值得概念性借鉴的实现模式

### Pattern: Token-budgeted repo map

来源文件：
- `reference/aider/aider/repomap.py`
- `reference/aider/tests/basic/test_repomap.py`
- `reference/aider/aider/website/docs/repomap.md`

机制摘要：
- 从 repo 文件和符号提取结构摘要。
- 根据 token budget 裁剪。
- 输出稳定、可测试的文本地图。

Kiana 可借鉴方式：
- 保留 `kiana-query/src/repo_map.rs` 的 deterministic map。
- 增加更多语言 fixtures 和 token budget golden。

不应该照搬的部分：
- 不复制 tree-sitter query 文件或 Python ranking 代码。

### Pattern: Lint/test repair loop

来源文件：
- `reference/aider/aider/linter.py`
- `reference/aider/aider/run_cmd.py`
- `reference/aider/tests/basic/test_linter.py`
- `reference/aider/aider/website/docs/usage/lint-test.md`

机制摘要：
- 修改后运行检查。
- 将失败摘要作为下一轮模型输入。
- 限制重试次数。

Kiana 可借鉴方式：
- 使用 `kiana review --json` 和 isolated checks。
- 通过 runner option `repairChecks` 控制 opt-in。

不应该照搬的部分：
- 不在 active worktree 直接运行 mutating 检查。
- 不让 repair loop 无限重试。

## 4. Behavior Contracts to Port into Kiana

### Contract: Repo Map Context

Input:
- cwd
- max tokens
- include/exclude files

Decision:
- 是否在 git repo 中。
- 哪些文件可读。
- token budget 如何裁剪。

Execution:
- 扫描文件。
- 提取语言和符号摘要。
- 按稳定顺序输出。

Output:
- repo map text/JSON。
- truncated flag 和 token estimate。

Runtime Events:
- SessionEvent

Acceptance Tests:
- 同一 fixture 输出 byte-stable。
- max tokens 较小时会截断并报告。
- ignored/unreadable files 不进入 map。

### Contract: Isolated Repair Loop

Input:
- assistant turn result
- repairChecks flag
- max repair attempts

Decision:
- 是否本轮有代码修改。
- checks 是否失败。
- 是否还有重试预算。

Execution:
- 在 isolated worktree 运行 checks/review。
- 将失败摘要反馈给模型。
- 新修改后再次检查。

Output:
- final assistant text。
- repair attempt summary。

Runtime Events:
- ToolCall
- ToolResult
- RuntimeError
- RuntimeResult

Acceptance Tests:
- check pass 不触发 repair。
- first fail second pass 完成成功。
- 达到 attempt 上限后返回失败摘要。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Repo map | 已有 deterministic repo-map | 语言覆盖、ranking 和大 repo budget 仍需加强 | `kiana-query/src/repo_map.rs` | multi-language golden | P1 |
| Git checkpoint/undo | 已有 checkpoint/diff/undo 和 late user edit protection | conflict UX 和 non-git fallback 不完整 | `kiana-commands/src/checkpoint.rs`, `kiana-commands/src/diff.rs` | fixture repo conflict tests | P0 |
| Repair loop | 已有 opt-in non-streaming/streaming repair 和 public attempt/check/final summary | multi-check grouping、cost cap 需补 | `kiana-entrypoints/src/runner.rs`, `kiana-commands/src/review.rs` | mock model repair scenarios | P0 |
| Eval | release smoke 已有 | task replay/eval harness 未建设 | `scripts/`, future `kiana-eval` | replay fixture tests | P2 |

## 6. Atomic Implementation Tasks

### Task: Expand Repo Map Language Fixtures

Goal:
- 用 fixtures 锁定 Rust/Python/TypeScript/Java/Kotlin/Go 的 repo map 输出。

Scope:
- 允许修改 `kiana-query/src/repo_map.rs` 和相关 tests。
- 不允许引入重型 indexing 服务。

Implementation Notes:
- 先用 regex/tree-sitter-light approach，保持 deterministic。
- 每种语言一个小 fixture。

Acceptance Criteria:
- 输出稳定。
- 每种语言至少识别 top-level types/functions。
- token budget 裁剪有测试。

Tests:
- `cargo test -p kiana-query repo_map`

Manual Verification:
- `kiana context repo-map --json --max-tokens 2000`

### Task: Repair Loop Result Summary

Goal:
- 给用户显示 repair loop 的 attempt/check/final 状态，不暴露内部 JSON。

Scope:
- 允许修改 `kiana-entrypoints/src/runner.rs`, `kiana-commands/src/review.rs`, `kiana-entrypoints/src/cli.rs`。
- 不允许改变默认不开启 repair 的行为。

Implementation Notes:
- 复用 RuntimeEvent 和 existing review JSON。
- 输出 concise text 和 stream-json event。

Acceptance Criteria:
- repair 被触发时用户能看到失败检查、修复轮数和 retrying final status。
- check pass 时 summary 显示 no repair needed。
- 超出预算时有非零/structured failure。

Tests:
- `cargo test -p kiana-entrypoints repair_loop`
- `cargo test -p kiana-commands review`
