# EXT-05 extension snapshot cache and invalidation baseline

> 快照日期：2026-09-16。本文记录 EXT-05 的 snapshot cache key、generation 和显式失效边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EXT-05`](skills-plugins-hooks.md#step-ext-05) |
| source snapshot | `ef10992`（EXT-04 deterministic catalog 提交后的干净基线） |
| feature_status | `implemented`（kiana-skills cache key/generation/invalidation + resolver content fingerprint source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | cwd/trust/source-root content/package/config/schema inputs → SnapshotCacheKey → immutable cache entry → explicit invalidate/new generation → legacy skill registry projection |
| this step does | `SnapshotCacheKey` 绑定 cwd digest、trust decision、source roots digest、package registry generation、config digest、schema version；`ExtensionSnapshotCache` 只复用未失效 entry，重复 insert 不覆盖现有 entry，invalidate 保留诊断并递增 generation；resolver root fingerprint 读取 bounded resource content，dynamic/conditional/clear cache 触发全局 generation；legacy `load_all_skills_with_trust` 使用 resolver/cache generation key |
| this step does not | 不把 cache/snapshot 作为授权来源，不持久化 EventLog，不实现 package signature/manifest/parser、Hook process、activation、cross-process CAS、notification 或外部 effect；后续 EXT-06+ 负责完整 snapshot adapter/失效传播 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Snapshot cache | `kiana-skills/src/snapshot.rs` | `03bb7e51d4f1510963fc7a8fb2c867980a593217215996df2abb0ebebe74281b` |
| Resolver content fingerprint | `kiana-skills/src/source_resolver.rs` | `25bf0799fe8a8d6269db77df26f2e1cccfb9fec8e67d07717996598896804cb3` |
| Registry/dynamic integration | `kiana-skills/src/lib.rs`, `kiana-skills/src/dynamic.rs` | `828f2cbc078b7ea6d5acf476181f9a85cb639dfeec7ddcbd96d42e3322eddd19`, `211da52dd14febce1b9c2c756ea9cfa92fea8ecce7632fc2124e4199c9af7bfd` |
| Domain schema registry | `kiana-domain/src/extension_contracts.rs`, `kiana-domain/src/contracts.rs` | `3548896c475578a8710e71dca28b1132add698ffbcd5a523cca7606f5851bd75`, `d098dcb9ee875695d7ba8650531423989dfce2b0e43d10860c5c1037798db7a4` |
| Fixtures | `kiana-skills/tests/ext05_snapshot.rs`, `.github/workflows/ext05-snapshot.yml` | `b48b99000471dcd2de6b5e30da4cded8912ed65b879881f1cab06e6c68a3aee9`, `3ce9008992836f5dbba8bb767b9d10cd3cac470b05c724b0b59e823d36c71567` |

hash 只用于 EXT-05 源码漂移复核，不构成 durable snapshot、权限、Hook effect 或业务结果证明。

## 2. Cache key and generation

`SnapshotCacheKey::new` 对 cwd 做 digest，不把绝对路径带入 key projection；trust、root-set digest、package registry digest、config digest 和 schema version 均参与 canonical key digest。任一输入变化会产生不同 key。`ExtensionSnapshotCache` 以 key digest 索引不可变 snapshot identity：相同 key 的重复 insert 返回原 entry，只有明确 invalidate 后才允许新 snapshot/generation；失效 entry 不再由 `get` 返回，但保留 `Invalidated` 状态和原因用于诊断。

`kiana-skills` 的兼容注册表把 SourceResolver 的 root-set/content fingerprint、插件 root digest、system prompt config digest 和全局 dynamic generation 编入缓存 key。动态 skill 注册、条件激活/清理和显式 `clear_caches` 都递增 generation，避免旧 Command 列表跨代复用。缓存没有 capability 权限，最终工具请求仍回 ControlPlane/Broker。

## 3. Source/content invalidation

SourceResolver 先校验实际 root，再以 bounded 深度/文件数/文件大小读取 resource digest；同一路径文件内容变化会改变 root-set digest，即使 mtime 未变也不会复用旧 key。symlink、非 regular resource、超限树和 root escape 保持 fail-closed。摘要仍只包含 digest-derived source id、trust、precedence 和 root digest，不暴露绝对路径。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `snapshot_cache_key_binds_trust_sources_registry_and_config` | trust/source/package/config 任一变化都会改变 key digest |
| `snapshot_cache_reuses_only_current_entries_and_invalidates_monotonically` | 当前 entry 可读/标记 Used，invalidate 后不可读、保留原因，新 insert 使用更高 generation |
| `source_root_fingerprint_changes_when_resource_content_changes` | 同 root resource 内容变化产生新的 root-set digest |
| `dynamic_invalidation_advances_global_generation` | 动态/显式失效递增全局 generation，旧 registry key 不复用 |

`.github/workflows/ext05-snapshot.yml` 在 GitHub runner 执行 cache/resolver fixtures、skills compile 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 cache 仍是进程内投影，未持久化 snapshot/manifest、generation CAS、cross-process lock、崩溃恢复或 EventLog cursor；不能声称 durable。
- package registry generation 目前使用插件 root/config digest 兼容代替，真实 ExtensionRegistry lifecycle/upgrade/revoke 与签名事实仍需 EXT-19+。
- resolver content fingerprint 是 bounded 文件摘要，不替代 package content hash、ProjectTrust 存储完整性或 Hook/Skill parser；超出边界按拒绝处理。
- cache invalidate 只阻止后续 projection reuse，不撤回已发送 provider/context 内容，也不改变已提交 capability/event；这些由 CM/ER/SC 的治理和 exposure 事实负责。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
