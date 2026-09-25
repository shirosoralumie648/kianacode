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
source_snapshot: current master 2430f50c plus P4-J7-21 structured-output slice and CM-36 fmt dependency
worktree_status: provider codecs expose independent response formats; complete JSON is validated
  and returned separately from text/tool calls; Harness has an explicit OutputRepair entry point
command_argv: GitHub Actions runs cargo fmt --all --check; cargo test -p kiana-provider --lib
  --locked -- --test-threads=1; cargo test -p kiana-core --test
  p4_j7_21_structured_output_guard --locked -- --test-threads=1
cwd/environment: GitHub Actions ubuntu-latest, Rust 1.97.1; local tests/build/check/clippy/smoke
  deliberately not run; `kiana-domain/src/memory_workbench.rs` is included in the workflow path filter
fixture·cassette: provider unit fixtures and core source guard in
  p4-j7-21-structured-output.yml
exit_code: not observed locally; fresh post-CM-36 GitHub CI rerun pending/unobserved
status change: P4-J7-21 source and CI wiring remain 🔄; workflow now requests a fresh remote run after CM-36, no pass is claimed
proof-level change: source plus remote CI wiring only; no local_behavior, durable, live or physical
limitations: provider-specific structured dialects remain limited to the supported request shapes;
  no live provider/schema compatibility or automatic repair behavior is claimed
reviewer: Codex source review; no local runtime test reviewer
```
