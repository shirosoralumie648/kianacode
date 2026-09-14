# EXT-00 Skills / Plugins / Hooks 可复核基线

> 快照日期：2026-09-14。本文是 `EXT-00` 的 source-only 基线与决策回执，不改变产品状态。
> 运行时行为由 GitHub CI 负责；本基线不提升任何组件的 enabled 状态。

## 1. 快照

| 项目 | 记录 |
|---|---|
| source snapshot | `c8e9578ab9e0d8a9c299a27b7ace0e9530f8a8c4`（CM-00 收口提交） |
| worktree 基线 | `master`，工作树干净；本文件 + 账本回填是本步提交 |
| 研究文档 | `docs/skills-plugins-hooks-design-research.md` SHA-256 `5dd6a6f2c1feb8ec8fd12f40091bd598c648db77f7453e119f3c2dee0889d5bc` |
| agent WIP | 无共享 WIP；上次提交后工作树干净 |

| 边界 | 文件 | SHA-256 |
|---|---|---|
| skills 门面 | `kiana-skills/src/lib.rs` | `d729994b0a2ada2b4426e06ca4e62a1b9293ca9f15d7dbbbad23c3f3c37c9527` |
| skills 类型 | `kiana-skills/src/types.rs` | `ddddc5efae6539a6370a93782be0ff72d26727fb06f7b6523f3da9ae5f12ca2e` |
| skills 加载 | `kiana-skills/src/loader.rs` | `b57a20cebad07171b7bcc2a3a4d0a0e3004da5ba013e57219857e5d4b9e99a05` |
| bundled | `kiana-skills/src/bundled.rs` | `ca9576a11d32c59a2a02532ea69eea0654cfb2765a5f4ece76f9c01cfb370e5f` |
| dynamic | `kiana-skills/src/dynamic.rs` | `a6e0a96de5b78d95b053e4ccc50803f23cc4b728aec6d6eba452344bb6aac7a4` |
| plugins | `kiana-skills/src/plugins.rs` | `a67c651651adaa92fdf5443a024df63c9d25aa00b58d4d025f839fcec0bf109a` |
| mcp skills | `kiana-skills/src/mcp.rs` | `61611d67f1f47709bd3294056f3f2d9270d4823586104595e4a1ab0032f90cfe` |
| prompt 注入 | `kiana-daemon/src/harness_skills.rs` | `2f14ba6b59bf8b54b104ed651626efde1558bc4980bdb886feb04bb3bbcd60bd` |
| 扩展注册表 | `kiana-daemon/src/extensions.rs` | `e748b146410572580b286b46a06a39c547d75756f5be8d4df01eb7cd6fa6cc6b` |
| pre-tool hooks | `kiana-daemon/src/pre_tool_hooks.rs` | `2e34e24bfbb1c1a71a438b1a8cc018ad1aeb5b67c2f6166c120a2b95637f61a6` |
| 扩展合同 | `kiana-domain/src/extensions.rs` | `0bbd1ca44fb7f64ab59e958d6732249ef9aa214444ce1468ad929b325f8434ea` |

## 2. 现有行为 / 目标行为 / 未证明（三列矩阵）

