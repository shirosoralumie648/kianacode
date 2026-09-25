# UI-26 Desktop workspace、托盘、通知与关闭策略基线

> 快照日期：2026-09-25。Electron/Node 与 Rust 验收由 GitHub Actions 执行；本步骤不在本地
> 运行测试、build、check、clippy 或 smoke，且不等待 CI 结果。

## 状态与唯一边界

`contrib/desktop/lib/desktop-state.js` 提供 `kiana.desktop-state.v1` 的纯 reducer。它只记录
当前工作区、worker lifecycle、实例 identity、未保存草稿标志，以及由服务端事实触发的有界
`pending_count`/`unknown_count`。切换工作区会清空旧 workspace 的 attention 和 notification
dedupe 窗口，避免旧窗口/旧 worker 的事实串入新窗口；重复 notification identity 不重复计数。

工作区打开、新建、继续和状态查询仍由已有 `kiana.desktop.intent.v1` typed intent 进入
Electron 主进程，再复用既有 `DaemonHost`/Web worker 路径。托盘只暴露 Show/Open/New/Quit 等
明确动作；没有任意 capability 参数、路径或 renderer 自定义权限。

## Server fact 到 OS notification

`contrib/desktop/lib/notifications.js` 只接受：

- `schema=kiana.desktop-notification.v1`、`source=server`；
- 当前 workspace binding digest；
- 有界 `feed_epoch`、正整数 `sequence`、opaque `notification_id`；
- `approval/pending` 或 `terminal/{completed,cancelled,failed,unknown,result_unknown}`。

Electron 主进程重新校验上述边界和 replay identity，然后把事实映射为固定、脱敏的标题/正文：
通知不显示 workspace path、prompt、artifact 内容、approval payload、token、secret 或 provider
错误原文。`unknown`/`result_unknown` 只提示查询原始 Receipt，不能被画成成功。OS Notification
不可用时返回 `unavailable`，不把 presentation failure 改写成业务终态。

Web SSE 在收到 `approval_requested` 或 terminal event 时，只向 preload 提交 bounded server fact；
preload 不接触 Web bearer token，IPC validator 拒绝错误 binding、未知 kind/status、旧/非法 cursor
和任意额外 payload。通知不是事实源，也不执行 cancel/resume/approve。

## Close / tray policy

关闭窗口仍由用户选择 Keep in background、Quit 或 Cancel。Close prompt 显示服务端已观察到的
pending action、unknown result 和本地未保存草稿，但 `mutation=none`：Keep 只隐藏窗口，Quit
才进入既有 worker process-group stop，窗口关闭本身不 cancel、resume、trust 或 approve。停止
不确认时继续保留 `unconfirmed`，不伪造成功。

## CI-only fixture 与限制

`contrib/desktop/tests/fixtures/ui26-workspace-tray.json` 与
`ui26_workspace_tray.test.js` 覆盖 workspace isolation、notification binding/status/sequence、
fixed redaction、duplicate replay、close attention 和 source guard。`.github/workflows/ui26-desktop-workspace.yml`
运行 Node syntax/fixture、UI-26 deny-first contract、Rust formatting、串行 Web regression 和
workspace test-target compile。

`feature_status=implemented`; `proof_level=source`。未证明真实 Electron/Chromium 多窗口、OS
notification permission/实际送达、托盘进程生命周期、跨重启/多进程 notification dedupe、browser
SSE timing、durable EventLog notification projector、provider/Broker/external effect、live 或 physical
结果；CI 结果保持 pending/unobserved。
