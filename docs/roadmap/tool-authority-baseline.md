# P1-H-01 single tool authority registry baseline

> 快照日期：2026-09-16。本页记录五个模型可见工具的 typed `ToolSpec` authority、别名、风险、schema 和 Runner/daemon 消费边界；本地不运行测试，运行时/guard 夹具只由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`P1-H-01`](../roadmap.md#step-p1-h-01) |
| feature_status | `implemented`（domain registry + Runner/daemon consumption source） |
| proof_level | `source`；静态编译和远程 fixtures 不提升为 `local_behavior`、`durable`、`live` 或 `physical` |
| canonical path | `TOOL_SPECS` → `model_tool_name/tool_spec` → Runner CapabilityRequest → ControlPlane/action catalog/Broker |
| this step does | 五个模型工具的单一名称/alias/capability/operation/risk/side-effect/schema registry，组合根启动校验和 alias/schema drift guard |
| this step does not | 不新增模型可见工具，不让 registry 替代 policy/gate/approval/action catalog，不改变 operator-only action 或实际 handler containment |

## 2. Authority rules

`TOOL_SPECS` 固定 `shell`、`apply_patch`、`mcp`、`memory.search`、`memory.write` 五个 canonical names。兼容别名只在 `tool_spec` 解析一次；Runner 先将模型名称归一化，再按 canonical name 构造 CapabilityRequest，避免 raw alias match 形成第二映射。`tool_schemas` 仍保存 bounded JSON schema，但其名称集合必须与 `TOOL_SPECS` 完全一致。

`validate_tool_authority` 在 DaemonHost 组合根执行，拒绝 surface 数量、canonical name、alias 冲突和 schema drift。`ToolSpec.side_effecting`/`risk_policy` 是审计元数据，不直接放行动作；ControlPlane action catalog、ScopeSet、approval、Broker 和 handler 仍是执行授权边界。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `tool_authority_covers_every_model_visible_tool` | 五个模型工具都有唯一 canonical spec、schema 和 action descriptor，operator-only action 不进入模型 schema |
| `runner_and_daemon_consume_the_single_tool_authority_registry` | catalog alias、Runner canonical mapping、daemon composition validation 和 schema registration 均指向同一 registry |

`.github/workflows/p1-h01-tool-authority.yml` 在 GitHub runner 执行 domain registry fixtures、core source guard、fmt 和 domain/runner/core/daemon test-target 编译；本地只做格式、静态编译和 diff 检查。

## 4. 限制与交接

- Registry 只描述模型工具入口，不等价于 capability grant、path containment、approval 或真实 handler 成功；P1-H-02/03、CAP/CP 继续收口。
- Provider wire 名称（如 `memory_search`）属于协议适配边界，必须映射回 canonical internal tool，不得成为新的 authority。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
