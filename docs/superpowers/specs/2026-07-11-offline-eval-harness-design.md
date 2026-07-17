# Kiana Offline Eval Harness Design

## 1. 目标

为 Kiana 增加一个可随源码和发行包运行的离线评测入口，用稳定 fixture 回放验证 RuntimeEvent、工具生命周期、最终结果和文本输出契约。

首版解决以下问题：

- 功能回归目前分散在 Rust 测试与 release smoke 中，缺少产品级统一评测报告；
- 无法用同一套 fixture 比较不同提交、发行包或后续 agent runtime 实现；
- AutoGen/aider 类 benchmark 参考尚未转化为 Kiana 可执行机制；
- 商业交付需要可归档、可复查、无需外部凭据的本地质量证据。

## 2. 非目标

首版明确不做：

- 不执行任意 shell 命令；
- 不访问网络或调用付费模型；
- 不运行 SWE-bench、HumanEval 或下载第三方数据集；
- 不提供排行榜、云端 dashboard 或团队共享服务；
- 不把 fixture 通过等同于真实客户验收或线上服务证据；
- 不做统计显著性、模型质量评分或 token 成本预测。

## 3. 方案比较

### 方案 A：命令/脚本 benchmark

由 suite 直接配置任意命令并检查退出码、stdout、stderr。

优点：通用、容易接入现有 smoke。

缺点：安全边界过大，容易变成第二套 CI runner；结果受宿主机、PATH、网络和外部服务影响，不满足首版确定性要求。

### 方案 B：fake-provider 端到端 agent scenario

suite 配置模型脚本、工具返回和期望对话，调用完整 runner。

优点：覆盖真实 agent loop，长期价值最高。

缺点：需要稳定公开 fake-provider、runner 构造、临时 session 和工具沙箱接口；首版范围过大，失败归因复杂。

### 方案 C：RuntimeEvent JSONL 回放评测

suite 引用本地 RuntimeEvent JSONL fixture，评测器只解析事件并检查稳定断言。

优点：完全离线、无副作用、可打包、可归档；直接复用 Kiana 已稳定的 RuntimeEvent 公共契约；后续 fake-provider 场景也可以输出同一 JSONL 后进入同一评测器。

缺点：首版只证明回放和输出契约，不证明模型推理质量。

### 决策

P0 采用方案 C。P1 在同一 suite/report 契约上增加 `fake_provider_scenario`，P2 再增加 baseline comparison 与可选外部 benchmark adapter。

## 4. 用户入口

```bash
kiana eval run --suite docs/eval/fixtures/basic-runtime-suite.json --json
```

Human 输出：

```text
Eval suite: basic-runtime
status: passed
cases: 2/2 passed
events: 9
tool_calls: 1
tool_errors: 0
```

支持参数：

- `run`：执行 suite；
- `--suite <path>`：必需，指向 `kiana.eval-suite.v1`；
- `--json`：输出 `kiana.eval-report.v1`；
- `--fail-on-failure`：任一 case 失败时命令返回错误，供 CI/release gate 使用；
- `--help`：输出使用说明。

首版不隐式搜索 suite，不自动下载 fixture，不自动选择“最新”结果。

## 5. Suite 数据契约

```json
{
  "schema": "kiana.eval-suite.v1",
  "id": "basic-runtime",
  "description": "Offline RuntimeEvent replay contract",
  "cases": [
    {
      "id": "tool-success",
      "kind": "runtime_event_replay",
      "fixture": "basic-tool-success.jsonl",
      "expect": {
        "final_status": "completed",
        "stop_reason": "end_turn",
        "min_event_count": 4,
        "tool_call_count": 1,
        "tool_error_count": 0,
        "required_tool_names": ["Read"],
        "final_text_contains": ["done"]
      }
    }
  ]
}
```

规则：

- `schema` 必须精确匹配；
- suite ID 和 case ID 不能为空，case ID 不能重复；
- `kind` 首版只能是 `runtime_event_replay`；
- fixture 路径相对于 suite 文件目录解析；
- 绝对路径、`..` 逃逸、目录、符号链接逃逸必须拒绝；
- fixture 必须是 UTF-8 JSONL，每个非空行必须是对象；
- 至少存在一个 case，每个 case 至少定义一个 expectation；
- 未知字段拒绝，避免拼写错误静默失效。

## 6. RuntimeEvent 归一化

评测器接受两种行形态：

1. 直接 RuntimeEvent 对象；
2. session JSONL record 中嵌套的 `event` 对象。

首版读取以下公共字段：

