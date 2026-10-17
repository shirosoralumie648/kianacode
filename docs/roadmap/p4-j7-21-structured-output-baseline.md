# P4-J7-21 structured output baseline

## Scope

This slice makes `text`, JSON object, and named JSON schema responses explicit at the
domain/provider boundary. Provider request compilation validates the requested schema and the
connection capability before transport. Each codec keeps response-format handling independent of
tool choice; a tool turn never becomes a structured success.

The provider parser validates a complete response after the protocol terminal marker. Empty,
invalid JSON, schema mismatch, refusal, and length termination have distinct error codes. There
is no hidden JSON repair loop. A caller that wants a repair must explicitly invoke the Harness
with `ModelPurpose::OutputRepair`, creating a separate budgeted attempt.

## CI-only fixtures

- `structured_output_validates_and_is_exposed_separately`
- `refusal_length_empty_and_invalid_json_are_distinct`
- `structured_output_and_tool_calls_are_distinct`
- `response_format_metadata_rejects_empty_schema_names`
- `structured_parser_has_no_implicit_repair_path`
- `p4_j7_21_structured_output_is_explicit_and_fail_closed`

The source guard also checks protocol capability rejection before send, schema validation markers,
the separate `ModelOutput.structured` field, explicit Harness repair entry point, and absence of an
implicit repair loop.

## Evidence

```text
source_snapshot: P4-J7-21 structured-output slice on master 2430f50c plus the 2026-10-17
  completion slice (branch step/p4-j7-21-20261017-1, base master b33515a2) and CM-36 fmt dependency
worktree_status: provider codecs expose independent response formats; complete JSON is validated
  and returned separately from text/tool calls; Harness has an explicit OutputRepair entry point
command_argv: GitHub Actions runs cargo fetch --locked; cargo fmt --all --check; cargo test -p
  kiana-provider --lib --locked -- --test-threads=1; cargo test -p kiana-provider --test
  p4_j7_21_structured_output --locked -- --test-threads=1; cargo test -p kiana-core --test
  p4_j7_21_structured_output_guard --locked -- --test-threads=1
cwd/environment: GitHub Actions ubuntu-latest, Rust 1.97.1; local tests/build/check/clippy/smoke
  deliberately not run; the dedicated workflow is dispatch-only with no path filter after the
  2026-09-27 fan-out consolidation, so the remote receipt is operator-triggered
fixture·cassette: provider unit fixtures, prepared-request fixtures and core source guard in
  p4-j7-21-structured-output.yml
exit_code: not observed locally; GitHub CI receipts not awaited for this completion slice
status change: P4-J7-21 source and CI wiring are complete at the source level; roadmap §1.1 row
  398 🔄 → ✅ (CI pending); no pass is claimed
proof-level change: source plus remote CI wiring only; no local_behavior, durable, live or physical
limitations: provider-specific structured dialects remain limited to the supported request shapes;
  no live provider/schema compatibility or automatic repair behavior is claimed
reviewer: Codex source review; no local runtime test reviewer
```

## 2026-10-17 completion slice (CI-only)

The card's exit conditions are covered at the source level and the closing slice adds:

- `structured_response_format_wire_matrix_is_recorded_per_protocol` records one wire patch per
  supported protocol/format pair (OpenAI Chat, OpenAI Responses, Anthropic Messages, Ollama,
  Gemini) and keeps `text` a byte-identical no-op on every protocol;
  `unsupported_structured_format_is_rejected_before_encoding` pins
  `model_structured_output_protocol_unsupported` (Legacy) and
  `anthropic_json_object_requires_schema` with `request_sent=false` and no partial body patch.
- `unsupported_response_schema_fails_before_send` drives the real
  `ProviderGateway::prepare_call` path on a connection whose model never declared structured
  output: the requested form fails before any socket exists while the same connection still
  prepares `text`.
- `structured_output_and_tools_have_distinct_contracts` asserts the requested schema reaches the
  `response_format.json_schema`, `text.format`, `output_config.format` and `response_format`
  bodies next to an unchanged tool catalog, and that `text` keeps the tools with no structured
  key left behind.
- `refusal_is_not_an_empty_structured_success` and
  `truncated_json_is_never_repaired_silently` keep refusal and length termination distinct from
  empty output and prove the only JSON parser reports partial text as unparseable instead of
  completing it.
- The dedicated manual workflow now runs `cargo fetch --locked`, the `cargo fmt --all --check`
  gate and `cargo test -p kiana-provider --test p4_j7_21_structured_output` beside the lib
  fixtures and the Core source guard; the guard binds the new fixture names, `apply_response_format`
  and the workflow steps, and asserts the prepared-request fixture never completes a call
  (`complete_admitted`/`TcpListener`/`tokio::net` absent).

Status: roadmap §1.1 row 398 `🔄` → `✅` (CI pending) with proof_level `source`. The unified
`ci.yml` shards remain the automatic lane (`kiana-provider` whole-crate plus the
`kiana-core-s5/6` shard that lists `p4_j7_21_structured_output_guard`); the dedicated workflow is
dispatch-only by the 2026-09-27 fan-out consolidation rule. No GitHub receipt had been observed
when this slice was written, so nothing above is `local_behavior`, `durable`, `live` or `physical`.
