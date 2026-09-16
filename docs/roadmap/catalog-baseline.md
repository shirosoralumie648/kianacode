# EXT-04 deterministic catalog and hook-order baseline

> 快照日期：2026-09-16。本文记录 EXT-04 的 catalog 选择、shadowed diagnostics 和 Hook matcher 排序；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EXT-04`](skills-plugins-hooks.md#step-ext-04) |
| source snapshot | `86e93ef`（EXT-03 strict parser 提交后的干净基线） |
| feature_status | `implemented`（domain deterministic catalog + skills Command adapter source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | parsed/trusted extension candidates → domain `ExtensionCatalog` deterministic selection → selected/shadowed projection → runner-compatible Command list |
| this step does | 按 kind/namespace/name/version、precedence、source id、content hash 排序并选择单一 winner；重复 identity 保留 shadowed candidate 和原因，catalog digest 可校验；Hook candidate 按 precedence、matcher specificity、event/matcher/id/source 稳定排序，重复 hook id fail-closed；`load_all_skills_with_trust` 使用 catalog adapter 而不是 filesystem/HashMap 顺序 |
| this step does not | 不决定 ProjectTrust、签名/包 validity、activation、capability grant、Hook match execution 或 snapshot invalidation；catalog 只是可重建 metadata projection，后续 EXT-05+ 负责 generation/失效 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain catalog/order | `kiana-domain/src/extension_catalog.rs` | `0f411397a7b8bd4221eb5d19f41a768b214b6a7cae81c54a4d02ef1c32ccc82a` |
| Domain exports/registry | `kiana-domain/src/lib.rs`, `kiana-domain/src/contracts.rs` | `6f4d6f597df50782283a9b9122bfb79629dda62f89287c80d6c490b0b5dbcb5d`, `e07c8e32fa62c11fabcf73d73ba0478df5696c3ff9ddfc61db7b506f13a6ae34` |
| Skills Command adapter | `kiana-skills/src/catalog.rs`, `kiana-skills/src/lib.rs` | `fcb99c39f2595b820f4c518eddd4a53a3709753113243450dba9404484639fb8`, `01fd03c999db89291d1d41ea7d8c8f2841aba79eb10d156ce02b8623e5c3a72f` |
| Fixtures | `kiana-domain/tests/ext04_catalog.rs`, `kiana-skills/tests/ext04_catalog.rs`, `.github/workflows/ext04-catalog.yml` | `35f2974f454589ab0c96b41aef5c827c672784e2aa4476c2e78af73d16734856`, `45beba8128d6db2a2c20ed6d8c801502bde7bee2097588c6b81bfe7fa973aeda`, `14738b80b06f1e951f015504899c7049241c7f14913c41e4f6462f57e02cd92b` |

hash 只用于 EXT-04 源码漂移复核，不构成加载信任、组件执行、Hook effect、持久 snapshot 或业务结果证明。

## 2. Deterministic catalog

`CatalogCandidate` 绑定 kind、namespace、name、version、content digest、SourceRef 和 source precedence。`ExtensionCatalog::new` 先按 identity、precedence、source id、hash 排序，再为每个 identity 选择一个 winner；其余候选不会静默丢失，而是以 `shadowed_by` 和 `duplicate_identity_shadowed` 诊断保留。Catalog 和 candidate digest 使用 canonical JSON，验证会拒绝 header/digest、selection/shadow 关系和重复 selected identity 漂移。

旧 `Command` 通过 `build_skill_catalog` 映射到 shared `skills` namespace，source scope 变成 precedence，内容/root 生成 digest-derived source id；因此同名 user/project/plugin/bundled skill 的选择不依赖目录遍历或 HashMap 顺序。Catalog 失败时 loader fail-closed 返回空集合，不把不确定候选交给模型。

## 3. Hook ordering

`HookOrderCandidate` 只保留可解释的 id/event/matcher/source/precedence，specificity 按 matcher 中非 wildcard 字符数确定。`order_hooks` 使用 precedence → specificity（更具体优先）→ event → matcher → hook id → source id 的固定 tie-break，输入逆序仍得到同一结果；重复 hook id 在排序前拒绝。排序不执行 Hook，也不替代后续 scope/approval/recursion guard。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `skill_name_collision_is_deterministic_and_audited` | 候选输入逆序时 winner 不变，shadowed candidate/原因保留 |
| `hook_matcher_order_is_replayable` | matcher specificity/tie-break 固定，逆序重放结果一致 |
| `command_catalog_selection_is_stable_and_reports_shadowed_candidates` | 兼容 Command 汇总不依赖来源输入顺序，报告 shadowed |

`.github/workflows/ext04-catalog.yml` 在 GitHub runner 执行 domain catalog、skills adapter fixtures 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 catalog 是进程内可重建 projection；generation、cache invalidation、cross-process snapshot/CAS 和 source cursor 尚未实现，EXT-05 负责。
- SourceRef/precedence 只表达来源声明，不验证 ProjectTrust、签名包、entry contents、Hook process 或 capability；loader 仍返回 legacy Command，EXT-06/09 负责 descriptor/provenance 迁移。
- 同名 skill 的 winner 选择遵循固定 precedence；这不是授权或安全等级，最终模型工具集合仍由 ControlPlane/Broker 决定，allowed-tools 不会扩权。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
