# Project Trust 信任根加固设计

日期：2026-07-11

## 1. 目标

本设计落实 `docs/kiana_project_os/08-trust-policy-plugin-mcp-hooks.md` 已批准的“能力不能默认可信”原则，修复两个同源问题：

1. 没有信任决定时，`project_trust_from_app_state` 当前返回 `Trusted`。
2. 项目可携带 `.kiana/trust.json` 给自己授权。

修复后，未知项目必须 fail closed；只有宿主会话中的显式决定或项目树外的用户信任库记录可以授权项目资源。

## 2. 根因与风险

当前 `ProjectTrust` 只有 `Trusted` 和 `Untrusted`，缺失决定被折叠为 `Trusted`。持久化记录又位于被审项目自身目录，因此 clone 一个仓库即可同时带入 hooks、skills、agents、plugins、MCP 配置和“信任声明”。

`SessionStart` 与 `UserPromptSubmit` hook 在模型调用前执行，并通过 shell 启动。这个路径早于普通 Tool permission，因此仅加强 BashTool 或 commercial profile 不能修复信任根。

## 3. 方案比较

### 3.1 方案 A：项目外的逐项目 JSON 记录，推荐

- 路径：`$KIANA_HOME/trust/projects/<project_id>.json`。
- `project_id` 是规范化项目根路径的 SHA-256。
- 每条记录绑定 schema、project id、project root 和 trust decision。
- 不同项目独立写文件，避免一个全局 JSON 的读改写竞争。

优点：边界清楚、易于原子写、无需 TOML 编辑器、项目无法自授权。

### 3.2 方案 B：写入用户 `config.toml` 的 projects map

与 Codex 的用户配置模型接近，但 Kiana 当前缺少保留格式和并发安全的 TOML edit 层。现在采用会扩大改动面。

### 3.3 方案 C：签名的项目内 trust 文件

仍需外部密钥、签名轮换和验签策略，且容易让用户误认为普通项目文件可授权。第一版不采用。

## 4. Trust 状态模型

```rust
pub enum ProjectTrust {
    Unknown,
    Trusted,
    Untrusted,
}
```

语义：

- `Unknown`：没有有效宿主决定；不允许项目资源，不允许 mutating Tool。
- `Trusted`：有效 session decision 或用户信任库明确授权。
- `Untrusted`：有效 session decision 或用户信任库明确拒绝。

只有 `Trusted` 的 `allows_project_resources()` 为 `true`。`as_bool()` 仅用于兼容输出，`Unknown` 返回 `false`；机器接口必须同时读取 `project_trust`，不能仅凭布尔值区分 Unknown 与 Untrusted。

## 5. 项目身份

解析顺序：

1. 将输入 cwd 转为绝对、规范化路径；规范化失败时使用绝对词法路径。
2. 从 cwd 向上寻找最近的 `.git` 目录或文件，作为 Git checkout root。
3. 非 Git 目录使用规范化 cwd 本身。
4. Windows 上对 identity key 做 ASCII lowercase；其他平台保留大小写。
5. 对 identity key 做 SHA-256，得到 64 位十六进制 `project_id`。

不使用项目内 `.kiana`、`Cargo.toml` 或其他可伪造标记决定信任根。嵌套 Git checkout 使用最近的 `.git` 根，避免父仓库信任自动扩散到独立子仓库。

## 6. 用户信任记录

```json
{
  "schema": "kiana.project-trust.v2",
  "project_id": "<sha256>",
  "project_root": "/canonical/project/root",
  "trusted": true
}
```

读取时必须验证：

- schema 完全匹配；
- project id 与当前路径重新计算结果一致；
- project root 与当前规范化根一致；
- 没有未知字段。

任一校验失败时返回读取错误，effective trust 降为 `Unknown`，状态接口报告错误，绝不继续使用旧值。

写入使用同目录临时文件、flush/sync 和 rename；Unix 上信任目录设为 `0700`，记录设为 `0600`。`reset` 删除用户记录并回到 `Unknown`。

## 7. 决策优先级

1. 合法的 session/app-state 显式布尔决定。
2. 合法的用户信任库记录。
3. `Unknown`。

无效字符串不算显式决定。项目内 `.kiana/trust.json` 永远不进入授权优先级。

## 8. Legacy 行为

旧 `.kiana/trust.json` 不自动迁移，因为迁移 `trusted: true` 会继续允许项目自授权。文件保留不删除，但 status 输出必须包含：

- legacy path；
- 是否存在；
- `ignored: true`；
- 原因 `project_local_trust_is_not_authoritative`。

用户需要显式运行 `kiana trust trust`，由命令写入项目外用户信任库。

## 9. 执行门禁

- hooks、skills、agents、project/local plugins、project MCP 继续统一依赖 `allows_project_resources()`；`Unknown` 与 `Untrusted` 均被过滤。
- mutating Tool 在 `Unknown` 或 `Untrusted` 项目中直接 deny；read-only Tool 仍可用于审查未知项目。
- commercial profile 的 managed deny/ask/allow 不能越过 trust gate。
- stream JSON init、app-server 和其他直连表面不得硬编码 `ProjectTrust::Trusted`。

本切片不把 hook 命令迁入 BashTool；trust gate 修复后，已信任项目的 hook sandbox/exec policy 统一化仍是后续独立切片。

## 10. 状态契约

`kiana trust json` 和 `/app/trust/status` 暴露：

- `project_trust`: `unknown | trusted | untrusted`；
- `project_trusted`: boolean 兼容字段；
- `allows_project_resources`；
- `source`: `session | user_store | default`；
- `project_id` 与规范化 `project_root`；
- 用户 trust record 的 path/status/exists/error；
- legacy project file 的 path/exists/ignored/reason。

## 11. 验收标准

1. 无决定项目返回 `Unknown`，项目资源不可见，mutating Tool 被拒绝。
2. 项目自带 `trusted: true` 不改变 effective trust。
3. `kiana trust trust` 只在 `$KIANA_HOME` 写记录，随后资源可见。
4. `kiana trust reset` 删除用户记录并立即回到 `Unknown`。
5. 记录被篡改、路径不匹配或 schema 错误时 fail closed。
6. nested cwd 与同一 Git root 映射到同一记录；嵌套独立 repo 不继承父记录。
7. CLI、app-server、skills、hooks、agents、MCP 和 permission tests 均覆盖 Unknown。

## 12. 不在本切片实现

- MCP server/tool exposure policy。
- 已信任 hook 的统一 sandbox 与 exec policy。
- 企业集中式 trust distribution 或设备签名。
- Windows/macOS 平台级 sandbox 验收。
- 外部客户、发布渠道或线上服务证明。
