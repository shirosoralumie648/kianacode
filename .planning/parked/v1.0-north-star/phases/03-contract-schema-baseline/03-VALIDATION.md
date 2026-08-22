---
phase: 03
slug: contract-schema-baseline
status: draft
nyquist_compliant: true
wave_0_complete: false
created: 2026-08-09
---

# Phase 3 - Validation Strategy

> Phase 3 validates the existing RuntimeEvent and registry contracts. It must not turn fixture coverage into a claim that deferred connector, lifecycle/version, MCP replay, or write-serialization behavior is complete.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Cargo/Rust unit and integration tests plus the repository-local Python JSON Schema validator and Bash schema smoke |
| **Config file** | Workspace `Cargo.toml`; pinned contract at `docs/schemas/kiana-runtime-event.v1.schema.json` |
| **Quick run command** | `cargo test -p kiana-types --test runtime_event_schema --locked --offline` |
| **Full suite command** | `cargo test --workspace --locked --offline --no-fail-fast -- --test-threads=1 && bash scripts/schema-contract-smoke.sh && cargo fmt --all --check` |
| **Estimated runtime** | Focused checks: about 5-120 seconds; full workspace gate is environment-dependent |

---

## Sampling Rate

- **After every task commit:** Run the narrowest affected package/test target and any fixture validator named by the task.
- **After every plan wave:** Run all Phase 3 focused package tests completed through that wave plus `bash scripts/schema-contract-smoke.sh` when schema fixtures are touched.
- **Before `$gsd-verify-work`:** Run the locked/offline serial workspace suite, schema smoke, `cargo fmt --all --check`, and `git diff --check`.
- **Max feedback latency:** 120 seconds for a focused task gate; keep the broad workspace suite as a separate phase gate.

### Wave Sampling

| Wave | Contract slice | Required wave gate |
|------|----------------|--------------------|
| 0 | Existing dirty RuntimeEvent WIP ownership and fixture spine | Repair the extra brace, format the remote adapter test, and prove the two focused tests compile before relying on any Phase 3 result. |
| 1 | RuntimeEvent v1, negative inputs, and legacy session migration | Run the kiana-types contract test, the relevant SDK session tests, and schema/stream-json negative fixtures. |
| 2 | Cross-adapter semantic replay equivalence | Run bridge, remote, and entrypoint replay targets against one canonical fixture/projection while retaining adapter-specific assertions. |
| 3 | Registry discovery and concurrency metadata | Run command/tool discovery projection tests plus focused read-batch/write-exclusion tests, then the complete phase gate. |

---

## Threat Coverage

| Threat Ref | Threat | Automated secure behavior |
|------------|--------|---------------------------|
| `T-03-01` | Unknown, malformed, out-of-bounds, or extra event fields are silently accepted | Serde and pinned-schema negative fixtures fail with explicit diagnostics; validator success for a negative fixture fails the test. |
| `T-03-02` | A replay accepts events belonging to another session or ambiguous sequence identities | Replay tests reject cross-session events and assert deterministic IDs, monotonic sequence, and terminal ordering. |
| `T-03-03` | Raw adapter differences are mistaken for contract drift or silently normalized away | A canonical semantic projection compares shared identity/type/terminal fields while separate adapter assertions preserve raw payload-specific behavior. |
| `T-03-04` | Discovery surfaces expose different command/tool names or permission/concurrency metadata | Tests compare sorted registry-derived projections and fail on unexpected omissions or metadata mismatch. |
| `T-03-05` | Contract tests bypass App authorization | App discovery/replay tests assert unauthenticated access is rejected before validating authenticated payloads. |
| `T-03-06` | Deferred CORE-04 behavior is reported as complete | Tests and verification explicitly list connector, lifecycle/version, MCP replay, decision-event, and write-serialization gaps; requirements remain pending unless the full contract is proved. |

---

## Per-Task Verification Map