- `type`；
- `status`；
- `stop_reason`；
- `assistant_text` 或文本 payload；
- `tool_name`；
- `tool_call_id` / `tool_use_id`；
- `is_error`、`error` 或失败状态；
- usage 中的 input/output token 数。

`type=usage` 的记录必须同时提供一组 input token 字段（`input_tokens` 或 `prompt_tokens`）和一组 output token 字段（`output_tokens` 或 `completion_tokens`）。字段值必须是可表示为 `u64` 的非负整数；缺失、字符串、浮点数、负数或溢出都属于 fixture 契约错误，并报告 case、行号和字段名。非 usage 事件可以不含 token 字段，但一旦显式提供，同样必须满足非负整数约束。评测器不得把缺失或非法 token 静默折算为 0。

未知事件类型保留在 `event_type_counts`，但不会自动失败；无效 JSON、非对象行、缺少事件类型、usage token 字段缺失或 token 类型非法会使 suite 命令失败，不生成伪造的 passed/failed 报告。

## 7. Case 指标与断言

每个 case 计算：

- `event_count`；
- `event_type_counts`；
- `assistant_text_count`；
- `tool_call_count`；
- `tool_result_count`；
- `tool_error_count`；
- `tool_names`；
- `input_tokens`、`output_tokens`；
- `final_status`；
- `stop_reason`；
- `final_text`；
- `fixture_sha256`。

首版支持断言：

- `final_status` 精确匹配；
- `stop_reason` 精确匹配；
- `min_event_count`；
- `tool_call_count`；
- `tool_error_count`；
- `required_tool_names` 全包含；
- `final_text_contains` 全包含；
- `max_input_tokens`、`max_output_tokens`。

每个失败断言产生一个结构化 finding：

```json
{
  "code": "tool_error_count_mismatch",
  "expected": 0,
  "actual": 1,
  "message": "expected tool_error_count=0, got 1"
}
```

## 8. Report 数据契约

```json
{
  "schema": "kiana.eval-report.v1",
  "suite_id": "basic-runtime",
  "suite_sha256": "64-hex",
  "status": "passed",
  "summary": {
    "total": 1,
    "passed": 1,
    "failed": 0,
    "events": 4,
    "tool_calls": 1,
    "tool_errors": 0
  },
  "cases": []
}
```

状态只有：

- `passed`：全部 case 通过；
- `failed`：至少一个 case 完成评测但断言失败；
- suite/fixture 契约错误属于命令错误，不生成伪造的 passed/failed 报告。

报告不得包含环境变量、API key、fixture 绝对路径或原始完整对话；只保留 suite 相对路径、摘要、哈希和断言证据。

## 9. 安全与信任边界

- 评测器只读；
- 不调用 shell、网络、MCP、插件、模型或 hooks；
- suite-relative canonical path 必须位于 suite 目录内；
- 单 fixture 默认上限 16 MiB、单 suite 默认最多 256 cases、单 fixture 最多 100,000 行；
- 数值断言必须是非负整数，token 上限不得使用浮点数；
- 文本 contains 按 UTF-8 字面量匹配，不执行 regex；
- JSON 输出不得泄露 canonical absolute path。

## 10. 发行与证据

新增：

- `docs/schemas/kiana-eval-suite.v1.schema.json`；
- `docs/schemas/kiana-eval-report.v1.schema.json`；
- `docs/eval/fixtures/basic-runtime-suite.json`；
- 对应 RuntimeEvent JSONL fixtures。

发行包必须包含 schemas 与 `docs/eval/fixtures`，package lifecycle smoke 必须在解包目录运行 basic suite，并验证 JSON report schema/status。

## 11. 验收标准

- 默认 registry 和 CLI 可发现 `eval`；
- basic packaged suite 离线通过；
- malformed suite、重复 ID、路径逃逸、未知 kind、无 expectation、坏 JSONL 均拒绝；
- usage 事件缺失 input/output token、token 为字符串/浮点/负数或溢出时均拒绝；
- passing 与 failing case 都生成稳定逐 case 指标；
- `--fail-on-failure` 对 failing report 返回错误；
- schema contract、focused tests、workspace tests、release build、package lifecycle smoke 全部通过；
- roadmap 与 feature matrix 明确记录该切片只完成离线 replay eval，不冒充 fake-provider、SWE-bench 或客户验收。

## 12. 后续阶段

P1：

- `fake_provider_scenario`；
- agent/team termination 与 replay 场景；
- baseline report comparison；
- `kiana eval compare`。

P2：

- opt-in HumanEval/SWE-style adapters；
- 多模型/多版本统计；
- dashboard 与历史趋势；
- 真实 provider eval，但必须由外部凭据与成本审批显式启用。
