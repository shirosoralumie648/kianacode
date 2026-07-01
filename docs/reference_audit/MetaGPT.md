# MetaGPT Reference Audit

## 1. 在 Kiana 中的参考定位

`reference/MetaGPT` 对 Kiana 的主要参考是角色/动作/产物驱动的软件公司式 workflow、项目产物 JSON、tool registry、repo/index/linter/git 工具、RAG 示例和 provider test matrix。它更像多角色工程流程框架，不适合作为 Kiana P0 core；适合用于规划未来 structured workflow、artifact contracts 和 agent team evaluation。

## 2. Capability Inventory

| Capability | 用户可见行为 | 关键文件路径 | 实现机制摘要 | Kiana 相关性 | 是否适合借鉴 | 风险说明 |
|---|---|---|---|---|---|---|
| Team/roles/actions | 用户可让 PM/Architect/Engineer/QA 角色协作生成软件产物 | `reference/MetaGPT/metagpt/team.py`, `reference/MetaGPT/metagpt/roles/role.py`, `reference/MetaGPT/metagpt/actions/write_prd.py`, `reference/MetaGPT/metagpt/actions/write_code.py`, `reference/MetaGPT/metagpt/schema.py` | Team 管理角色，Role 执行 Action，schema 描述消息和产物 | future workflow mode | 部分 | P0 coding agent 不需要完整角色公司模型 |
| Role tests | 角色行为有单元测试 | `reference/MetaGPT/tests/metagpt/roles/test_product_manager.py`, `reference/MetaGPT/tests/metagpt/roles/test_architect.py`, `reference/MetaGPT/tests/metagpt/roles/test_engineer.py`, `reference/MetaGPT/tests/metagpt/roles/test_qa_engineer.py` | tests 检查特定角色 action 输出和状态 | future workflow tests | 是 | 测试依赖 prompt 输出时需使用 fake model |
| Tool registry/libs | 工具可注册、推荐，并包含 terminal/git/linter/index repo | `reference/MetaGPT/metagpt/tools/tool_registry.py`, `reference/MetaGPT/metagpt/tools/tool_recommend.py`, `reference/MetaGPT/metagpt/tools/libs/terminal.py`, `reference/MetaGPT/metagpt/tools/libs/git.py`, `reference/MetaGPT/metagpt/tools/libs/linter.py`, `reference/MetaGPT/metagpt/tools/libs/index_repo.py` | tool registry 与工具库分离，recommend 根据任务选择工具 | `kiana-tools/src/registry.rs`, `kiana-query/src/repo_map.rs` | 部分 | tool recommendation 可后置，基础 registry 已优先 |
| Project artifacts | PRD/design/tasks/dependencies 作为结构化项目产物 | `reference/MetaGPT/metagpt/utils/project_repo.py`, `reference/MetaGPT/metagpt/utils/graph_repository.py`, `reference/MetaGPT/metagpt/utils/di_graph_repository.py`, `reference/MetaGPT/tests/data/demo_project/prd.json`, `reference/MetaGPT/tests/data/demo_project/system_design.json`, `reference/MetaGPT/tests/data/demo_project/tasks.json`, `reference/MetaGPT/tests/data/demo_project/dependencies.json` | project repo 保存 JSON 产物，graph repository 描述依赖 | `kiana-commands/src/export.rs`, future workflow artifacts | 是 | 不能让产物格式锁死普通 coding loop |
| Provider tests | OpenAI/Ollama/Anthropic/Bedrock providers 有 tests | `reference/MetaGPT/tests/metagpt/provider/test_openai.py`, `reference/MetaGPT/tests/metagpt/provider/test_ollama_api.py`, `reference/MetaGPT/tests/metagpt/provider/test_anthropic_api.py`, `reference/MetaGPT/tests/metagpt/provider/test_bedrock_api.py` | provider 层按 provider 测试 | `kiana-services/src/api` | 是 | Live provider tests 必须 opt-in |
| RAG/retrievers | 示例和 tests 覆盖 BM25/FAISS retrieval | `reference/MetaGPT/examples/rag/rag_pipeline.py`, `reference/MetaGPT/tests/metagpt/rag/retrievers/test_bm25_retriever.py`, `reference/MetaGPT/tests/metagpt/rag/retrievers/test_faiss_retriever.py` | retrieval pipeline 与不同 retriever 分离 | `kiana-query/src/repo_map.rs` | 部分 | 向量依赖和隐私边界需后置 |
| Workflow examples/eval | 示例覆盖写游戏、修 issue、软件公司增量开发 | `reference/MetaGPT/examples/write_game_code.py`, `reference/MetaGPT/examples/di/fix_github_issue.py`, `reference/MetaGPT/tests/metagpt/test_software_company.py`, `reference/MetaGPT/tests/metagpt/test_incremental_dev.py` | examples/tests 表达端到端 workflow contract | future `kiana-eval` | 是 | 不适合直接纳入默认 CLI |

## 3. 值得概念性借鉴的实现模式

### Pattern: Structured workflow artifacts

来源文件：
- `reference/MetaGPT/tests/data/demo_project/prd.json`
- `reference/MetaGPT/tests/data/demo_project/system_design.json`
- `reference/MetaGPT/tests/data/demo_project/tasks.json`
- `reference/MetaGPT/tests/data/demo_project/dependencies.json`

