# v0.5 later Verification — Department symposiums

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: SYMP-04 (five department-bounded symposiums; JointSymposium frozen)
Chosen slice: department symposiums, not JointSymposium / RAG auto-ingest

## Demo contract

```text
PM + anti-meeting
  → plan/DECISION.json + packet/TASK.json
  → builder_present=false
sponsor + anti-meeting
  → charter/DECISION.json; no packet/TASK.json
builder chair
  → receipt/DECISION.json; not plan/DECISION.json
reviewer chair
  → gate/DECISION.json
closer chair
  → lessons/DECISION.json
architect chair
  → symposium_chair_must_be_pm
planning attendees still exclude Builder
default Builder can still write GOLDEN_PATH.txt
```

Same-host proof is in-process DaemonHost. Chair is the department's
`can_convene` role. Planning still emits a Builder WorkPacket. Other
departments write that department's decision file only. Decisions are
not auto-promoted into memory JSONL.

## Evidence

| Criterion | Result |
|---|---|
| Five departments can convene | `each_department_can_convene_without_a_joint_meeting`; core `each_department_anti_meeting_writes_its_own_artifact`; daemon `each_department_can_convene_on_daemon_host` |
| Planning regression | existing `anti_meeting_writes_artifacts_without_runner`, `convene_two_rounds_uses_private_speaker_sessions`, `planning_symposium_excludes_builder_and_prompt_is_blackboard_only`, `convene_then_spawn_keeps_architect_on_blackboard_not_pm_transcript` |
| Builder not pulled into planning | planning attendees exclude builder; mixed attendees `joint_symposium_frozen` |
| Architect cannot chair planning | `symposium_architect_chair_fails_closed` / daemon architect chair → `symposium_chair_must_be_pm` |
| Builder chairs executing only | `symposium_builder_chair_writes_executing_decision` writes `receipt/DECISION.json`, not plan/packet |
| Anti-meeting skip still records | skipped status + department decision file; planning still writes packet |
| Default write still works | existing `trusted_workspace_write_apply_patch_creates_file` still green |
| No CLI format / no kiana-tools | Did not format `cli.rs`; did not expand `kiana-tools` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-core -p kiana-daemon
cargo test -p kiana-domain --locked --lib --offline -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-core --locked --lib --offline -- --test-threads=1
cargo test -p kiana-daemon --locked --lib --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
cargo test -p kiana-protocol --locked --lib --offline -- --test-threads=1
```

All listed targets passed (13 / 27 / 0 / 27 / 42 / 8). Did not
`cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not
compile the CLI crate, did not run `scripts/release-smoke.sh` or live
provider.

## Not claimed

- JointSymposium / staffing every COMPANY.md role / Librarian
- auto-ingest of decisions into department RAG (`memory.write` remains explicit)
- new CLI flags (CLI still gates `--symposium` to PM)
- pause as a new command
- TeamCreate / SendMessage
- HTTP / SSE / WS MCP
- live provider / `unsupported_streaming`
- migrating TUI
- v0.6 parallel builders
- v1.0
