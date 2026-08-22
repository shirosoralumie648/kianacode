# v0.3 Phase 2 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass

## Demo contract

```bash
kiana trust .
# packet/TASK.json is a kiana.work-packet.v1 assigning builder
kiana run --packet packet/TASK.json --sandbox workspace-write --json
# receipt: new session_id, role_id=builder, work_packet_id set, file from goal appears
```

Same-host proof is in-process DaemonHost: planner session first, spawn second, planner secret absent from builder model messages.

CLI still uses a fresh `DaemonHost` per process. Cross-process continue of a spawned worker is not claimed.

## Evidence

| Criterion | Result |
|---|---|
| WorkPacket is the domain contract (`kiana.work-packet.v1`) | domain `work_packet_prompt_is_only_packet_fields`; protocol `spawn_envelope_round_trips_packet_without_transcript` |
| Spawn is a new protocol body, not Continue | protocol spawn round-trip; `RequestBody::Spawn` |
| Same host, new session | daemon `spawn_builder_from_packet_does_not_copy_planner_transcript`; core `spawn_from_packet_starts_a_fresh_builder_session` |
| Occupied session fail-closed | core/daemon `spawn_reuses_of_a_live_session_fail_closed` → `spawn_session_not_fresh` |
| Worker user prompt is packet fields only | capturing model: builder user text has `Work packet wp-1` and no `PLANNER_SECRET_TOKEN` |
| CLI `--packet` writes from cassette | `packet_spawn_creates_golden_path_without_prompt`; file `hello\n`; receipt `role_id=builder` `work_packet_id=wp-1` `input=work_packet` |
| Unknown packet path fail-closed | CLI `packet_unknown_path_fails_closed` → `packet_not_found` |
| Packet + prompt fail-closed | CLI `packet_with_prompt_fails_closed` → `packet_prompt_conflict` |
| `--role pm` with packet fail-closed | CLI `packet_role_pm_fails_closed` → `packet_role_must_be_builder` |
| Parent packet path fail-closed | CLI `packet_parent_path_fails_closed` → `packet_path_denied` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-policy -p kiana-protocol -p kiana-core -p kiana-daemon -p kiana-client
cargo test -p kiana-domain --locked --lib -- --test-threads=1
cargo test -p kiana-protocol --locked --lib -- --test-threads=1
cargo test -p kiana-client --locked -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
cargo test -p kiana-entrypoints --test cli_run --locked -- --test-threads=1
```

All listed targets passed (7 / 6 / 2 / 16 / 18 / 19). Did not `cargo fmt --all` or format `kiana-entrypoints`.

## Not claimed

- Symposium / DecisionRecord / blackboard (Phase 3)
- packet `path_allow` intersecting Builder policy
- five departments / six-layer RAG
- TeamCreate / SendMessage
- eval harness / install.sh as completion (Phase 4)
- live provider / physical readiness
- migrating TUI
- CLI same-host spawn (each `kiana` process opens a new DaemonHost)
