# v0.3 Phase 3 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
kiana trust .
kiana run --symposium --anti-meeting --sandbox workspace-write --json -- "create GOLDEN_PATH.txt containing hello"
# plan/DECISION.json + packet/TASK.json; builder not seated
kiana run --packet packet/TASK.json --sandbox workspace-write --json
```

Same-host proof is in-process DaemonHost: convene then spawn. Architect user text has blackboard claims, not PM transcript (`Speak as pm`). Anti-meeting writes artifacts without calling the model.

CLI still uses a fresh `DaemonHost` per process. Cross-process continue of a symposium speaker is not claimed.

## Evidence

| Criterion | Result |
|---|---|
| Planning symposium attendees are PM+Architect; Builder never seated | domain `planning_symposium_excludes_builder_and_prompt_is_blackboard_only`; result `builder_present=false` |
| Speaker prompt is agenda + blackboard, not fused transcript | domain speaker prompt; daemon `convene_then_spawn_keeps_architect_on_blackboard_not_pm_transcript` |
| Protocol body is `symposium`, not run/spawn/continue | protocol `symposium_envelope_round_trips_goal_without_transcript` |
| Anti-meeting skips the model and still writes DecisionRecord + WorkPacket | core `anti_meeting_writes_artifacts_without_runner` (UnavailableRunner); daemon capturing model `seen` empty then spawn writes `GOLDEN_PATH.txt` |
| Two rounds use private speaker sessions | core `convene_two_rounds_uses_private_speaker_sessions` → `{id}-pm` / `{id}-architect` |
| Close writes `plan/DECISION.json` + `packet/TASK.json` | core/daemon/CLI files; schemas `kiana.decision-record.v1` + `kiana.work-packet.v1` |
| Chair must be PM | core `symposium_builder_chair_fails_closed`; CLI `symposium_role_builder_fails_closed` → `symposium_chair_must_be_pm` |
| Goal required | core `symposium_empty_goal_fails_closed`; CLI `symposium_without_goal_fails_closed` → `symposium_goal_required` |
| `--anti-meeting` only with `--symposium` | CLI `anti_meeting_without_symposium_fails_closed` |
| `--symposium` exclusive with `--packet` | CLI `symposium_with_packet_fails_closed` |
| CLI anti-meeting writes artifacts without a model script | `symposium_anti_meeting_writes_decision_and_packet` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-protocol -p kiana-core -p kiana-daemon -p kiana-client
cargo test -p kiana-domain --locked --lib -- --test-threads=1
cargo test -p kiana-protocol --locked --lib -- --test-threads=1
cargo test -p kiana-client --locked -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --locked -- --test-threads=1
```

All listed targets passed (8 / 7 / 3 / 21 / 20 / 24). Did not `cargo fmt --all` or format `kiana-entrypoints`.

## Not claimed

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- packet `path_allow` intersecting Builder policy
- eval harness / install.sh as completion (Phase 4, 可后做)
- live provider / physical readiness
- migrating TUI
- CLI same-host symposium (each `kiana` process opens a new DaemonHost)