| # | 现有行为（source-indexed） | 目标行为（§25.2） | 未证明 / 缺口 |
|---|---|---|---|
| 1 | 无 `SkillDescriptor`；核心类型是 `Command`（types.rs:34-51），无 version/namespace/content-hash 字段；`Frontmatter.version` 解析后从不映射进 Command（loader.rs:113-129） | `source/namespace/name@version` + content hash 唯一身份 | 插件/MCP 靠字符串前缀命名空间（`plugin:skill`、`mcp__server__prompt`），无类型化身份 |
| 2 | 静态缓存 `SKILL_REGISTRY`（lib.rs:27-36）按 `cwd::trust::plugin-roots` 键缓存；失效仅显式 `clear_caches()`；无 generation、无 mtime/hash 校验 | 统一 generation、失效与 provenance | 缓存命中即返克隆，无新鲜度检查；插件装卸靠 cache key 变化触发重扫 |
| 3 | `dynamic.rs` 三张进程级全局表（DYNAMIC/CONDITIONAL/ACTIVATED）；`ACTIVATED_SKILL_NAMES` 只写不读 | 有界、可撤销的动态表 | `activate_conditional_skills_for_paths` **全仓库零调用者**——paths-frontmatter skill 存入后永不激活，导出函数未接产品路径 |
| 4 | `plugins.rs` 无签名验证：manifest 是纯 JSON 解析（plugins.rs:203-206），唯一门是 enabled/disabled 状态文件 | 签名 Skill 包注册 | 签名验证在 `extensions.rs` 的 ExtensionRegistry（`KIANA_EXTENSION_TRUSTED_KEYS_JSON`、`extension_signature_verification_failed`），与 plugins.rs 是**两套并行体系** |
| 5 | `ExtensionRegistry`（extensions.rs）验证签名并持久化，但只有 Skill 类型进 enabled；`register_extension_static`（broker lib.rs:275）**零调用者**，ExtensionHandler 包装是死代码 | Capability/Workflow/Memory/Provider/UI 组件经 adapter 进 enabled | 非 Skill 组件永远停在 staged；manifest 依赖/配置/secret/迁移/回滚无统一绑定 |
| 6 | Hook 定义合并：daemon 侧 env >64KiB 拒绝、相对路径拒绝、8 命令/8192 字节上限、symlink 全组件拒绝、非 PreToolUse phase 拒绝 | 合并快照 + 事件证据 | kiana-query 侧 stop_hooks 合并**不去重**（stop_hooks.rs:1134-1215 三处 extend）；同一命令 user+project+plugin 可执行多次 |
| 7 | daemon PreToolUse：`decide_owned` digest 不匹配 Block（pre_tool_hooks.rs:108-111）；`UpdateInput` 一律 Block `hook_update_input_unsupported`（:269-281）→ `hook_blocked:*` Denied | Ask/UpdateInput 语义、超时、后代清理、重放、PostToolUse | 超时分支（`hook_timed_out`/`hook_deadline_exceeded` 30s/`hook_nonzero_exit`）无 daemon 测试；**语义分裂**：kiana-query 侧 runner 尊重 UpdateInput 改写输入，daemon 侧拒绝——同一配置两种行为 |
| 8 | `allowed-tools` 是 Skill 元数据（types.rs:41,61），kiana-policy 零引用，CLI 仅展示（kiana-commands/src/skills.rs:333,350,589） | 不作为授权事实（P4-L5-01 要求） | **目标已满足**：allowed-tools 不进 policy 已是现状；PromptBundle.extensions 在授权路径零消费（fail-closed 成立） |
| 9 | `ExtensionExecutionScope`（domain extensions.rs:195-203，deny_unknown_fields）：extension_id/package_sha256/content_hash/effect/required_capabilities/network_policy/supported_roles | 最终请求与 capability 使用同一代快照 | 无共享 generation/snapshot id——一致性仅靠 dispatch 时对 registry 事件流做 content-hash 重验证；`_extension_scopes` 标注（harness.rs:1093）与 `admit_extensions`（broker lib.rs:187）无测试覆盖 |
| 10 | `ExtensionManifest::validate`/`ExtensionExecutionContract::check`/`request_extension_scopes`（domain extensions.rs:106/251/220）**零测试**（339 行无 cfg(test)） | 拒绝测试 + 恢复证据 | 测试覆盖：kiana-skills 仅 lib.rs 8 个 + mcp.rs 2 个（loader/types/bundled/dynamic/plugins 零测试）；extensions.rs 726 行零测试；`PromptBundle::decode` 不校验 extensions 内容（后续 check 补上，fail-closed 成立） |

## 3. 交叉引用与重复字段

| 关联卡 | 现状 | 差额 |
|---|---|---|
| `P4-L5-01` | allowed-tools 不进 policy、扩展 scope 仅提示——已满足「不扩大授权」方向 | 缺 broker 侧 read-only 扩展写操作拒绝的显式测试 |
| `P4-L6-01` | ExtensionRegistry 有签名验证 + CAS 事件 | plugins.rs 明文 manifest 加载与之并行；content hash/license/rollback 审计链不完整 |
| `CP-25` | hook Block→Denied 已接 ControlPlane（capabilities.rs:183-198），Ask 合并 approval_requirements 已实现 | hook Ask 与既有 AwaitingApproval 的合并无 core 单测；后代清理/重放无事件证据 |
| `H29` | `skill_context` 注入 run-start system bundle（harness_skills.rs:156-162；harness.rs:445-456），每 ModelRequest 复用 | 动态激活（§2.3）未接产品路径 |
| `CAP-18/20-22` | MCP skills 经 `mcp__server__prompt` 命名空间入目录 | 与 CAP 卡的 stdio MCP 执行链对齐验收未做 |
| `CAP-30/34` | `register_extension_static` 死代码 | 非 Skill 组件 binding 是 staged-only，等待 adapter |

**重复字段/命名冲突**：`Command`（kiana-skills）与 `ExtensionExecutionScope`/`ExtensionManifest`（kiana-domain）是两套身份形状——前者字符串 name + 前缀命名空间，后者 hash + deny_unknown_fields。复用的 port：`PreToolHookPort`（kiana-core/src/commands.rs:36,60,85 持 `Arc<dyn PreToolHookPort>`）、`CapabilityBrokerPort`。

## 4. 架构阻断检查（拒绝验收项）

- **无第二个 runner loop**：extensions/hooks 全部经 `DaemonHost → ControlPlane → Broker` 事实链；hook Block 只能收紧（Denied），不能放开平面已拒绝的请求。✅
- **入口不自行判断权限**：skill/hook 注入不产生授权；`PromptBundle.extensions` 在 policy/gate/approval 路径零消费。✅
- **Prompt 字段不是 capability 来源**：required_capabilities 只是声明；实际执行仍走 prepare→authorize→dispatch。✅

三项均无阻断，EXT-00 可置 ✅；上表 #3/#5/#7 的死代码与语义分裂是 EXT-01+ 的直接输入。
