# Kiana 架构说明

本文描述 **master 上实际存在的控制平面分层**。权威设计稿仍是 [2026-07-17 控制平面规格](superpowers/specs/2026-07-17-kiana-control-plane-architecture-design.md)；若本文与规格冲突，先修规格或先修代码，再改索引。

## 现状

```bash
kiana architecture status --json
```

当前报告：

```json
{
  "schema": "kiana.architecture-status.v1",
  "control_plane": "kiana-core",
  "composition_root": "kiana-daemon",
  "runner": "kiana-runner",
  "harness": "kiana-harness",
  "capability_mode": "brokered",
  "legacy_prompt_loop": false,
  "legacy_edges_remaining": 9
}
```

`legacy_edges_remaining: 9` 表示迁移未完成。不要把 harness 落地写成全部 surface 已脱离旧路径。

## 分层

```text
Product surfaces
  kiana-entrypoints (CLI / REPL / print / tui / MCP / daemon / chrome / computer-mcp)
  kiana-screens / kiana-components / kiana-tui / kiana-ink
        |
        v
Client / protocol
  kiana-client
  kiana-protocol
        |
        v
Composition root
  kiana-daemon          adapter、EventLog、broker 接线
        |
        v
Control plane
  kiana-core            capability / operation / risk
  kiana-domain          纯领域模型
  kiana-ports           trait 与端口
  kiana-policy          trust / 权限 / 网络
  kiana-gates           approval / verification
  kiana-workflow        DAG / resume / recovery
        |
        v
Execution
  kiana-runner-protocol
  kiana-runner::KianaHarness
  kiana-capability-broker
  kiana-commands / kiana-tools / kiana-query / kiana-services / kiana-skills / kiana-tasks
        |
        v
Persistence / evidence
  kiana-eventlog
  artifacts / projections / audit（通过 daemon 与 ports）
```

## 不变量

1. **入口隔离**：CLI/TUI/SDK/MCP 不能直接调用 Tool、Service、Task 或 Query implementation。
2. **Core 权威**：`kiana-core` 独立确定 capability、operation 和 risk；客户端不能自签权限。
3. **单一组合根**：`kiana-daemon` 是 concrete adapter、EventLog 和 broker 的组合根。
4. **副作用可恢复**：外部副作用必须经过 ProjectTrust、policy、gate、approval 和可恢复事件合同。
5. **owned harness**：生产循环是 `kiana-runner::KianaHarness`，实现 `RunnerPort`。工具执行走 capability broker。
6. **reference 隔离**：`reference/**` 只作审计输入，不参与 workspace 构建。

## Crate 职责

| Crate | 职责 |
| --- | --- |
| `kiana-entrypoints` | 进程入口：CLI 路由、REPL、print、`kiana tui`、MCP、daemon、native host |
| `kiana-client` / `kiana-protocol` | 请求构造、传输、重试、流消费 |
| `kiana-daemon` | 组合根：host、session、stream、adapter wiring |
| `kiana-core` | 控制平面决策 |
| `kiana-runner` | owned harness 与 runner protocol 实现 |
| `kiana-commands` | slash/local 命令 |
| `kiana-tools` | 模型可调用工具、sandbox、permissions |
| `kiana-query` | repo map / context / hooks |
| `kiana-tasks` / `kiana-workflow` | 长任务、DAG、evidence、swarm |
| `kiana-screens` | 产品 TUI 屏幕（`kiana tui`） |
| `kiana-tui` | TUI 组件库与独立实验二进制 |
| `kiana-capability-governance-supervisor` | 能力治理 production/public 切片监督 |

成员列表以根 `Cargo.toml` 为准，当前 38 个 crate。

## 数据流

### Local command

1. CLI / REPL / TUI 识别命令
2. `create_default_command_registry()` + `CommandContext`
3. 返回 `CommandResult`；prompt 类命令可再进入 assistant 路径

### Assistant / harness

1. SDK、print mode 或 `kiana run` 创建/恢复 session
2. `KianaHarness` 组装模型循环，把工具请求交给 brokered 执行路径
3. 事件写入 session JSON / `events.jsonl` 或 EventLog

### 产品 TUI

`kiana tui` 走 `kiana-entrypoints/src/tui.rs` → `kiana-screens::run_app_with_handlers`。它消费同一套 command registry 和 session 合同，而不是复制一套 core。

独立二进制 `kiana-tui` 用于组件演示和后续接线，见 [tui.md](tui.md)。

## 未进入 master 的实验面

以下内容保留在 `wip/unlanded-extensions`，**不是**当前 workspace 成员：

- `kiana-acp-server`
- `kiana-plugins`
- `kiana-scripting`
- `extensions/vscode`

master 上的 `kiana plugin` 命令管理的是既有 plugin 运行时合同，不代表上述 crate 已接入构建。
