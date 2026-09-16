# EXT-03 strict Skill/Plugin/Hook parser baseline

> 快照日期：2026-09-16。本文记录 EXT-03 的严格文档/manifest parser 与显式兼容适配器；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EXT-03`](skills-plugins-hooks.md#step-ext-03) |
| source snapshot | `6298111`（EXT-02 SourceResolver 提交后的干净基线） |
| feature_status | `implemented`（kiana-skills strict parser + legacy adapter source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | resolved trusted root/resource → strict Skill frontmatter or JSON manifest parse → bounded normalized metadata → later catalog/ControlPlane re-check |
| this step does | Skill parser 拒绝未知 frontmatter、坏类型、超限内容/metadata、非法 context/path/allowed-tools，并规范化目录 slug；Plugin/Hook manifest 使用 bounded duplicate-key JSON parser、deny_unknown_fields strict schema、component/hook id/entry/phase 校验和显式 legacy adapter 标记；兼容结果不携带执行权限 |
| this step does not | 不执行脚本/entrypoint，不验证签名或 package trust，不建立 catalog/generation、Hook supervisor、动态 activation 或 capability authority；这些由 EXT-04+ 负责 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Skill frontmatter/types | `kiana-skills/src/types.rs` | `9fbbd9f4e6db496acc01c9550f9f3e892dba9432b4bfcb3fbffd00cf5d081908` |
| Strict Skill loader | `kiana-skills/src/loader.rs` | `fff35956b89bba2da8a40fb86c65d1f528873bdb74a4b768b118be3bebe056ef` |
| Plugin/Hook manifest parser | `kiana-skills/src/manifest.rs` | `a5a7c466fe7781ad1e1d5e156606269e480ffb0f83ec8fdc6014b7bdba76fabc` |
| Skills exports/dependency | `kiana-skills/src/lib.rs`, `Cargo.lock` | `b5dd371e30b3c68dbf37e79913ba35c4c8a43cc1e0d9e9de0d91f189880d5025`, `0072e7494b1b3cac293267d035d48f393465701ea19eb252168768fffcbf9f20` |
| Fixtures | `kiana-skills/tests/ext03_manifest.rs`, `.github/workflows/ext03-strict-parsers.yml` | `97a5ddcbf06c21c5132c20801983802911704284fa1b69c0dbce0ddfd2bb37c7`, `7930f2c26ea6b2cd60dae7e6bb62b220e0f610687b0989e3b5083c6848a81e29` |

hash 只用于 EXT-03 源码漂移复核，不构成资源信任、签名、激活、Hook effect 或业务结果证明。

## 2. Skill parser boundary

`parse_skill_document` 先限制整份文档大小，再解析 YAML frontmatter。frontmatter 使用 deny-unknown-fields；`allowed-tools` 明确只接受字符串（按空白分隔）或字符串列表，legacy `tools`/`triggers` 作为显式兼容字段单独限额。目录 slug 和 frontmatter display name 分开：目录身份规范化为 ASCII lowercase-hyphen、最多 64 字节；display label 只用于展示，不改变权限。description/body/license/compatibility/version/model、metadata 深度/条目/字符串、context、path pattern 和工具列表均有 bounded validation。解析只返回兼容 `Command`，不能从 allowed-tools 或正文授予 capability。

## 3. Plugin/Hook parser boundary

`parse_plugin_manifest` 使用 domain `parse_bounded_json`，因此 duplicate object key 在 serde 丢弃前就失败。strict `kiana.plugin-manifest.v1` 只允许 name/version/description/components；组件 id 必须唯一，entry 必须是 package-relative path 且非空。旧 manifest 通过显式 `legacy_adapter=true` 分支，仅允许登记的旧字段和 component arrays，仍要求至少一个有效 entry。`parse_hook_manifest` 同样区分 strict/legacy，校验 hook id、event、matcher、guard/observer phase、entry 和重复 id。归一化 parser 只产生 metadata/index，不读取或启动任何入口。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `skill_frontmatter_is_strict_and_names_are_normalized` | 合法 frontmatter round-trip/slug normalization 成功，unknown field、非法 name 被拒绝 |
| `strict_plugin_manifest_rejects_unknown_and_duplicate_components` | strict manifest unknown field、duplicate component id、坏 entry fail-closed |
| `legacy_plugin_and_hook_manifests_require_explicit_adapter_and_entry` | legacy adapter 明确标记；缺少 hook entry 或 malformed component 被拒绝 |

`.github/workflows/ext03-strict-parsers.yml` 在 GitHub runner 执行 parser fixtures、skills test-target compile 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 5. 限制与交接

- 当前 strict parser 仍是 `kiana-skills` 的读取/归一化边界，Command 兼容 DTO 没有完整绑定 EXT-01 SkillDescriptor、SourceResolutionSummary 或 snapshot generation；EXT-04/05/09 负责 catalog/snapshot/provenance。
- legacy adapter 只说明来源格式迁移，不代表旧内容可信、项目已信任、包已签名或组件可执行；后续 SourceResolver/ProjectTrust/signature/ControlPlane 必须再次 gate。
- manifest parser 不做 filesystem entry existence、package root containment 或 process execution；资源安全由 EXT-02 resolver/EXT-07 adapter 和 Broker 负责。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
