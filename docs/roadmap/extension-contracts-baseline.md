# EXT-01 stable extension contracts baseline

> 快照日期：2026-09-16。本文记录 EXT-01 的扩展身份、目录快照、Hook 决策、插件生命周期和错误合同；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`EXT-01`](skills-plugins-hooks.md#step-ext-01) |
| source snapshot | `05c3b288`（EXT-00 baseline closure） |
| feature_status | `implemented`（domain/protocol versioned extension contract source） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | trusted source inventory → typed descriptor/snapshot → protocol projection → ControlPlane/Broker re-check |
| this step does | 新增 ExtensionId/ComponentId/SnapshotId/HookRunId 稳定 UUID 合同；SkillDescriptor、HookDescriptor/Decision、PluginLifecycle、ExtensionSnapshot、ExtensionError 具备版本、边界、digest、重复 identity 和 unknown-field fail-closed 校验；protocol re-export 仅暴露可序列化 contract |
| this step does not | 不解析/执行 SKILL.md、Hook 或插件，不从 allowed-tools/Prompt 产生 capability，不引入 snapshot store、SourceResolver、ProjectTrust adapter、Hook process supervisor 或第二授权路径；这些由 EXT-02+ 负责 |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Stable IDs | `kiana-domain/src/ids.rs` | `9ae2f7066b768e5ab626d41346185bb09a2ee1356a79f2e05df4608144ab87ad` |
| Extension contracts | `kiana-domain/src/extension_contracts.rs` | `d9e1ffaf186be2233c10ae6a7a4248f62aaa0b4ace9805fff716ed75636195a3` |
| Domain registry/export | `kiana-domain/src/contracts.rs`, `kiana-domain/src/lib.rs` | `f15683de266411a3f510fd19ef1f658f881f003d4aa05589579b9b91b91ae7fb`, `1a38cc021715a6cf1a23fb71c8f6048331b4c89329f9ac5afe3f933de26c48ee` |
| Protocol surface | `kiana-protocol/src/lib.rs` | `ca3e6c4badaa74ef4a15def30f05d0b2edca0124a958d19ce080ad2b01c50109` |
| Fixtures | `kiana-domain/tests/ext01_contracts.rs`, `kiana-protocol/tests/ext01_protocol.rs` | `5b0c5ff657b3f2c4b41ce0bd968e116a18bb94c504cf02cd54d4873873e459be`, `a5a2f04a843159a1cd13eef7644d642c5dfa99a157d8142e0971efa3b19888e7` |

hash 只用于 EXT-01 源码漂移复核，不构成 extension load、签名、ProjectTrust、Hook effect 或业务结果证明。

## 2. Contract boundary

稳定 ID 使用 UUID wire shape，并登记到 domain ID contract registry。`SkillDescriptor` 保存 component/version/source/content digest、lifecycle、allowed-tools 和 snapshot 关联；`allowed_tools` 仅是声明，不是 capability grant。`HookDescriptor` 区分 guard/observer phase，`HookDecision` 覆盖 allow/block/ask/update-input/additional-context/timeout/cancelled/unknown，决策携带输入/输出 digest 和来源。`PluginLifecycle` 以 extension ID、版本、包 hash、revision 记录状态。`ExtensionSnapshot` 绑定 generation、trust revision、来源及 descriptor 集合，并通过 snapshot digest 防篡改。`ExtensionError` 使用封闭错误码和 bounded message，不回显 secret/path internals。

所有 descriptor/snapshot 在进入后续 adapter 前执行 `deny_unknown_fields`、长度/字符、digest、嵌套对象、重复 identity 和 generation/revision 校验；snapshot invalidate 只改变投影状态，不撤销已经提交的 EventLog 或 capability permit。

## 3. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `extension_contracts_round_trip_and_reject_unknown_fields` | Snapshot/HookDecision 可 round-trip，未知字段不能进入 versioned DTO |
| `extension_snapshot_digest_and_duplicate_identity_fail_closed` | generation/digest 漂移、重复 skill/hook/plugin identity 被拒绝；invalidate 重算 digest |
| `extension_error_codes_are_stable_and_bounded` | 错误码 union 稳定、message 有界且 unknown field fail-closed |
| `protocol_reexports_versioned_extension_contracts` | protocol 只重新暴露可序列化 domain contract，不提供执行入口 |

`.github/workflows/ext01-extension-contracts.yml` 在 GitHub runner 执行 domain/protocol fixtures 和 fmt；本地只做格式、workspace test-target 静态编译和 diff 检查。

## 4. 限制与交接

- 当前 contracts 是值对象和投影 schema，不代表 Skill/Plugin/Hook 已被加载、签名验证、信任或激活；EXT-02 负责 SourceResolver/ProjectTrust/path root，EXT-03 负责严格 parser。
- `ExtensionSnapshot` 没有持久 store、generation CAS、跨进程恢复或失效广播；旧 snapshot 不能成为授权依据，后续 ControlPlane 必须重新检查 scope、trust、package hash 和 effect。
- protocol re-export 保留 SourceRef 的版本化字段；客户端展示仍需 redaction/summary DTO，不能把 locator、secret ref 或 allowed-tools 当执行权限。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
