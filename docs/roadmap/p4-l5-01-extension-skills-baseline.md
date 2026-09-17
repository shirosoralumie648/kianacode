# P4-L5-01 扩展与 Skill 授权边界基线

> 快照日期：2026-09-18。运行时验收由 GitHub Actions 执行；本地不运行测试或 smoke。

## Skill metadata is not authorization

`SKILL.md` 的 `allowed-tools` 仍由严格 frontmatter parser 作为有界元数据读取。Daemon 将
它以 JSON 形式标注为 display-only context，且 PromptSection 的 authority 保持
`Context`；它不会增加五工具面、Grant、Policy、sandbox 或 write set。未信任项目的 skill
仍不进入 PromptBundle。

扩展包的 `ExtensionManifest`/`ExtensionExecutionScope` 只提供额外限制：effect、required
capabilities、supported roles、content/package hash 和 network policy。请求在
ControlPlane 准备后进入 Broker，Broker 重新执行 `ExtensionExecutionContract::check`；
read-only 扩展若 handler 被分类为 write 或请求为 shell/process/network，直接返回
`extension_read_only_write_denied`/相应拒绝，不因 `allowed-tools: [shell.exec]` 获得授权。

声明的 required capability 也不是执行入口；能力仍必须通过同一 `DaemonHost → ControlPlane
→ Broker → EventLog` 链路，扩展不创建第二个 Runtime 或模型循环。

## CI-only 验收

```text
cargo fmt --all --check
cargo test -p kiana-daemon --lib skill_allowed_tools_cannot_grant_shell --locked -- --test-threads=1
cargo test -p kiana-domain --test p4_l5_01_skill_allowed_tools --locked -- --test-threads=1
cargo test -p kiana-core --test p4_l5_01_skill_allowed_tools --locked -- --test-threads=1
cargo check --workspace --tests --locked
```

本地只执行格式、workspace test-target 静态编译和 `git diff --check`；不执行测试，也不等待
GitHub CI。

## 限制与交接

- `allowed-tools` 只影响模型上下文的可解释展示，不能证明实际授权或效果；完整 Skill
  catalog/activate/resource-read/invoke 生命周期留 EXT-06..10。
- ExtensionRegistry 当前只为受控本地包/fixture 适配；非 Skill adapter、durable
  snapshot、外部网络和 live/physical effect 仍留 P4-L6/INT/PD/ER/SC。