机制摘要：
- 多步骤工程流程输出结构化 JSON 产物。
- 后续角色读取前序产物而不是只读自然语言 transcript。
- tests 可直接比较 artifact schema。

Kiana 可借鉴方式：
- 未来 workflow mode 可输出 plan/task/check artifact。
- 普通 session export 可保留 lightweight artifacts 字段。

不应该照搬的部分：
- 不把 PRD/system design 强加到每个 coding task。
- 不复制 MetaGPT 的具体角色分工文案。

### Pattern: Provider test matrix

来源文件：
- `reference/MetaGPT/tests/metagpt/provider/test_openai.py`
- `reference/MetaGPT/tests/metagpt/provider/test_ollama_api.py`
- `reference/MetaGPT/tests/metagpt/provider/test_anthropic_api.py`
- `reference/MetaGPT/tests/metagpt/provider/test_bedrock_api.py`

机制摘要：
- 每个 provider 单独测试。
- local/Ollama 与 cloud provider 分离。
- provider 能力差异通过 tests 暴露。

Kiana 可借鉴方式：
- `kiana-services/src/api` 应按 provider 建 fake/unit tests。
- live tests 通过 env gate 显式开启。

不应该照搬的部分：
- 不依赖真实 key 作为默认 CI。
- 不复制 provider payload。

## 4. Behavior Contracts to Port into Kiana

### Contract: Workflow Artifact Export

Input:
- session id
- workflow kind
- generated plan/tasks/checks

Decision:
- 哪些 runtime events 可转换成 artifact。
- artifact schema version。
- 是否允许覆盖已有 artifact。

Execution:
- 从 session store 读取 events。
- 生成 JSON artifact。
- 写入 export location。

Output:
- artifact file path。
- artifact schema version。

Runtime Events:
- SessionEvent
- AssistantMessage
- ToolResult
- RuntimeResult

Acceptance Tests:
- export 生成 versioned JSON。
- legacy session 没有 workflow metadata 时返回 clear error。
- artifact 可被 re-import 或 validate。

### Contract: Provider Matrix Smoke

Input:
- provider id
- model id
- smoke kind: text, tools, streaming
- opt-in live flag

Decision:
- provider 是否允许 live smoke。
- fake/local/cloud 测试路径。
- 缺少 key 是否 skip 或 fail。

Execution:
- 构建最小 request。
- 对 fake/local/cloud 分别执行。
- 记录 capability result。

Output:
- smoke report。
- provider capability status。

Runtime Events:
- SessionEvent
- RuntimeError

Acceptance Tests:
- 默认 CI 只跑 fake provider。
- 缺少 cloud key 时不会误报 core failure。
- live smoke 输出 provider/model/capability 状态。

## 5. Kiana Gap Analysis

| Capability | Kiana 当前状态 | 缺失行为 | 建议修改模块/crate | 测试要求 | 优先级 |
|---|---|---|---|---|---|
| Workflow artifacts | session export/compact 有基础 | structured plan/task/check artifact schema 未定义 | `kiana-commands/src/export.rs`, `kiana-types/src/runtime.rs` | artifact schema fixtures | P2 |
| Provider matrix | Anthropic 基础路径存在 | provider-by-provider fake/live smoke matrix 缺失 | `kiana-services/src/api`, `scripts/release-smoke.sh` | fake default, live opt-in | P0 |
| Tool recommendation | registry 已有 | task-based tool recommendation 不存在 | `kiana-tools/src/registry.rs` | recommendation fixtures | P3 |
| RAG retrievers | repo_map 基础存在 | BM25/vector retriever contract 未定 | `kiana-query/src/repo_map.rs` | offline retrieval fixtures | P3 |

## 6. Atomic Implementation Tasks

### Task: Add Provider Smoke Matrix

Goal:
- 让 Kiana 能报告每个 provider/model 的 text/tools/streaming smoke 状态。

Scope:
- 允许修改 `scripts/release-smoke.sh`, `kiana-services/src/api`, `kiana-commands/src/model.rs`。
- 不允许默认 CI 访问付费 API。

Implementation Notes:
- fake provider 总是跑。
- cloud provider 只有设置 opt-in env 时运行。

Acceptance Criteria:
- smoke report 包含 provider、model、capability、status、skip reason。
- 缺少 API key 时标记 skipped。
- fake provider failure 会让 CI fail。

Tests:
- `cargo test -p kiana-services provider_smoke`
- run `scripts/release-smoke.sh` locally.

Manual Verification:
- `KIANA_LIVE_PROVIDER_SMOKE=1 scripts/release-smoke.sh`

### Task: Define Workflow Artifact Schema

Goal:
- 为未来计划/任务/检查类 workflow 输出定义 versioned JSON artifact。

Scope:
- 允许新增 `docs/workflow-artifacts.md`, `kiana-types/src/workflow.rs`。
- 不实现多角色 workflow。

Implementation Notes:
- schema 从 plan、tasks、checks、dependencies 四类开始。
- export command 可先只 validate schema。

Acceptance Criteria:
- artifact schema 有 version 字段。
- JSON examples 可反序列化。
- export command 能拒绝 invalid artifact。

Tests:
- `cargo test -p kiana-types workflow_artifacts`
- `cargo test -p kiana-entrypoints export_workflow_artifact`

Manual Verification:
- `kiana export workflow --session <id>`
