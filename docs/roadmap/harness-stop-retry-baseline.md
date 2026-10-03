# H05 model stop, error and retry baseline

> 首次快照：2026-09-16；typed recovery 分类切片：2026-10-02（基线 `e17143f7`）；structured-output producer mapping：2026-10-03；tool-call producer classification：2026-10-03。本文记录 H05 的模型 StopReason、错误阶段、retry/recovery 分类和完整性边界；本地不运行测试，运行时夹具仅由 GitHub Actions 执行。

## 1. 目标与证明上限

| 项目 | 记录 |
|---|---|
| roadmap card | [`H05`](harness.md#step-h05) |
| source snapshot | `e17143f7`（H05 typed recovery classification slice base） |
| feature_status | `partial`（typed stop/error/recovery contract + transport retry gate; repair execution remains unwired） |
| proof_level | `source`；静态编译与远程夹具不提升为 local_behavior/durable/live/physical |
| canonical path | Provider response → ModelFinish/ModelStopReason → ModelReply/ModelError/ModelOutcome → Harness transition → RunnerEvent/ControlPlane |
| this step does | 统一 stop reason vocabulary、ModelOutcome、ModelError phase/retry/recovery/side-effect fields；ModelOutcome 与 `run.model_turn` 保留该 disposition；length/refusal/incomplete/unknown stop 在 Harness 完成或工具派发前 fail-closed；Domain retry policy 仅接受 `TransportRetry`，Runner 对未接线 repair disposition 明确拒绝；保留 provider 原始 detail 供诊断 |
| this step does not | 不改变 H06 流式 accumulator、不引入自动 effect retry、不把 provider 传输成功当业务成功、不宣称 live/physical effect |

## 1.1 Source anchors

| 边界 | 文件 | SHA-256 |
|---|---|---|
| Original H05 source snapshot | `kiana-domain/src/model.rs`, `kiana-domain/src/contracts.rs`, `kiana-runner/src/harness.rs`, `kiana-provider/src/response.rs` | `6243275aa959281071e4ecfe6820d6feeda63170f5e41b0ffdc105519bf16510`, `94f8430b9139fc22003197a757d14b3d2ffb47b6a5cb493145078c6be0d124ff`, `06075ed0c651ba946dd65c9ae9989e7c1915dfb2f578ea5324d2c3bde4fe2c82`, `06b416a096671fee2637b5c3c782a7e9042f21a23c7f27fd80b85db6a3dbbe66` |
| Explicit-incomplete source at integrated commit `958ece21` | `kiana-domain/src/model.rs`, `kiana-provider/src/response.rs`, `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `30724bcc1dc42b2129712f92c67cd709597b39c4e17e09c36a00ad37d8a4b702`, `95590f54331f8b86f5ab7651134a932caf5606fddc1eaaaba3daead26d12b301`, `91264fcc6c82a91587db80d4afde52c12611c615abe1def1059824eceaa1cf6f`, `5eb206a2e2241db713fe2c86452e428efb103364548c60d7f480573a50dc2051` |
| Current stop and typed-recovery source/fixtures | `kiana-domain/src/model.rs`, `kiana-provider/src/response.rs`, `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `18d585d9815dcc37992767809dac1dec868fbec1f5f3a4bba355d30878799a82`, `51b4ec0c38382c0d7695ad1070c4f6dcd6d8dc4ac498bbbd1bb1366abb7060679a`, `1bd3810cbbfd2b623cf00f1c5e0971593c848f47fbb2403a4ceb56259c23023f`, `09a167fcd24be51c7316d52709c87d2170eee8a5f8aaf98c44865fc3c5c39559` |
| Typed recovery classification slice | `kiana-domain/src/model.rs`, `kiana-domain/src/retry_policy.rs`, `kiana-domain/src/event_contracts.rs`, `kiana-runner/src/retry.rs`, `kiana-runner/src/harness.rs`, `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `18d585d9815dcc37992767809dac1dec868fbec1f5f3a4bba355d30878799a82`, `c8a123777f93443d30a6838bdf256dffbd8d4c70ca86767f955e7bad5c9093be`, `eeecb9f23a576722031c1608b6140c61b7586cba5c160e3ae5eca4bef05067b4`, `6d864b9101eb28d544d4af2871e924cb40f93ae6849d8926e1d9042b01c203fe`, `24cf1673137fc7e5be3f136750a1b6e3b8107e58a7449f511e2fe18208465ef2`, `1bd3810cbbfd2b623cf00f1c5e0971593c848f47fbb2403a4ceb56259c23023f`, `09a167fcd24be51c7316d52709c87d2170eee8a5f8aaf98c44865fc3c5c39559` |
| Structured-output FormatRepair source and fail-closed fixture | `kiana-provider/src/request.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `10c739e32709937b75625dcad5bdb27cc74e328173813ea7b492514fdd80699b`, `f7b63466280190894837160e5eca74a7383237091ccf160e1bd4e351b62943a7` |
| 2026-10-03 length-stop diagnostic fixture | `kiana-runner/tests/h05_stop_guard.rs` | `4a5cf74bad53313286a47072c4e4790625598795e502f902b96809dd636080dd` |
| Original H05 fixtures | `kiana-domain/tests/h05_model_outcome.rs`, `kiana-runner/tests/h05_stop_guard.rs` | `c570a3cc4fe1f9c7c9ca2a6cd6c50ac1f67d441d0dd74fd02b42718f214e5b34`, `402ed08a46d6222da228635e51cd60d8a507a614fd74caf1c36b7279fc8a7331` |
| Current CI wiring | `.github/workflows/ci.yml`, `scripts/ci/test-shards.json` | `038675197ece4752f335d4265f229aaa422a46df287c9fb94dd6387b3a9d0ef2`, `73fe32c73194833a25a120ea459f57c7c72f283fc77cac1e5a28d91cc4f323b6` |
| Historical standalone workflow | `.github/workflows/h05-stop-retry.yml` | `e458eee8925eb353b6a288c8e47571b16b0906ff540720c3132c6aaa67bb7bab` |

hash 只用于 H05 源码漂移复核，不构成 provider 网络、账单或外部效果证明。

## 2. Typed stop and outcome

`ModelFinish` 仍是 Provider 完整响应的内部完成类型；`ModelStopReason` 是跨 adapter 的闭合分类，额外含 `Unknown`。`ModelOutput::normalized_stop_reason()` 对缺失/未知 stop 返回 Unknown；`ModelReply::legacy` 只有在 `ModelFinish::require_complete()` 通过后才可交给 Harness，并为旧 cassette 缺失 stop 补上明确的 `end_turn`/`tool_use`。

显式 `incomplete` 统一归一化为 `ModelFinish::Incomplete` / `ModelStopReason::Incomplete`，并以 `model_transport_incomplete` 结束 Provider 与 Harness 调用；OpenAI Responses 和 Gemini Interactions 的完整响应状态及流事件均映射到同一错误。包含 tool call 的 incomplete 响应不会形成 `ModelReply`，Runner 也不会发出 `CapabilityRequested` 或 `Completed`。

`ModelError` 统一带 `code / phase / retry_class / recovery_disposition / request_sent / retry_after_ms / safe_message / side_effect_state`；`ModelOutcome` 只输出受限的 stop、retry、recovery disposition、phase、error code 和 side-effect 认知，原始 provider body/headers 不在其中。Structured-output validation errors are emitted after a provider response, so they retain `request_sent=true`, `side_effect_state=none`, and `retry_class=Never`; that records a completed request attempt without claiming a capability effect. A sent transport request with no reliable response remains conservatively `side_effect_state=unknown`.

## 3. Harness and provider gates

- Harness 在处理 `ModelOutput` 后先校验 normalized stop；`length`、`refusal`、`pause`、`incomplete`、Unknown 不会完成 Turn，也不会进入工具队列。
- Provider response parser 对缺 stop、重复 finish、open tool block、截断 SSE、未知 stop 和 refusal 返回稳定 `ModelError`；已有 retry 只允许 `BeforeSend`/`Rejected` 且受剩余 deadline/预算约束。
- `run.model_turn` metadata 同时记录 typed stop reason 和 bounded ModelOutcome；CLI/模型回灌消费的是这一错误分类，诊断 detail 保留但不参与 retry/authority 决策。

## 4. Failure-first fixture matrix

| Fixture | 断言 |
|---|---|
| `length_stop_never_dispatches_tools_or_completes_turn` | Runner 收到带 shell tool call 的 length 响应后发出 `model_output_truncated`，`ModelTurn` 诊断保留 normalized stop 和错误码，且没有发出 `CapabilityRequested` 移交给 ControlPlane/Broker 或 `Completed` |
| `incomplete_stop_never_dispatches_tools_or_completes_turn` | Runner 收到带 shell tool call 的 incomplete 响应后发出 `model_transport_incomplete`，且没有发出 capability handoff 或 `Completed` |
| `length_stop_is_rejected_by_legacy_reply_conversion` | Domain legacy reply conversion 将 length 响应拒绝为 `model_output_truncated`；不单独证明 Runner 派发边界 |
| `explicit_incomplete_stop_is_not_success_even_with_tool_calls` | Domain 将 incomplete 分类为 `ModelStopReason::Incomplete` 并拒绝带工具调用的响应 |
| `explicit_incomplete_statuses_share_one_fail_closed_outcome` | Anthropic、OpenAI Chat/Responses、Ollama 与 Gemini 的显式 incomplete 完整响应均映射为 `model_transport_incomplete` |
| `explicit_incomplete_stream_events_share_one_fail_closed_outcome` | OpenAI Responses 与 Gemini 的显式 incomplete 流事件映射为相同的 typed outcome |
| `refusal_is_not_success` | refusal 归一化为失败，side-effect/retry 语义不伪造成功 |
| `unknown_stop_reason_fails_closed` | 未知 stop 进入稳定 ModelError/Unknown，不猜测 end_turn |
| `normal_text_stop_finishes_and_tool_stop_continues` | end_turn 完成文本，tool_use 继续工具路径 |
| `harness_stop_and_retry_paths_are_typed_and_fail_closed` | Harness/provider 使用 typed stop/retry markers，禁止文本 contains 控制状态 |
| `recovery_dispositions_are_typed_and_survive_error_round_trips` | 五类 recovery disposition 在 ModelError serde round trip 后保持 typed 值 |
| `successful_model_outcome_records_terminal_no_recovery` | 成功 ModelReply 的 ModelOutcome 显式记录 Terminal/no-recovery |
| `model_turn_event_contract_accepts_typed_recovery_outcome` | `run.model_turn` contract allowlist accepts its typed ModelOutcome payload |
| `legacy_model_outcome_without_recovery_disposition_defaults_terminal` | 旧 ModelOutcome 缺字段时按 Terminal 解码 |
| `legacy_model_error_and_retry_observation_fail_closed_without_disposition` | 旧错误缺 recovery 字段时采用 Terminal，旧 RetryObservation 缺 phase/disposition 时策略校验失败 |
| `unknown_recovery_disposition_is_rejected` | 未知未来 disposition serde 解码失败 |
| `retry_policy_denies_non_transport_recovery_dispositions` | 只有 TransportRetry 能进入现有有界 retry policy |
| `typed_recovery_disposition_bounds_runner_routing` | 两种安全 transport 分类按既有策略 retry；format/tool/context repair 明确失败且不重试；FormatRepair/ToolRepair 保留于 `run.model_turn` outcome、只调用一次且不 handoff capability；Terminal 保持原错误 |
| `structured_output_errors_are_typed_format_repair_without_running_a_repair` | Provider structured-output 空/非法 JSON、类型和 schema 内容错误带 `FormatRepair`，同时保留 `request_sent=true`、`side_effect_state=none`、`retry_class=Never`；无效的本地 response schema 保持 `Terminal` 且未发送请求 |
| `malformed_model_tool_arguments_are_typed_tool_repair_after_request` | Provider 已收到响应后，工具参数非法 JSON、非对象或不符合现有工具 schema 的错误标记为 `ToolRepair`，记录 `request_sent=true`、`side_effect_state=none`、`retry_class=Never`；Runner 只做一次模型调用、记录 bounded outcome、以 unavailable 失败且不移交 capability |

`.github/workflows/ci.yml` 在 GitHub runner 的 `kiana-domain-s3/4`、`kiana-runner` 和 `kiana-provider` shards 执行这些 domain outcome、Runner 行为和 Provider parser fixtures；本切片复用现有 `h05_model_outcome` / `h05_stop_guard` targets，不改 shard map。本地不运行测试。

Exact prior H05 receipts: run `37008943358` / head `cc303315`, Runner job `110844252604` passed all 3 `h05_stop_guard` tests, Domain job `110844252435` passed all 5 `h05_model_outcome` tests, and Provider job `110844252490` passed both `explicit_incomplete_*` fixtures. The overall run was later cancelled; each relevant shard was red on sibling targets. H05 production/source/test/shard files were byte-identical from `cc303315` through base `e17143f7`.

Run `37054968622`, head `8d42319c`, executed the new H05 checks: Provider job `110997881483` logged both `structured_output_errors_are_typed_format_repair_without_running_a_repair ... ok` and `malformed_model_tool_arguments_are_typed_tool_repair_after_request ... ok`; Runner job `110997880876` passed all 4 tests in `h05_stop_guard`, including `typed_recovery_disposition_bounds_runner_routing`. Both enclosing crate shards failed on unrelated sibling tests; no full-shard or workflow success is claimed. H05 remains `partial/source`.

## 4.1 Length-stop diagnostic preservation (2026-10-03)

The existing `length_stop_never_dispatches_tools_or_completes_turn` fixture now also requires the
length failure's `RunnerEvent::ModelTurn` metadata to retain normalized stop reason `length` and
bounded error code `model_output_truncated`. Its existing zero-capability-handoff and
zero-completion assertions remain. This pins the card's diagnostic record without changing
runtime behavior, repair disposition, retry or dispatch policy.

```text
source_snapshot: isolated commit `21635be0`; integrated source commit `bf7114b631e51b71360785bda1461689df952682`; `kiana-runner/tests/h05_stop_guard.rs`
worktree_status: CI-only length-stop fixture now checks both failure outcome and retained ModelTurn diagnostics; no production behavior, manifest, lockfile or shard mapping changed
command_argv: isolated `cargo fmt --all --check`; isolated `git diff --check`; root `git cherry-pick 21635be0`; no local tests/build/check/clippy/smoke
cwd·environment: isolated worktree `/tmp/kiana-h05-stop-diagnostic-20261003`, branch `step/h05-stop-diagnostic-20261003`; integration in repository root; Linux/bash; GitHub Actions is the only runtime test executor
fixture·cassette: existing `length_stop_never_dispatches_tools_or_completes_turn` in the `kiana-runner` crate shard; the updated diagnostic assertions await a fresh GitHub receipt after push
exit_code: isolated formatting and diff checks passed; no local runtime result; CI receipt pending
status_change: H05 remains roadmap row 047 `🔄`, `feature_status=partial`, `proof_level=source`; the pre-dispatch denial fixture now pins diagnostic retention
proof-level change: none
limitations: this is fixture-only evidence once CI runs; it does not establish automatic repair/retry, a ContextRepair producer, provider live behavior, billing, external effects or physical behavior
reviewer: source review confirmed the assertions inspect only bounded ModelTurn metadata and preserve the existing zero-handoff/zero-completion checks; no local runtime reviewer
```

## 4.2 Incomplete-stop diagnostic preservation (2026-10-03)

The existing `incomplete_stop_never_dispatches_tools_or_completes_turn` fixture now mirrors the
length-stop boundary: it requires a retained `RunnerEvent::ModelTurn` with normalized stop
`incomplete`, outcome stop reason `incomplete`, and bounded error code
`model_transport_incomplete`, while preserving zero capability handoff and zero completion.

```text
source_snapshot: isolated commit `f27e0c2e8c7505f6f4e016651163d1b7d36e1349`; kiana-runner/tests/h05_stop_guard.rs
worktree_status: fixture-only diagnostic assertions; no production stop/retry/repair behavior, manifest, lockfile or shard map changed
command_argv: isolated cargo fmt --all --check; isolated git diff --check; root cherry-pick f27e0c2; no local tests/build/check/clippy/smoke
cwd·environment: isolated H05 worktree integrated at repository root Linux/bash; GitHub Actions is the only runtime test authority
fixture·cassette: `incomplete_stop_never_dispatches_tools_or_completes_turn` in the existing kiana-runner shard; fresh receipt pending after push
exit_code: isolated format/diff 0; no local runtime result
status_change: H05 remains row 047 🔄 / partial / source; incomplete-stop diagnostic retention is now pinned beside length-stop
proof-level change: none
limitations: no current-head receipt yet; no automatic repair/retry, ContextRepair producer, provider live behavior, external effect, billing or physical proof
reviewer: source review preserved original zero-handoff/zero-completion checks and added only bounded ModelTurn metadata assertions
```

## 4.3 Current-head focused H05 GitHub receipt

Manual workflow `37097488794` at head `31d79ad4` passed all six focused jobs: domain
`h05_model_outcome`, runner `h05_stop_guard`, and the four provider filters for explicit
incomplete statuses, incomplete stream events, structured-output FormatRepair, and malformed tool
arguments ToolRepair. This confirms the deny-first stop/recovery fixtures at this source snapshot.

```text
source_snapshot: `31d79ad4`; .github/workflows/h05-stop-diagnostic.yml; kiana-domain/tests/h05_model_outcome.rs; kiana-runner/tests/h05_stop_guard.rs; kiana-provider/src tests
worktree_status: focused workflow only; no production behavior, manifest, lockfile or unified shard map changed
command_argv: gh run view 37097488794 --json headSha,status,conclusion,jobs,url; cargo fmt --all --check; git diff --check; no local tests/build/check/clippy/smoke
cwd·environment: repository root Linux/bash; GitHub Actions exclusively executed the six targets
fixture·cassette: six jobs all conclusion=success; runner h05_stop_guard and domain h05_model_outcome plus four provider selectors
exit_code: all six remote jobs 0; local format/diff 0
status_change: H05 remains row 047 🔄 / feature_status=partial / proof_level=source; current-head focused stop/recovery evidence is complete for the selected matrix
proof-level change: none; no full-crate green, live provider, billing, external effect or physical promotion
limitations: ContextRepair producer/automatic repair loops, complete unified shard, stream accumulator, budget/live/provider and physical contracts remain open
reviewer: exact GitHub job receipts and source fixture review; no local runtime reviewer
```

## 4.4 Focused H05 acceptance receipt

Manual workflow `37097488794` at head `31d79ad4` completed successfully across all six matrix
jobs: domain `h05_model_outcome`, runner `h05_stop_guard`, and provider selectors for incomplete
status responses, incomplete stream events, structured-output FormatRepair, and malformed tool
argument ToolRepair. This is the first current-head receipt for the newly retained incomplete
diagnostic assertions as well as the existing length and repair boundaries.

```text
source_snapshot: `31d79ad4`; `.github/workflows/h05-stop-diagnostic.yml`; H05 domain/runner/provider source fixtures
worktree_status: focused manual workflow only; product behavior, manifest, lockfile and unified shard map unchanged
command_argv: gh run view 37097488794 --json headSha,status,conclusion,jobs,url; cargo fmt --all --check; git diff --check; no local tests/build/check/clippy/smoke
cwd·environment: repository root Linux/bash; six selected targets ran only on GitHub Actions
fixture·cassette: six jobs all success: h05_model_outcome, h05_stop_guard, explicit_incomplete_statuses, explicit_incomplete_stream_events, structured_output_format_repair, malformed_tool_arguments
exit_code: all six remote jobs 0; local format/diff 0
status_change: H05 remains row 047 `🔄` / `feature_status=partial` / `proof_level=source`; focused current-head matrix has a complete receipt
proof-level change: none; no full-crate, live provider, billing, external effect or physical promotion
limitations: unified shard siblings, ContextRepair producer/automatic repair, stream accumulator, budget/live provider and physical contracts remain open
reviewer: exact GitHub job receipts and source fixture review; no local runtime reviewer
```

## 4.5 Structured Responses ContextRepair producer

The provider boundary now maps only a structured OpenAI Responses failure code,
`error.code=context_length_exceeded`, to `ContextRepair`. The mapping is shared by non-stream
`status=failed` responses and stream `response.failed` events. A message that merely contains the
same words, or another provider error code, remains terminal. The producer records that the model
request was sent, no tool side effect was observed, and the error is not admitted to the transport
retry path.

```text
source_snapshot: integrated commit `022bbf26`; kiana-provider/src/response.rs; .github/workflows/h05-stop-diagnostic.yml
worktree_status: OpenAI Responses structured failure producer and focused selector added; non-stream and stream fixtures include a message-only adversarial case; no automatic repair loop, manifest or lockfile change
command_argv: isolated cargo fmt --all --check; isolated git diff --check; no local tests/build/check/clippy/smoke; root cherry-pick ef0c5efb; git push origin master
cwd·environment: isolated worktree `/tmp/kiana-h05-context-repair-producer-20261003` integrated at repository root Linux/bash; GitHub Actions is the runtime authority
fixture·cassette: `responses_context_limit_errors_are_typed_context_repair`; structured `context_length_exceeded` maps to `model_context_limit_exceeded`/ContextRepair with request_sent=true, side_effect_state=none and retry=Never; message-only same text and non-context provider codes remain Terminal
exit_code: isolated format/diff 0; integration/push 0; focused run `37102779074` all seven jobs success
status_change: H05 row 047 remains 🔄 / feature_status=partial / proof_level=source; one unambiguous OpenAI Responses ContextRepair producer is now source-backed
proof-level change: none
limitations: no automatic repair/compaction loop, no arbitrary provider-message inference, no complete provider matrix, unified shard/live/durable/physical proof
reviewer: isolated provider source/fixture review; no local runtime reviewer
```

## 4.6 Rustdoc doctest boundary correction (2026-10-04)

The full CI runner/protocol jobs exposed documentation-only doctest drift: two Chinese/Unicode
architecture diagrams in `kiana-runner/src/harness.rs` were unlabelled fenced blocks, and the
`ProviderCredentialProbeResponse::digest` prose in `kiana-protocol/src/lib.rs` had indentation that
rustdoc interpreted as code. Commit `424e5321` labels the diagrams as `text` and restores ordinary
prose indentation. No runtime behavior, API, assertion, manifest or lockfile changed.

```text
source_snapshot: `424e5321`; kiana-runner/src/harness.rs; kiana-protocol/src/lib.rs; .github/workflows/h05-stop-diagnostic.yml
worktree_status: two documentation parsing corrections only; `git diff --check` passed; no local test/build/check/clippy/smoke
command_argv: source diff review; git diff --check; gh workflow run h05-stop-diagnostic.yml --ref master
cwd·environment: repository root; GitHub Actions is the runtime test authority
fixture·cassette: focused H05 workflow run `37140646601`, queued after push; no result awaited
exit_code: source/diff checks 0; remote run pending
status_change: H05 remains roadmap row 047 `🔄` / `feature_status=partial` / `proof_level=source`
proof-level change: none; documentation parsing correction awaits remote confirmation
limitations: unified CI remains red from unrelated workspace failures; complete H05 provider/shard/live evidence is still open
reviewer: exact rustdoc diff and queued focused workflow reviewed; no local runtime reviewer
```

## 4.7 Focused CI receipt after doctest correction (2026-10-04)

Run `37140646601` at head `424e5321` completed successfully. All seven focused jobs passed: the
domain `h05_model_outcome` target (12/12), runner `h05_stop_guard` (4/4), and five provider
selectors for incomplete status/stream, structured-output FormatRepair, malformed tool arguments,
and Responses ContextRepair. H05 remains `partial/source`; this receipt does not prove a green
full crate shard, automatic repair/compaction, streaming accumulator, budget, live provider or
physical effect.

## 5. 限制与交接

- 当前错误分类和 stop gate 是本地领域/adapter合同；H06 负责流式分片一致性、H07 预算贯通、H08 静默 I/O 取消。
- Provider structured-output content failures (empty, invalid JSON, shape and schema mismatch) identify `FormatRepair` at their producer. Invalid local response schemas remain `Terminal`. After a model response arrives, malformed tool-argument JSON, non-object arguments and arguments rejected by the existing tool schema identify `ToolRepair`; Runner records the bounded outcome and reports `model_tool_repair_unavailable` without retrying or handing off a capability. OpenAI Responses `error.code=context_length_exceeded` is the first explicit ContextRepair producer for both non-stream and stream failure envelopes; message-only matches and other provider codes remain `Terminal`. Other provider errors default to `Terminal` unless a typed source explicitly marks an eligible transport rejection. The existing dead-code OutputRepair helper remains inactive and no repair loop is introduced.
- TransportRetry still requires the existing `RetryPolicy` class/status/effect checks, per-attempt budget reservation, deadline, observed-delta fence and cancellation-aware delay. Legacy ModelError without a disposition becomes Terminal; unknown values fail deserialization.
- `side_effect_state=unknown` 只表示模型请求/传输边界不确定，不替代 capability attempt 的 effect/stop projection。
- Provider-specific stop detail 和 response IDs 仍只作受限诊断，不能跨 route/model 复用；live provider、账单、外部业务 Outcome 和 physical effect 未证明。
- CI 结果故意不等待；本地不运行测试，proof level 保持 `source`。