The IDs below are the required validation partitions. The planner must preserve them or update this table when final PLAN/task IDs differ.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| `03-01-01` | 01 | 1 | `CORE-01` | `T-03-01`, `T-03-03` | All nine pinned tags round-trip; unknown tags and missing required payload fields are rejected without changing the v1 schema. | contract integration | `cargo test -p kiana-types --test runtime_event_schema --locked --offline` | Existing dirty WIP; Wave 0 repair required | pending |
| `03-01-02` | 01 | 1 | `CORE-01` | `T-03-01`, `T-03-02` | Legacy JSON and JSONL-only session reads preserve messages, ownership, and explicit incompatible-input failures. | persistence integration | `cargo test -p kiana-entrypoints sdk_session_store --lib --locked --offline` | Adjacent tests exist; explicit migration case missing | pending |
| `03-01-03` | 01 | 1 | `CORE-01` | `T-03-01` | Malformed, unknown-schema, bounded-field, extra-key, and stream-json fixtures all fail through their documented typed/schema layer. | negative fixture smoke | `cargo test -p kiana-entrypoints --test cli_stream_json --locked --offline && bash scripts/schema-contract-smoke.sh` | Partial coverage exists; consolidated fixture set missing | pending |
| `03-02-01` | 02 | 2 | `CORE-01` | `T-03-02`, `T-03-03` | Bridge and remote adapters produce equal canonical identity/type/sequence/terminal projections from equivalent inputs. | adapter integration | `cargo test -p kiana-bridge --test runtime_event_adapter --locked --offline && cargo test -p kiana-remote --test runtime_event_adapter --locked --offline` | Bridge exists; remote is untracked WIP | pending |
| `03-02-02` | 02 | 2 | `CORE-01` | `T-03-03`, `T-03-05`, `T-03-06` | Local SDK/App consumers replay the canonical sequence where supported, retain auth, and record MCP replay as a gap rather than inventing an endpoint. | cross-surface integration | `cargo test -p kiana-entrypoints runtime_event --locked --offline` | New focused assertions required | pending |
| `03-03-01` | 03 | 3 | `CORE-04` | `T-03-04`, `T-03-05`, `T-03-06` | CLI/TUI/App command discovery agrees on the visible registry-derived name set; static help and connector gaps remain explicit. | discovery integration | `cargo test -p kiana-entrypoints registry_discovery_consistent_across_surfaces --locked --offline` | New test required | pending |
| `03-03-02` | 03 | 3 | `CORE-04` | `T-03-04`, `T-03-06` | ToolRegistry, MCP tools/list, and doctor tool_parity agree on sorted names/common metadata; safe reads batch and writes stay outside the read batch. | registry/concurrency integration | `cargo test -p kiana-tools registry --locked --offline && cargo test -p kiana-entrypoints mcp::tests::list_tools_exposes_default_registry --lib --locked --offline && cargo test -p kiana-commands doctor_json_reports_tool_parity_snapshot --lib --locked --offline` | Independent tests exist; unified projection/assertions missing | pending |

---

## Wave 0 Requirements

- [ ] Repair and integrate the existing extra closing brace in `kiana-types/tests/runtime_event_schema.rs`; preserve the user's Phase 3 WIP assertions.
- [ ] Format, compile, and assign Phase 3 ownership to the untracked `kiana-remote/tests/runtime_event_adapter.rs`; do not discard it.
- [ ] Add one canonical nine-variant, usage-bearing JSONL fixture plus a semantic projection helper shared by the relevant integration tests.
- [ ] Establish the negative fixture directory/manifest and make unexpected validator acceptance fail closed.
- [ ] Establish sorted command/tool discovery projections without changing registry implementation or relying on `HashMap` order.
- [ ] Keep `Cargo.toml` and `Cargo.lock` unchanged; no new test framework or third-party dependency is required.

---

## Manual-Only Verifications

All Phase 3 behavior is locally automatable. Human review is limited to confirming that the final verification report does not promote deferred CORE-04/CORE-01 gaps to completion; no product-interaction UAT substitutes for the automated contract gates.

---

## Validation Sign-Off

- [x] Every required contract partition has an automated command or an explicit Wave 0 dependency
- [x] Sampling continuity has no three consecutive tasks without an automated focused gate
- [x] Wave 0 owns every current dirty/missing fixture prerequisite
- [x] Commands are non-interactive, locked, offline where Cargo is used, and contain no watch-mode flags
- [x] Security-sensitive ownership, input, authorization, and discovery drift have explicit tests
- [x] `nyquist_compliant: true` is set in frontmatter
- [ ] Wave 0 prerequisites are green and `wave_0_complete: true` is recorded
- [ ] Final workspace/schema/fmt/diff gates pass

**Approval:** strategy ready for Phase 3 planning on 2026-08-09; implementation evidence pending
