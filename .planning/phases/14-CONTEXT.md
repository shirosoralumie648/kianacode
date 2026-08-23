# v0.5.1 Context — Five department objects

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirement: DEPT-02

## Classify

Phase, not spike. User-visible completion is: Initiating / Planning /
Executing / Monitoring / Closing all exist as `DepartmentSpec` control-plane
objects at the same time. Default `kiana run` is still executing Builder.
A new initiating `sponsor` can write `charter/` and cannot write src. A new
closing `closer` can write `lessons/` and cannot write src.

P1-READ is **not** this slice. The matrix already locked Codex-shaped search
(`shell` runs `rg`/`ls`/`cat`). Sandbox shell tests already prove that path.
Do not add Read/Grep/Glob tools.

This is not six-layer RAG. `rag_collection` may exist as a name on the
department object. `memory.search` / `memory.write` wait for MEM.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4 review/MCP/skills/
provider degrade, TUI park, JointSymposium, or TeamCreate/SendMessage.

1. **Five departments are catalog objects, not a linear pipeline.** Catalog
   becomes `[initiating, planning, executing, monitoring, closing]`. Existing
   planning/executing/monitoring behavior must not regress.
2. **One role per new department this slice.** `sponsor` (initiating) writes
   `charter/`. `closer` (closing) writes `lessons/`. Do not staff five roles
   per department. Do not add a new CLI flag; `--role` already exists. Do not
   format `cli.rs`.
3. **Default worker unchanged.** Empty / omitted role is still `builder` /
   `executing`. Builder still writes src. PATH-03 still `shell` +
   `apply_patch` + `mcp`.
4. **Product proof is DaemonHost.** Same-host tests, not a CLI compile.
5. **P1-READ skipped.** Matrix §3 P0 search strategy remains shell. Opening
   structured Read/Grep/Glob now would violate that lock.

Demo (same-host):

```text
DepartmentSpec::catalog() has initiating, planning, executing, monitoring, closing
default run → role_id=builder, department_id=executing, can still write GOLDEN_PATH.txt
--role sponsor + workspace-write
  charter/GOAL.md appears; GOLDEN_PATH.txt does not
--role closer + workspace-write
  lessons/LEARNED.md appears; GOLDEN_PATH.txt does not
```

## Requirements this phase

DEPT-02. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01..04 still true.

## Frozen

- six-layer RAG / memory.search / memory.write
- JointSymposium
- TeamCreate / SendMessage
- staffing every COMPANY.md role in every department
- structured Read/Grep/Glob
- live provider / unsupported_streaming
- HTTP/SSE MCP
- migrating TUI
- formatting `kiana-entrypoints/src/cli.rs`
