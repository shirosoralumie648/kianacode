# P1-J4-01 Capability Descriptor 与 MCP 生命周期基线

> 快照日期：2026-09-18。本文记录 MCP 在产品 DaemonHost 路径的可复核合同；运行时验收由 GitHub Actions 负责，本地不运行测试。

## 已交付的受控路径

Capability catalog 仍是唯一 server-owned descriptor：`CapabilityKind::Network` 的 `mcp.discover` 与 `mcp.call` 具有固定 operation、risk、resource fields、argument/result schema、binding version、取消、reconciliation 和 idempotency 描述，Broker 在组合根 seal 后只接受精确 `(kind, operation)` 绑定。descriptor/catalog 不是授权本身；ControlPlane 的 policy/gate/approval/permit 仍是唯一授权来源。

产品 MCP adapter 的生命周期为：

1. `McpRegistry` 读取 bounded 配置，只允许 stdio，解析并 pin server command、参数文件、环境白名单和配置 hash；未 trust 的 project、重复/未知 server、HTTP/SSE/WS transport、越界参数 fail-closed。
2. operator-only `mcp.discover` 在受控 sandbox 中启动一次 per-invocation process，完成 MCP initialize/tools/list，校验协议版本、tool input/output schema、metadata、分页、帧/字节预算，并把 config pin、scope、catalog digest、protocol、tools、版本和固定 `health_snapshot` 写入 `mcp.discovery_committed` EventLog stream。
3. `mcp.call` 只接受带 discovery version/catalog/config/executable/health snapshot 的 prepared request；执行前重新校验 scope、trust、pin、catalog、schema 和 health，漂移不启动业务调用。
4. 每次调用使用隔离的 stdio client/process group；输出带 server info/config/catalog/input/output schema hash、health、diagnostics、untrusted_data 和 adapter result。取消、响应缺失或 stop 未确认不会伪造成功，按 `cancelled`/`result_unknown` 保留 reconciliation 证据。

`health_snapshot` 只在 initialize 和 tools/list 成功后产生 `status=ready`，包含协商 protocol version、检查项和 tool count；因此健康是可追踪的本地握手事实，不是远端业务可用性或外部效果证明。

## CI-only 验收

`mcp_tool_schema_and_health_are_traceable` source guard 覆盖 descriptor/broker binding、stdio-only、trust/scope/pin、discovery/health/version/digest、schema/result validation、分页/帧/字节界限、process-group stop 和 Unknown 边界。现有 `harness_mcp` inline fixtures 继续由同一 workflow 在 CI 运行拒绝路径（unknown/ambiguous/malformed/oversized/schema/result）和静态 workspace compile。

```text
cargo fmt --all --check
cargo test -p kiana-daemon --lib harness_mcp::tests:: --locked -- --test-threads=1
cargo test -p kiana-daemon --test p1_j4_01_mcp --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行 `cargo fmt --all`、`cargo fmt --all --check`、`cargo check --workspace --tests --locked --offline` 和 `git diff --check`；不执行测试或 smoke binary，也不等待 GitHub CI。

## 限制与交接

- HTTP/SSE/WS MCP 与远程授权仍按冻结边界返回 unsupported；`kiana-services` 中的 legacy client 不属于产品执行路径。
- discovery/health/catalog 是 EventLog 派生/事实边界，但尚无跨进程 MCP pool、durable health heartbeat、自动 drift projector 或外部业务 receipt/reconciliation；这些留给 CAP-20+、PD/ER/SC/INT。
- serverInfo/health 和工具结果均为不可信输入，不能授予 grant、approval、scope 或 role；真实外部效果、live/physical proof 不在本切片声明内。

