# v0.6.1 Context — Parallel Builders on the same core

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: ORCH-04
Chosen slice: two packet Builders on one DaemonHost + path locks, not worktrees / SDK / IDE

## Classify

Phase, not spike. User-visible completion is: the same `DaemonHost` can
run two executing Builders from two WorkPackets. They have independent
sessions. Disjoint `path_allow` both write. Overlapping live path locks
fail closed. Packet `path_allow` now intersects apply_patch policy.
Default single Builder still writes `GOLDEN_PATH.txt`.

This is not git worktrees. This is not an Integrator merge queue. This
is not SDK/IDE/TUI. This is not promoting `kiana-tasks` swarm schema to
the product path. This is not a new CLI flag.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 planning symposium/packet, v0.4
review/MCP/skills/provider, v0.5 departments/RAG/compact/department
symposiums, TUI park, or TeamCreate/SendMessage.

1. **Spawn stays the primitive.** No `RequestBody::Dispatch`, no
   `kiana run --swarm`. Software orchestrator is `tokio::join` of two
   `spawn` calls on the same host. Chair/Queen is not an LLM. Ruflo
   topology here means hierarchical software dispatch, not a Queen
   agent.
2. **Path locks live on ControlPlane.** Empty `path_allow` locks `*`.
   Overlap includes prefix (`src` vs `src/lib.rs`). Conflict error is
   `path_lock_conflict`. Locks release when that spawn returns, even on
   failure. Sequential empty packets still work because the first
   release happens before the second acquire.
3. **Packet `path_allow` intersects policy.** v0.3 recorded it in the
   prompt only. This slice: spawn copies `path_allow` onto
   `RequestContext`; apply_patch outside it is `packet_path_denied` and
   does not write. Direct Builder run (no packet) is unchanged.
4. **No worktrees this slice.** Isolation is path locks in the same
   workspace. `.kiana/swarm-worktrees/{dispatch}/{task}` stays a later
   v0.6 cut. Do not call `kiana-tasks::prepare_swarm_execution` the
   product.
5. **Product proof is DaemonHost.** Same-host tests, not a CLI compile.
   Leave `cli.rs` / `harness_run.rs` alone.

Demo (same-host):

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

## Requirements this phase

ORCH-04. PATH/TRUST/SESS/EVD/ROLE/ORCH-01..03/SYMP-01..04/WB/REV/
CODE-01..04/DEPT-02/MEM-01..04/LONG-02 still true.

## Frozen

- git worktrees / Integrator merge
- `kiana-tasks` swarm schema as the product bus
- ruflo Queen / Raft / BFT / Gossip / shared swarm memory
- TeamCreate / SendMessage
- JointSymposium / staffing every COMPANY.md role / Librarian
- new CLI flags / formatting `cli.rs`
- expanding `kiana-tools`
- SDK/IDE/Desktop/Web as this slice
- live provider / unsupported_streaming
- HTTP/SSE MCP
- migrating TUI
- vector DB / kiana-query as a swarm engine
