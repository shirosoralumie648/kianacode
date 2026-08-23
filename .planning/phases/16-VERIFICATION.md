# v0.5 later Verification — User-visible compact + resume-after-compact

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: LONG-02 (compact on receipt + continue after compact; pause remains cancel)
Chosen slice: compact/resume, not SYMP-04

## Demo contract

```text
trusted Builder + compact budget + over-budget prompt
  → receipt.compact.applied=true
  → tokens_before / tokens_after present
  → summary_present=true
  → tokens_after < tokens_before when discarded history is large
under-budget prompt
  → receipt.compact.applied=false
  → count=0
continue after compact + workspace-write
  → GOLDEN_PATH.txt appears
  → continue receipt still shows compact.applied=true
default Builder can still write GOLDEN_PATH.txt without compact
```

Same-host proof is in-process DaemonHost. Compact is Codex-style history
replacement inside `kiana-runner`, surfaced as `RunnerEvent::Compacted`
and copied onto the run receipt. Pause remains the existing cancel path.
Cross-process live transcript restore is not this slice.

## Evidence

| Criterion | Result |
|---|---|
| Over-budget compact is a receipt fact | `over_budget_run_records_compact_on_receipt`: `compact.applied=true`, `summary_present=true`, `tokens_after < tokens_before` |
| Under-budget does not claim compact | `under_budget_run_does_not_claim_compact`: `applied=false`, `count=0` |
| Resume after compact still writes | `continue_after_compact_still_writes_golden_path` |
| Compact event is protocol-visible | `compacted_event_round_trips_token_counts`; harness `over_budget_history_is_compacted_before_the_model_step` |
| Default write still works | existing `trusted_workspace_write_apply_patch_creates_file` still green |
| No CLI format / no kiana-tools | Did not format `cli.rs`; did not expand `kiana-tools` |

## Commands run

```
cargo fmt -p kiana-runner-protocol -p kiana-runner -p kiana-core -p kiana-daemon
cargo test -p kiana-runner-protocol --locked --lib --offline -- --test-threads=1
cargo test -p kiana-runner --locked --lib --offline -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --locked --lib --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
cargo test -p kiana-protocol --locked --lib --offline -- --test-threads=1
```

All listed targets passed (5 / 26 / 25 / 27 / 41 / 8). Did not
`cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not
compile the CLI crate, did not run `scripts/release-smoke.sh` or live
provider.

## Not claimed

- pause as a new command (still cancel / P0-CANCEL)
- cross-process live-session restore (`session_not_found` stays)
- model-written compact summaries (stub remains `(no summary available)`)
- TUI `/compact` slash command
- `kiana-query` as the compact engine
- SYMP-04 department symposiums
- JointSymposium / staffing every COMPANY.md role / Librarian
- vector DB / letta landing page
- HTTP / SSE / WS MCP
- live provider / `unsupported_streaming`
- TeamCreate / SendMessage
- migrating TUI
- v1.0
