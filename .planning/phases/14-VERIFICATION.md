# v0.5.1 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirement: DEPT-02
PATH-03: unchanged (`shell` + `apply_patch` + `mcp`)

## Demo contract

```text
DepartmentSpec::catalog() has initiating, planning, executing, monitoring, closing
default run → role_id=builder, department_id=executing, can still write GOLDEN_PATH.txt
--role sponsor + workspace-write
  charter/GOAL.md appears; GOLDEN_PATH.txt does not
--role closer + workspace-write
  lessons/LEARNED.md appears; GOLDEN_PATH.txt does not
```

Same-host proof is in-process DaemonHost. Five PMP process groups exist as
catalog objects at the same time. One role per new department: `sponsor`
writes `charter/`; `closer` writes `lessons/`. Default worker is still
executing Builder. P1-READ was skipped (matrix locked Codex-shaped `shell`
search). `rag_collection` is a name on the department object, not a working
memory tool.

## Evidence

| Criterion | Result |
|---|---|
| Five departments exist together | `v0_5_catalog_has_five_departments_without_changing_default_worker` |
| Default worker unchanged | empty role / department still `builder` / `executing`; existing write test still green |
| Sponsor writes charter, not src | `initiating_sponsor_can_write_charter_but_not_source`: `charter/GOAL.md`; GOLDEN_PATH denied `role_path_denied` |
| Closer writes lessons, not src | `closing_closer_can_write_lessons_but_not_source`: `lessons/LEARNED.md`; GOLDEN_PATH denied |
| Department objects have mission/artifacts/gates | `DepartmentSpec` fields: `pmp_group`, `mission`, `artifacts`, `rag_collection`, `gates` |
| Not RAG / not full staffing | `rag_collection` is a name string; did not add `memory.search`; did not staff every COMPANY.md role |
| P1-READ skipped | Did not add Read/Grep/Glob; PATH-03 unchanged |
| No new CLI flag | Did not format `cli.rs` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-daemon
cargo test -p kiana-domain --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --locked --lib -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked -- --test-threads=1
```

All listed targets passed (11 / 25 / 32). Did not `cargo fmt --all`, did not
format `kiana-entrypoints/src/cli.rs`, did not compile the CLI crate, did not
run `scripts/release-smoke.sh` or live provider.

## Not claimed

- six-layer RAG / `memory.search` / `memory.write` (MEM next)
- JointSymposium
- staffing every COMPANY.md role in every department
- structured Read/Grep/Glob
- HTTP / SSE / WS MCP
- live provider / `unsupported_streaming`
- TeamCreate / SendMessage
- migrating TUI
- v1.0
