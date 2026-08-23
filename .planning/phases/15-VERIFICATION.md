# v0.5.2 Verification

Date: 2026-08-23
Proof: `local_behavior`
Verdict: pass
Requirements: MEM-01, MEM-02, MEM-03, MEM-04
PATH-03: `shell` + `apply_patch` + `mcp` + `memory.search` + `memory.write`

## Demo contract

```text
six layer files exist and do not mix
trusted Builder memory.search collection=project
  → receipt.memory_hits include source; verified=true
trusted Builder memory.search collection=user-private
  → role_knowledge_denied
trusted Builder memory.search collection=planning:unreleased-debate
  → role_knowledge_denied
trusted Builder memory.write collection=instance-scratch
  → record is not in project search
trusted Builder memory.write collection=project
  → role_memory_write_denied
unsourced hit → verified=false
default Builder can still write GOLDEN_PATH.txt
```

Same-host proof is in-process DaemonHost. Memory tools are harness tools
executed by the daemon broker. Collections are partitioned JSONL files.
Chat is not auto-ingested. `rag_collection` on DepartmentSpec is still
only a name. `kiana memory` CLI is not this slice.

## Evidence

| Criterion | Result |
|---|---|
| Six layers exist without mixing | `MEMORY_LAYERS` length 6; `user_and_project_paths_do_not_mix`; project under `{project}/.kiana/memory/`, user/company under `$KIANA_HOME/memory/` |
| Search/write through daemon broker | model tools `memory.search` / `memory.write`; operations match; registered on CapabilityBroker |
| role_id + department_id + grants | core stamps identity; policy filters with `knowledge_grants`; Builder has company/project/role:builder/scratch |
| Hits on receipt with source | `builder_project_search_hits_land_on_receipt`: `memory_hits[0].source` + `verified=true` |
| Unsourced is not verified | `unsourced_memory_hit_is_not_verified` |
| Scratch does not promote | `builder_scratch_write_does_not_promote_to_project`; promote_to=project → `role_memory_promote_denied` |
| Builder cannot read private/unreleased | `builder_cannot_search_user_private_memory`; `builder_cannot_search_unreleased_planning_debate` |
| Builder cannot write project memory | `builder_cannot_write_project_memory` |
| Default worker still writes | existing `trusted_workspace_write_apply_patch_creates_file` still green |
| No CLI format / no kiana-tools | Did not format `cli.rs`; did not expand `kiana-tools` |

## Commands run

```
cargo fmt -p kiana-domain -p kiana-runner -p kiana-policy -p kiana-core -p kiana-daemon
cargo test -p kiana-domain --locked --lib --offline -- --test-threads=1
cargo test -p kiana-runner --locked --lib --offline -- --test-threads=1
cargo test -p kiana-policy --locked --lib --offline -- --test-threads=1
cargo test -p kiana-core --test control_plane --locked --offline -- --test-threads=1
cargo test -p kiana-daemon --locked --lib --offline -- --test-threads=1
cargo test -p kiana-daemon --test daemon_host --locked --offline -- --test-threads=1
cargo test -p kiana-protocol --locked --lib --offline -- --test-threads=1
```

All listed targets passed (12 / 25 / 14 / 25 / 27 / 38 / 8). Did not
`cargo fmt --all`, did not format `kiana-entrypoints/src/cli.rs`, did not
compile the CLI crate, did not run `scripts/release-smoke.sh` or live
provider.

## Not claimed

- vector DB / embeddings / kiana-query as the memory engine
- `kiana memory` CLI or `memory.md`
- auto-ingest of chat transcripts
- Librarian / staffing every COMPANY.md role
- JointSymposium
- compact / resume as a user-visible product
- HTTP / SSE / WS MCP
- live provider / `unsupported_streaming`
- TeamCreate / SendMessage
- migrating TUI
- v1.0
