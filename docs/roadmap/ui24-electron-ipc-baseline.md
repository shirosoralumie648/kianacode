# UI-24 Electron IPC sender、导航和新窗口 allowlist 基线

> 快照日期：2026-09-25。Electron/Node 验收由 GitHub Actions 执行；本地不运行 Node
> 测试、npm test 或 Electron 进程。

## 受控 IPC 合同

`contrib/desktop/lib/ipc-security.js` 定义了 versioned `kiana.desktop.ipc.v1` 握手和四个
workspace channel 的 allowlist。主进程将每个 BrowserWindow 绑定到 `sender`、顶层
`senderFrame`、当前 loopback/file origin、workspace binding 和单调 nonce。握手只返回短期的
desktop instance token；每次后续调用必须带匹配的 instance token、workspace binding、channel
和下一个 nonce。未知 sender、嵌套 frame、origin/instance/workspace/nonce 不匹配、未知
channel 或版本漂移都在 handler 进入 workspace action 前拒绝。

通过验证的请求只产生 `kiana.desktop.intent.v1`、无路径/URL/凭据的 typed intent，再进入现有
workspace action/DaemonHost 路径；renderer 不能传任意参数、workspace 路径、Web token 或
执行器。instance token 和 binding 是 desktop 进程内短期值，不是 Web bearer credential。

## 导航和新窗口边界

`will-navigate` 只接受当前受信 loopback origin 或本地 welcome 文件；端口、origin、userinfo、
敏感 query/fragment 漂移会 fail closed。`file:`, `javascript:`, `data:`、未知 loopback 和
伪造 workspace/token URL 均被拒绝；token/access-token/auth/api-key/secret 等敏感 query key
也统一拒绝。IPC handshake/envelope 的未知字段同样 fail closed。`setWindowOpenHandler` 对安全的显式 HTTP(S) 外部链接
调用 `shell.openExternal` 并拒绝 renderer 加载；受信 loopback popup 只以
`sandbox/contextIsolation/nodeIntegration` 安全覆盖打开且没有 preload bridge，未知/不受控
新窗口一律拒绝。

BrowserWindow 固定 `sandbox: true`、`contextIsolation: true`、`nodeIntegration: false`、
`webSecurity: true`、`allowRunningInsecureContent: false` 和 `webviewTag: false`。URL 只用于
受信导航分类，不进入日志、IPC response 或 renderer Web token；preload 只暴露四个无参数
typed methods 和短期 desktop envelope。

## CI-only 验收与限制

```text
node --test contrib/desktop/tests/ui24_ipc.test.js
```

Fixture/source guard 覆盖合法/伪造 sender、嵌套 frame、nonce replay、workspace binding、
unknown channel、trusted loopback/welcome、external browser、external token query、
file/javascript/data URL、token-free preload/source 和 Electron security flags。workflow 还做
Node syntax 与 fixture JSON parse。Rust workspace 不由本 step 改动。

当前 proof ceiling 是 `feature_status=implemented`、`proof_level=source`；CI 结果不等待。
未证明真实 Electron renderer、Chromium popup lifetime、跨重启 instance rotation、OS 外部
浏览器、Durable IPC/session、Web token 的 browser memory/DevTools 抽取、provider/Broker effect
或 live/physical proof。workspace action 的事实和授权仍由既有 DaemonHost → ControlPlane
路径负责，Electron 不新增执行循环或权限判断。
