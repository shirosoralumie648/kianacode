# UI-27 Desktop 安全持久化和 detach 基线

> 快照日期：2026-09-25。Electron/Node 与 Rust 验收由 GitHub Actions 执行；本步骤不在本地
> 运行测试、build、check、clippy 或 smoke，且不等待 CI 结果。

## Metadata-only store

`contrib/desktop/lib/desktop-persistence.js` 定义 versioned `kiana.desktop-store.v1`，允许的
字段只有 layout、draft policy、workspace reference、instance reference、session reference、
last cursor 和 detached 标志。workspace path 是用户选择的本地引用，另绑定 SHA-256 digest；
instance/session/cursor 只保存 opaque identity、epoch、sequence 和 scope digest。递归 key guard
拒绝 token、secret、password、credential、authorization、bearer、private key、access key 和
refresh 等敏感字段，store 不接收 Web bearer 或 Electron instance token。

布局尺寸、路径、ID、epoch、cursor 和 JSON 总大小均有边界；unknown field、schema/revision 漂移、
workspace digest 不匹配、非法 cursor、symlink 目标和损坏 JSON fail-closed。写入先在同一目录
生成临时文件、设置 0600，再 rename 到目标；读取到损坏 store 不会自动启动旧 worker。

## Detach / reattach

worker stop 成功后主进程只把 store 标为 `detached=true`，保留旧引用供诊断；ready 后更新新的
workspace/instance/feed reference 和 snapshot cursor。Web hydrate/terminal 只提交带当前
workspace binding、session、scope digest、epoch 和 sequence 的 server reference，Electron IPC
validator 和 persistence validator 都重新校验后才记录。

`reattachPlan` 只返回 workspace path、新 handshake 要求和 `discard_until_new_handshake`；它不
返回可提交 cursor，也不调用 runner、resume、approval、trust 或 provider。显式 Continue 经过新的
worker stdout/sidecar/health attach，scratch 自动 trust 仅由新建项目动作选择。

## CI-only fixture 与限制

`contrib/desktop/tests/fixtures/ui27-persistence.json` 和 `ui27_persistence.test.js` 覆盖 store
schema/size/unknown/secret/symlink/digest、0600 atomic write、bounded refs、old cursor discard、
reattach no-effect 和 source guard。`.github/workflows/ui27-desktop-persistence.yml` 运行 Node
syntax/fixture、UI-27 deny-first contract、Rust formatting、串行 Web regression 和 workspace
test-target compile。

`feature_status=implemented`; `proof_level=source`。未证明真实 Electron/OS keyring、Windows
ACL/macOS secure storage、跨用户权限、多进程 CAS、崩溃中断 rename、备份/升级迁移、真实浏览器
detach timing、durable EventLog session projector、provider/Broker/external effect、live 或 physical
结果；CI 结果保持 pending/unobserved。
