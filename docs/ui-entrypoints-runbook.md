# Kiana 入口 Runbook（UI-37）

本页是当前入口的操作地图，不把设计目标或类型存在写成运行时事实。当前 UI-29/30 仍是
source + GitHub CI fixture；真实 ACP/IDE 连接要等 UI-39 的显式 opt-in。

## 唯一执行脊柱

```text
CLI / TTY Workbench / Web / Electron Desktop / ACP adapter
  → versioned client/protocol DTO
  → DaemonHost
  → ControlPlane policy/gate/approval/lease
  → KianaHarness / Capability Broker
  → EventLog fact
  → Receipt / snapshot / feed / artifact projection
```

入口可以改变文案、布局和输入，但不能自己判定权限、执行 shell/MCP、写 EventLog、把 transcript
当事实或把 disconnect 当作 cancel/completed。

## 当前可运行入口

| 入口 | 主要命令/动作 | 事实与限制 |
|---|---|---|
| CLI | `kiana trust .`; `kiana run --sandbox workspace-write -- "..."`; `kiana run --receipt <session>` | 默认 `run` 只读；写入要求 trust + workspace-write；Receipt 由服务端/EventLog 投影 |
| TTY Workbench | `kiana`; `/sandbox read-only|workspace-write`; `Esc`/`Ctrl-C` | 复用同一 DaemonHost；输入/状态是展示层；不声称 token streaming/live |
| Web | `kiana web --no-open --bind 127.0.0.1:3080` | loopback Host/Origin/token；snapshot→SSE→hydrate；Unknown/gap 需查原 Receipt |
| Desktop | `bash scripts/install-desktop.sh`; 选择 Open/New/Continue；托盘 Keep/Quit | UI-24–28 typed IPC、sidecar/readiness、metadata-only persistence；关闭不隐式 cancel/resume/trust/approve |
| ACP/IDE | 当前仅 `kiana-client` fake-peer/source contract | UI-29/30 不连接真实 host、不 spawn、不拥有 permit；UI-39 才能显式 opt-in |

## 统一失败处理

1. `project_untrusted`、只读写盘、错误 sender/origin/token、foreign session、过期 approval、旧
   cursor/revision、scope/digest drift：保持拒绝且零 effect，回到服务端 snapshot/action query。
2. 响应丢失或 action 已 Accepted：使用原 command/idempotency 查询，不生成新 ID 重投。
3. feed gap/old epoch/worker crash：先 hydrate；未知外部/运行结果保持 `Unknown`，走 reconcile，
   不自动 retry/resume/approve。
4. Desktop detach：只保留布局和 opaque instance/session/cursor reference；重新 handshake，旧 cursor
   不能直接提交。

## 证据口径

- `feature_status=implemented` 只表示本切片源码和 CI wiring 已接入。
- `proof_level=source` 表示当前 UI-26–35 contracts/fixtures 的边界；不等于 local_behavior、durable、
  live 或 physical。
- GitHub Actions 是本项目测试/build/check 的权威；本次工作约定不在本地运行测试，不等待 CI。
- 真实 browser/PTY/Electron、多进程 EventLog recovery、provider/connector external effect、签名包、
  ACP/IDE live、跨平台安装和物理结果仍必须保留 limitation。

## 相关事实入口

- 当前账本：[`CURRENT_STATUS.md`](../CURRENT_STATUS.md)
- 入口设计：[`roadmap/ui-entrypoints.md`](roadmap/ui-entrypoints.md)
- 模块地图：[`module-map.md`](module-map.md)
- 用户命令：[`../USER.md`](../USER.md)
- 安全边界：[`company-os-security-constitution.md`](company-os-security-constitution.md)
