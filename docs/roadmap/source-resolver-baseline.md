# EXT-02 SourceResolver / ProjectTrust / path-root baseline

> 快照日期：2026-09-16。本文记录 EXT-02 的来源根解析、ProjectTrust 过滤和 resource containment；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EXT-02`](skills-plugins-hooks.md#step-ext-02) |
| source snapshot | `c1d59b7`（EXT-01 稳定扩展合同提交后的干净基线） |
| feature_status | `implemented`（kiana-skills SourceResolver + loader path gate source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | cwd → actual canonical project root → source root validation → ProjectTrust decision → trusted path list/resource containment |
| this step does | 统一 User、KIANA_HOME、Project、Bundled、SignedPackage、PluginComponent、External source root kind/precedence；生成不含绝对路径的 SourceResolutionSummary/source digest；资源路径拒绝绝对路径、`..`、反斜杠、控制字符、symlink 和 root 外 canonical path；untrusted project root 保留 denied decision 但不进入 trusted paths；loader 复用 resolver 并拒绝 symlink skill root/resource |
| this step does not | 不解析 SKILL.md/Plugin manifest、不验证签名或 package 内容、不激活 Hook、不建立 snapshot store 或 capability grant；这些由 EXT-03+ 负责 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| SourceResolver | `kiana-skills/src/source_resolver.rs` | `07e948d22f8074e08b6d45c1f54ddbcd6662bc263ddc789439a7d566f79a6597` |
| Loader integration | `kiana-skills/src/loader.rs` | `1ed973c96baf21f1d493de4f7071a616e6d90f8529b2af929058409762ee470d` |
| Skills exports/dependency | `kiana-skills/src/lib.rs`, `kiana-skills/Cargo.toml`, `Cargo.lock` | `4ae39784e83723d1f4c6d107f15e47cbc53aca7b4a97bfa88f107b3b4250d8c8`, `58cf1f4aead5c1e48e4db0be67ff03a41f5ee09cc293552bdd75a72b1a8cf07e`, `0072e7494b1b3cac293267d035d48f393465701ea19eb252168768fffcbf9f20` |
| Fixtures | `kiana-skills/tests/ext02_source_resolver.rs`, `.github/workflows/ext02-source-resolver.yml` | `efe0ab0730285e637666e7203018be761ac2e38370510f8bebfe1ebf9fede094`, `869c4b8290d0785e40bc2a23ea08aedb5d6ed407e6cb3b8d30bef57bcf85c802` |

hash 只用于 EXT-02 源码漂移复核，不构成项目资源加载、签名、Hook effect、外部副作用或业务结果证明。

## 2. Resolver contract

`SourceResolver` 首先 canonicalize cwd 和 `project_trust_root`，再枚举标准 root 与显式附加 root。每个 root 经过绝对路径、`ParentDir`、regular-directory 和 symlink-root 校验；canonical root 重复直接返回 `DuplicateRoot`，不静默 dedupe。来源 kind 有稳定 precedence，summary 使用 digest-derived source id/root digest，不把物理绝对路径暴露给 summary/protocol。

Project root 的 trust 只来自显式 `ProjectTrust`：Trusted 才进入 `trusted_paths`，Unknown/Untrusted 只保留不可用的 source decision。Signed/Bundled 属于已声明可信来源；External 默认 Unknown，必须由后续受控 adapter 显式提升。SourceRef 只证明来源定位和 digest，不能授权 capability。

`resolve_resource(root, relative)` 只接受 root 内相对 POSIX path，逐级检查 symlink，再 canonicalize 并确认 containment；失败返回结构化 `SourceResolveError`。loader 的目录扫描和 plugin 单 skill 读取复用这条 guard，缺少 `SKILL.md` 返回空结果，symlink/越界不跟随。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `untrusted_project_source_is_reported_and_not_usable` | 未信任项目源产生 Untrusted decision，不出现在 trusted paths，summary 不泄露绝对路径 |
| `duplicate_roots_are_rejected_instead_of_silently_deduped` | canonical duplicate root 返回 DuplicateRoot |
| `resource_resolution_rejects_escape_and_symlink` | safe resource 可读取；`..`、绝对路径和 symlink 资源全部拒绝 |

`.github/workflows/ext02-source-resolver.yml` 在 GitHub runner 执行 resolver filesystem fixtures、loader compilation/fixture 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 4. 限制与交接

- 当前 SourceResolver 只产生来源/信任路径投影，不验证 ProjectTrust 存储本身、签名包 key、manifest schema、组件依赖或 source content hash；EXT-03/19+ 负责严格 parser/供应链。
- `SourceResolver::resolve` 的标准 user/KIANA_HOME roots 仍由环境决定；缺失目录被跳过，symlink/duplicate fail-closed；跨进程 generation、snapshot CAS、失效/恢复和 EventLog facts 尚未实现。
- loader 仍返回兼容 `Command`（含内部 `skill_root`），尚未把其完全映射为 EXT-01 `SkillDescriptor` 或 redacted protocol view；allowed-tools 仍只是声明，不能授权。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
