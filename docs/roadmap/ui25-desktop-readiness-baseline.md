# UI-25 Desktop readiness、attach 和 worker 生命周期基线

> 快照日期：2026-09-25。Electron/Node 与 Rust 验收由 GitHub Actions 执行；该步骤不在本地
> 运行测试、build、check、clippy 或 smoke。

## Worker 状态与 attach 顺序

```text
stopped → starting → ready → stopping → stopped
                 ↘ failed       ↘ unconfirmed
```

Electron 为每个新 worker 注入一次性随机 nonce，并只接受 stdout 上完整的
`KIANA_DESKTOP_READY={...}` 行。stderr 和普通 URL 输出只是诊断文本，不参与地址发现。
ready DTO 固定 schema、protocol、nonce、PID、canonical workspace、loopback URL、feed identity
和 sidecar lease identity。JSON 有界，未知字段、旧 nonce、PID 不一致、workspace 漂移或非
loopback URL 都会拒绝。

Rust Web worker 在绑定 `127.0.0.1` ephemeral port 后创建现有 `DaemonHost` 和 workspace
`InstanceLease`。sidecar record 与 lock 先落盘，ready 行再携带相同的 instance id、epoch、PID、
workspace/endpoint digest 和 record digest。Electron 从目标 workspace 的 `.kiana/instances`
读取 sidecar，要求 regular non-symlink lock/record、受限目录与文件权限、正确 protocol、当前子进程
PID、workspace/endpoint digest 与重算 record digest。之后只向该 ready record 中验证过的 URL
访问 `/api/health`，并交叉检查 feed identity 和 sidecar identity；探测前后重新验证子进程仍活跃
且 sidecar 未变。UI feed epoch 与 sidecar authority epoch 使用独立字段，避免混成第二个身份源。

```text
spawn(child, nonce, canonical workspace)
  → structured stdout ready
  → protected workspace sidecar ↔ ready record identity check
  → loopback health probe ↔ feed + sidecar identity check
  → bind IPC session and load Web URL
```

worker stop 先 SIGTERM 进程组并等待 leader 与整个组退出，再有界 SIGKILL；Windows 使用有界
`taskkill /T /F`。leader 已退出但 POSIX process group 仍存在、超时或无法确认树退出时状态保留为
`unconfirmed`，不拿可能已重用的 PID/PGID 发送信号，也不清除 worker reference 伪装成已停止。
启动/健康校验失败不加载工作区页面；stop/close 失败不隐式 cancel、resume、trust 或 approve。

## Deny-first coverage 与 CI

`contrib/desktop/tests/fixtures/ui25-readiness.json` 列出 PID 重用/sidecar lock、旧 nonce、错误
workspace/instance/protocol/endpoint、跨实例 health、stderr URL、ready 前 action 和自动 resume
拒绝路径。`ui25_readiness.test.js` 在 GitHub Actions 检查这些路径及源码接线；workflow 还做
Node syntax/JSON、Rust formatting、串行 Web entrypoint regression 和 workspace test-target compile。

CI 命令由 `.github/workflows/ui25-desktop-readiness.yml` 执行。本次只进行目标 Rust 文件格式化
和 `git diff --check`；GitHub CI 结果不等待、不作通过声明。

proof ceiling: `feature_status=implemented`; `proof_level=source`。UI-25 roadmap 保持 `🔄`，直到
CI 与所需实机证据明确。未证明真实 Electron/Chromium renderer、Linux/macOS/Windows 进程树回收、
系统 PID start-time 防复用、跨用户 sidecar 对抗、跨重启 durable lease、端口竞争/重复窗口、升级
兼容、crash/physical process e2e 或任何 provider/Broker 效果。sidecar 和 health 是 attach 身份
证据，不是工作授权；所有工作区命令仍回到既有 DaemonHost → ControlPlane 路径。
