# P4-M6-01 Desktop 壳基线

> 快照日期：2026-09-18。Node 运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Workspace onboarding and health

Electron 壳只通过 preload IPC 提供 `workspace:state/open/new/continue` 四个入口。欢迎页
明确三步 onboarding：选择 workspace、在 Kiana 工作区内显式信任、再提交任务；没有选择
项目时只在 `~/.kiana/workspaces/project-*` 创建 scratch README。DaemonHost 以 loopback
随机端口启动，stdout 只有检测到 `127.0.0.1` URL 且进程仍存活才视为 ready；启动超时/提前
退出回到欢迎页并显示错误，不打开外部浏览器。

## Tray, background and safe close

窗口 close 是阻止默认关闭的显式决策：Keep in background 隐藏窗口但保留同一 worker/tray，
Cancel 不产生变化，Quit 才进入 `before-quit`→`stopWorker`。worker 以独立进程组启动；
`stopWorker` 先向组发送 SIGTERM，等待父进程及进程组消失，超时才 bounded SIGKILL（Windows
使用 `taskkill /T /F`），无法确认树退出则返回 `worker_stop_unconfirmed`，不宣称安全关闭。
`shutdownComplete` 防止 `app.quit()` 重新进入关闭循环；workspace 切换串行化且旧 worker
停止确认后才启动新 DaemonHost，避免孤儿进程和锁争抢。

## CI-only 验收

```text
node --test tests/p4_m6_01_desktop.test.js
```

本地不执行 Node 测试；只在 GitHub Actions 运行该进程树 fixture 和 source assertions。Rust
workspace 的静态检查沿既有 CI 执行，本 step 不改变产品执行脊柱。

## 限制与交接

- 当前为 Electron/loopback 壳的源代码与 Linux/Windows 进程树行为证据；没有真实桌面会话、
  macOS 原生后端、tray OS 通知、安装/升级签名、崩溃后跨重启 DaemonHost 恢复或 live/physical
  effect 证明。
- 关闭确认只证明 worker 进程树；EventLog/Receipt 仍由 DaemonHost/ControlPlane 权威，UI
  不创建第二个 runtime 或权限判断。
