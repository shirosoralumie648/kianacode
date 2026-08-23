# v0.5.2 Context — Six-layer RAG ACL

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: MEM-01, MEM-02, MEM-03, MEM-04

## Classify

Phase, not spike. User-visible completion is: six memory layers exist as
separate stores; `memory.search` / `memory.write` are harness tools executed
by the daemon broker; every request carries `role_id` + `department_id` and
is filtered by `knowledge_grants`; search hits land on the run receipt with
sources; instance scratch does not promote by default. Builder cannot read
`user-private` or `planning:unreleased-debate`.

This is not a vector database. This is not `kiana memory` CLI. This is not
`rag_collection` on DepartmentSpec becoming a memory engine. This is not
JointSymposium. This is not staffing Librarian.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, v0.4
review/MCP/skills/provider, v0.5.1 five departments, TUI park, or
TeamCreate/SendMessage.

1. **Six file/JSONL partitions, no mixed store.** Layers are `company`,
   `department`, `role`, `project`, `user`, `instance-scratch`. Project /
   department / role / scratch live under `{project}/.kiana/memory/`.
   Company / user live under `$KIANA_HOME/memory/`. Searching one collection
   never opens another file. Shape from memorix (project memory lives with
   the repo) and MemPalace (user memory is a separate house). Not embeddings.
2. **Tools go through the daemon broker.** Model names `memory.search` and
   `memory.write`. Operations are the same strings. Capability kind is
   `Query` for search (ReadOnly) and `Filesystem` for write (LocalWrite).
   Do not add a `skill` tool. Do not expand `kiana-tools`. Do not format
   `cli.rs`. PATH-03 becomes `shell` + `apply_patch` + `mcp` +
   `memory.search` + `memory.write`.
3. **ACL is knowledge_grants, fail closed.** Policy denies unknown
   collections (`role_knowledge_denied`), disallowed writes
   (`role_memory_write_denied`), and scratch promotion
   (`role_memory_promote_denied`). Builder grants are company / project /
   role:builder / scratch. Builder write is scratch only. PM may read
   `user:prefs` and department planning, including unpublished debate.
4. **Hits are receipt facts.** `kiana.memory-search.v1` hits include
   layer, collection, id, source, verified. Receipt copies them to
   `memory_hits`. Empty source → `verified: false`. Chat is never
   auto-ingested. Identity (`role_id`, `department_id`, `session_id`) is
   stamped from RequestContext before policy.
5. **Product proof is DaemonHost.** Same-host tests, not a CLI compile.

Demo (same-host):

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

## Requirements this phase

MEM-01..MEM-04. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP/WB/REV/CODE-01..04/DEPT-02
still true. PATH-03 gains memory.search / memory.write.

## Frozen

- JointSymposium / staffing every COMPANY.md role / Librarian
- vector DB / graphiti / GitNexus / kiana-query as the memory engine
- treating `kiana-commands` memory.md or letta landing page as this slice
- auto-ingest of chat transcripts
- TeamCreate / SendMessage
- live provider / HTTP MCP
- structured Read/Grep/Glob
- migrating TUI
- formatting `kiana-entrypoints/src/cli.rs`
