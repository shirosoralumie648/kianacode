# v0.6.1 Verification — Parallel builders on the same core

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: ORCH-04 (two packet Builders on one DaemonHost + path locks; packet `path_allow` intersects apply_patch)
Chosen slice: same-core parallel Builders, not worktrees / SDK / IDE

## Demo contract

```text
join two spawns: ALPHA.txt + BRAVO.txt
  → both files exist; two session_ids; two work_packet_ids
live overlap: second spawn while first still holds ALPHA.txt
  → path_lock_conflict; ALPHA.txt written once
packet path_allow=[ALPHA.txt] + cassette GOLDEN_PATH.txt
  → packet_path_denied; GOLDEN_PATH.txt absent
sequential empty packets still spawn
default Builder can still write GOLDEN_PATH.txt
```

Same-host proof is in-process DaemonHost. Spawn stays the primitive:
`tokio::join` of two `spawn` calls. Path locks live on ControlPlane.
Empty `path_allow` locks `*`. Prefix overlap is a conflict. Locks
release when that spawn returns. Packet `path_allow` is copied onto
`RequestContext` and fail-closes apply_patch outside the allow list.

## Evidence

| Criterion | Result |
|---|---|
| Disjoint packets write in parallel | daemon `disjoint_packet_builders_write_in_parallel`: ALPHA.txt + BRAVO.txt, two session_ids, two work_packet_ids |
| Live overlap fail-closed | core `overlapping_live_spawns_fail_closed_on_path_locks`; daemon `overlapping_live_packet_spawns_fail_closed` → `path_lock_conflict` |
| Packet path intersects policy | policy `packet_path_allow_denies_writes_outside_the_packet`; core `spawn_packet_path_allow_fails_closed_outside_the_packet`; daemon `packet_path_allow_blocks_writes_outside_the_packet` → `packet_path_denied`, no write |
| Sequential empty packets still spawn | core `sequential_empty_packets_still_spawn` |
| Default write still works | existing `trusted_workspace_write_apply_patch_creates_file` still green |
| No CLI format / no kiana-tools | Did not format `cli.rs`; did not expand `kiana-tools` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-policy -p kiana-core -p kiana-daemon
cargo test -p kiana-domain --locked --lib --offline -- --test-threads=1
cargo test -p kiana-policy --locked --lib --offline -- --test-threads=1
cargo test -p kiana-core --locked --lib --offline -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --locked --lib --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
cargo test -p kiana-protocol --locked --lib --offline -- --test-threads=1
```

All listed targets passed (14 / 15 / 0 / 30 / 27 / 45 / 8). Did not
`cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not
compile the CLI crate, did not run `scripts/release-smoke.sh` or live
provider.

## Not claimed

- git worktrees / Integrator merge queue
- `kiana-tasks` swarm schema as the product bus
- ruflo Queen / Raft / BFT / Gossip / shared swarm memory
- TeamCreate / SendMessage
- JointSymposium / staffing every COMPANY.md role / Librarian
- new CLI flags / formatting `cli.rs`
- expanding `kiana-tools`
- SDK / IDE / Desktop / Web as this slice
- live provider / `unsupported_streaming`
- HTTP / SSE / WS MCP
- migrating TUI
- vector DB / kiana-query as a swarm engine
- v1.0
