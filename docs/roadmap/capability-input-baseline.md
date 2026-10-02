# CAP-02 capability input boundary and digest baseline

> 首次快照：2026-09-16；provider raw-string/object 参数补充及 ControlPlane authority/scope fixture：2026-10-02；专项 GitHub acceptance workflow：2026-10-03。本文记录 CAP-02 的 bounded JSON/schema、provider shell/argv、patch、MCP、Memory 输入归一化和摘要合同；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`CAP-02`](capability.md#step-cap-02) |
| source snapshot | CAP-02 production and fixture snapshot `302c6b46` includes ControlPlane fixture commit `c609482b`; this acceptance slice is based on `5ec79dca` and adds only a GitHub workflow, its workflow-target source guard, and evidence documentation |
| feature_status | `partial`（bounded input/schema/normalization/digest source exists; the focused acceptance workflow has no CI receipt yet） |
| proof_level | `source`；静态编译不提升为 local_behavior/durable/live/physical |
| canonical path | raw model/direct input → bounded parser/schema validator → canonical CapabilityRequest → PreparedAction input/action/catalog digest → policy/approval/Broker |
| this step does | JSON bytes/depth/items/duplicate key 限制，schema dialect subset，shell string/argv、patch、MCP、Memory 归一化，reserved authority field 清理、alias/NUL/path/numeric 校验及 canonical input digest |
| this step does not | 不把 digest 当授权，不允许展示脱敏占位值进入摘要，不引入网络 `$ref`，不把输入 validator/PreparedAction 当 permit 或 handler effect 证明 |

## 2. Input boundary matrix

| 输入 | canonicalization/validation | failure boundary |
|---|---|---|
| JSON object | `parse_bounded_json` 拒绝重复 key、NUL/非有限数、bytes/depth/node/array/object 超限 | `json_duplicate_key` / `json_bytes_limit` / `json_complexity_limit` |
| JSON Schema | bounded local dialect，unknown keyword、unsupported `$schema`/reference 和非法 numeric bounds 拒绝 | `schema_keyword_unsupported` / `schema_dialect_unsupported` / `schema_number_invalid` |
| shell/process | string 或非空 argv，首 executable 非空、无 NUL；workdir/sandbox/timeout bounded | `action_command_required` / `action_workdir_invalid` / `action_numeric_argument_invalid` |
| apply_patch/path | patch 及 path 必须可解析；relative canonical path，角色/packet scope 后续再交集 | `action_path_invalid` / `action_arguments_invalid` |
| MCP | `tool`/`tool_name` 同时存在必须相等；alias 归一化，arguments 必须 object | `action_tool_alias_conflict` / `action_tool_arguments_object_required` |
| Memory | collection/text/source/query 必填且 bounded；promote_to 由 policy 继续限制 | `action_required_argument_missing` / policy scope deny |
| authority fields | `prepare_capability_action` 删除 caller 的 grant/permit/role/permission fields，再 stamp server context；这些字段不能改 execution scope | `action_not_prepared` / server context mismatch |

`additionalProperties` 由每个 tool/operation schema 显式提供；当前兼容 descriptor 对 server-stamped metadata 保持允许，但 unknown operation/unsupported schema keyword/ambiguous alias 仍 fail-closed。收紧兼容字段必须提升 catalog/binding version并重新审批。

## 3. Digest contract

`canonical_action_input_digest` 对规范化后的 `CapabilityRequest` 计算：

```text
{ operation, binding_version, capability, arguments }
```

它排除 request/correlation ID，使不同入口或重试可以比较同一执行输入；执行影响字段（command、path、server、MCP tool arguments、collection/text 等）变化会改变 digest。`capability_action_digest` 仍包含 catalog digest 与完整 normalized request，用于 action pin、approval hash、permit 和 EventLog；两者都在 redaction 占位值之前计算，不能把 `[REDACTED]` 当输入替代原值。

`PreparedAction` 私有 request/catalog/action/input digest 字段，`validate()` 重新归一化并拒绝 catalog/input/action drift。它只冻结 action，不签发 Grant/Approval/Permit；Broker 仍须以 server EventLog permit 验证后才调用 handler。

## 4. CI-only fixture catalog

| Fixture | Purpose |
|---|---|
| `reserved_authority_fields_are_not_a_server_grant` | caller actor/project/role 等保留字段只会改变待重准备输入，不能成为服务端权限来源 |
| `reserved_authority_fields_cannot_change_execution_scope` | `kiana-core/tests/control_plane.rs` uses a RunnerPort request with actor/project/role/path/sandbox spoofing and a self-valid, action-bound forged scope; the existing ControlPlane preparation and authorization path must deliver the same server-stamped arguments and ExecutionScope to the Broker as an unspoofed request |
| `schema_depth_and_reference_limits_fail_before_dispatch` | depth/bytes/schema reference/keyword 限制先于 Broker |
| `conflicting_mcp_tool_aliases_are_rejected` | MCP alias 冲突 fail-closed |
| `equivalent_json_inputs_have_the_same_digest` | key order/null normalization 后 digest 一致 |
| `execution_affecting_input_changes_change_digest` | 执行影响字段变化导致新摘要 |
| `capability_input_boundary_is_shared_by_runner_core_broker_and_daemon` | 五工具 mapping、core prepare、Broker normalized check、daemon caller into `ShellCommandPlan::from_value`, and shell string/argv byte, NUL, and executable checks |
| `duplicate_json_object_fields_are_rejected_before_value_collapse` | domain raw parser 拒绝重复 key，不让 serde 将冲突值折叠 |
| `duplicate_openai_tool_argument_fields_are_rejected_before_value_collapse` | provider 字符串形式的 tool arguments 复用 bounded parser，在构造 `ModelToolCall` 前拒绝重复 key |
| `object_form_tool_arguments_reject_duplicate_keys_before_model_tool_call` | 非流式 Anthropic/Ollama 对象参数在 Value 构造前拒绝重复 key |
| `streamed_object_form_tool_arguments_reject_duplicate_keys_before_model_tool_call` | Anthropic/Gemini 流帧在保存对象参数前拒绝重复 key |
| `object_form_tool_arguments_accept_nested_values_and_keep_envelope_limit_separate` | >512 KiB 的有效响应 envelope 仍可解析，嵌套 MCP 参数有效 |
| `object_form_tool_arguments_over_the_bounded_input_limit_are_rejected` | 参数本身超过 512 KiB 在产生 ModelToolCall 前被拒绝 |
| `streamed_object_form_tool_arguments_accept_valid_nested_values` | Anthropic/Gemini 流式对象参数保留有效嵌套输入 |

CAP-02 原专属 workflow 于 2026-09-27 合并进 `.github/workflows/ci.yml`。当前 `scripts/ci/test-shards.json` 把 `cap02_input` 分配给 domain shards、`cap02_input_guard` 分配给 `kiana-core-s1/6`；新增的 ControlPlane fixture 复用既有 `control_plane` target（`kiana-core-s2/6`），不改 shard manifest；`kiana-provider` shard 运行 provider 单元夹具。GitHub CI 负责执行，本地不运行测试，不连接 provider/connector。

2026-10-03 acceptance follow-up adds `.github/workflows/cap02-input.yml` as a manual-only workflow. It runs provider raw-string/object duplicate-key and oversized-input rejection fixtures, domain alias/schema/depth/duplicate-key denial fixtures, the ControlPlane forged-authority fixture, the complete domain digest/input target, accepted provider nested-input fixtures, and the existing cross-layer source guard. That source guard pins the selected denial and success target names in the workflow so the documented acceptance surface cannot drift silently. The `workflow_dispatch`-only trigger preserves the repository's automatic-workflow fan-out rule; the unified `ci.yml` remains the push gate and retains ownership of the same test targets. First dispatch run `37044371789` / job `110962037240` failed at the ControlPlane fixture: `ScopeRequestRunner` inherited the default `runner_model_assignment_unsupported` response. Provider denial commands and three domain denial targets passed before that failure. The test runner now explicitly supports trusted model assignment; the workflow must be dispatched again, and CAP-02 remains `partial` / `source`.

## Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Domain input/actions | `kiana-domain/src/tool_catalog.rs`, `kiana-domain/src/actions.rs`, `kiana-domain/src/capabilities.rs` | `9c46d8713d6ce55a8eca707179e566036111b2da285539693aac11426dd480ac`, `fd97036676d046ebf801707f72073ff59de96d9e42b257bee01b5fdf3f22d88c`, `1e8c6cb45c2875fcc35a9c876cc780bc2011913d15db7d4376a8ced1c117b522` |
| Core/Broker/Runner/daemon | `kiana-core/src/capabilities.rs`, `kiana-capability-broker/src/lib.rs`, `kiana-runner/src/tools.rs`, `kiana-daemon/src/harness_capabilities.rs`, `kiana-daemon/src/shell_plan.rs` | `f869c545b6aaf24f494bceb2352ef0a1874ff503de55e1bdf951f5414f7b435a`, `afd4556a453bc6462fb6a66f59f981b67db2ed3041d74869a0f90d145d7d6b28`, `2a757b5ba06622622d949cccacbf50f4caf6d38b04c8806611853e6f4498d213`, `342dcbca27709f41821872c60ab199595f51ebc6fa9dccabbaf0b040ae5c46ce`, `66c21bf720d5014b2b6d3a643ee4d73cd686c1e8f1e066182df4ddc4d54de7fd` |
| Fixtures/guard/historical workflow | `kiana-domain/tests/cap02_input.rs`, `kiana-core/tests/cap02_input_guard.rs`, `kiana-core/tests/control_plane.rs`, historical `.github/workflows/cap02-input.yml` (removed by CI consolidation `08552ada`) | `808fb7152cd8a0d30bbed59a687f7d1f9460091ffb440117e4f13092655f650d`, `54d1aad77469ff8eba6b6e1e7c60443f285c98a066671ba6d0159de1b1691cee`, `a977449a88dcce8a5b99b3a8b00dfdb399ce558099eb7ae2e798a14c26fed377`, `2526a6060cc27c11db4462234d2faa9ce189fadefb41a865ca97b08fd2bd155d` |
| 2026-10-02 provider raw-argument correction | `kiana-provider/src/response.rs`, `kiana-domain/tests/cap02_input.rs`, `kiana-core/tests/cap02_input_guard.rs` | `518ed2fbae7da1580fadaaf7b0ac3ee88dd97eee629eaf442a051e8073f6d2b4`, `6a1ea156037fc4536bff8cd75d2ba4f1de97e943df00883c93cc5847cc0e53eb`, `c4d0c8494095dad30b396f27a0db513f1e6f19938ab3418d6f6782e91a46d98e` |
| 2026-10-02 provider object-form duplicate-key follow-up | `kiana-provider/src/response.rs`, `kiana-provider/src/transport.rs`, `kiana-core/tests/cap02_input_guard.rs` | `0720112e594054bdccd0e9e410e53a24039c71cf60065df56ecbad0d7d2dcfde`, `79a3583f1ade40d7966bebfbd9a030372a2e7aecb1dab69caf4e3ce560d7c5dd`, `b60aada1dcded971ba3f02129e33719855f1d3ba2b85ff2513e1cf099c21a4a0` |
| 2026-10-03 focused acceptance workflow and target guard | `.github/workflows/cap02-input.yml`, `kiana-core/tests/cap02_input_guard.rs` | `2c2de8e4441d1a40f081e474454778b5659c0ec7daabed75d58c6d5897ee3847`, `2804f39bafff46103bd229ef98d5c62db4534590073b223169eb6cf47e08899e` |

2026-10-02 provider ingress correction: string-form arguments use `parse_bounded_json`; object-form parameters are now duplicate-checked from the original provider envelope/frame bytes by `UniqueProviderJson` before they become `Value`. The envelope parser uses the existing 8 MiB transport-body ceiling and does not apply the 512 KiB tool-argument parser limit to the whole response. Tool argument schema validation retains the 512 KiB bound before `ModelToolCall` construction; the existing later `validate_model_calls` boundary still applies its stricter 256 KiB per-call limit. This deliberately fails closed on duplicate object keys anywhere in a provider envelope/frame, including metadata, while preserving the existing ProviderGateway and ControlPlane path.

The follow-up fixtures are in `kiana-provider/src/response.rs` and are executed by the unified `kiana-provider` CI shard; the CAP-02 cross-layer guard is in `kiana-core/tests/cap02_input_guard.rs` (`kiana-core-s1/6`). They cover direct Anthropic/Ollama and streamed Anthropic/Gemini duplicate-key rejection, valid nested MCP input, a valid envelope larger than 512 KiB, and an object argument above 512 KiB rejected before a model tool call is created. Source commit `b930ad87` is cherry-picked to master as `a6d8ba7d` and pushed with its evidence update; GitHub CI was triggered and its result was not awaited.

The core source guard now follows the real daemon boundary: `harness_capabilities.rs` calls `ShellCommandPlan::from_value`, and `shell_plan.rs` bounds and rejects NUL in both string and argv inputs and rejects an empty executable. It no longer expects the handler module itself to contain parser literals. The new `reserved_authority_fields_cannot_change_execution_scope` fixture uses the existing `CoreHarness`/`RunnerPort` and `CapabilityBrokerPort` path. It first captures a trusted Broker request, then sends a request with the same action plus caller actor/project/role/department/session/trust/path/sandbox and authority-field spoofing, along with a self-valid forged `ExecutionScope` whose action binding matches. It checks that ControlPlane overwrites/removes those values and the Broker receives the same normalized arguments and trusted scope.

Exact prior receipts on unchanged CAP-02 source: run `37003572738` / head `1f8e16be`, provider job `110826678310`, passed the raw-string duplicate-key fixture and all five object/stream/size fixtures; the provider shard was red on unrelated `gemini_generate_content_events_are_not_accepted_as_interactions`. Core-s1 job `110826678351` failed the old source guard at its daemon NUL marker assertion. Domain job `110826678459` was cancelled before the CAP-02 target. Earlier run `36999753811` / head `491e6bd7`, domain-s1 job `110814891062`, passed all six `cap02_input` fixtures; that shard failed on unrelated `aut07_workflow_queue_claim`. CAP-02 production source and existing fixture files are unchanged from those receipts through current master; the newly added ControlPlane authority fixture and corrected parser-source guard have no GitHub CI receipt yet.

Remote CI evidence at the raw-string correction snapshot: historical CAP-02 run `36074802434` failed at the repository-wide `cargo fmt --all --check` step, before the CAP-02 fixtures ran. The unified run for `d4a85ebd` was later cancelled during the same fmt gate; no CAP-02 runtime result was observed for that snapshot. The current provider/domain/core targets are assigned to the `kiana-provider`, `kiana-domain-s1/4`, and `kiana-core-s1/6` shards; the object-form follow-up awaits its CI run.

这些 hash 只用于 CAP-02 输入边界漂移复核，不是执行授权、secret 或 handler effect 证明。

2026-10-03 acceptance follow-up adds .github/workflows/cap02-input.yml as a manual-only workflow. Its first test step runs provider raw-string/object duplicate-key and oversized-input rejection fixtures; the next step runs domain alias/schema/depth/duplicate-key denial fixtures and the ControlPlane forged-authority fixture; later steps run the complete domain digest/input target, provider accepted nested-input fixtures, and the existing cross-layer source guard. The workflow_dispatch-only trigger preserves the repository's automatic-workflow fan-out rule. The unified ci.yml remains the push gate and continues to own the same test targets; the focused workflow provides an independently inspectable receipt after dispatch. Run 37044371789 / job 110962037240 failed at `reserved_authority_fields_cannot_change_execution_scope` because its custom ScopeRequestRunner did not implement the newly required trusted `bind_model_assignment` hook. Provider denial commands and the three preceding domain denial targets passed. The fixture now opts into that trusted hook; the workflow will be redispatched after the fix is pushed. CAP-02 remains partial / source. A successful focused run can establish fixture execution for that snapshot only and does not by itself prove durable authorization, correct external effects, live provider behavior, or physical containment.

## 5. 限制与交接

- The focused CAP-02 workflow is manual-only. Its first dispatch found a fixture admission mismatch; the corrected fixture has no CI result yet, so no proof promotion is made here.

- 当前 schema validator 是有界子集，未支持任意 JSON Schema `$ref`、所有组合关键字、MCP outputSchema 或外部 schema fetch；不因 parser 通过宣称完整兼容。
- authority fields 在 ControlPlane prepare 清理并覆写，但 `CapabilityRequest` compatibility JSON 仍可被调用者构造；Grant/Approval/ExecutionContext/epoch 的最终交集由 CP-04+/CAP-03+/SC-04+ 完成。
- shell string/argv 与 patch parser 有局部输入校验，真实文件身份/TOCTOU、网络 endpoint、secret 注入、process stop、external effect/reconcile 仍需 CAP-07+、ER/PD/SC。
- Digest 证明 canonical input 相等/不同，不证明 handler 已执行、结果正确、幂等或业务 Outcome；catalog drift 只在 PreparedAction/Broker boundaries fail-closed。
- Provider object arguments are duplicate-checked before `Value` conversion; the 512 KiB schema-input ceiling remains distinct from the downstream 256 KiB per-call `ModelToolCall` ceiling, the 4 MiB streamed text limit, and the 8 MiB transport-body limit.
- Local tests/build/check/fmt/clippy/smoke are not run; only `git diff --check` is used for whitespace validation. GitHub CI results are not awaited and do not promote local_behavior/durable/live/physical.
