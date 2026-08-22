# v0.3 Phase 2 Context — Independent Builder spawn

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is: the same `DaemonHost` dispatches a Builder from a WorkPacket into a **new** session. The worker prompt is the packet only. Planner chat is not copied.

## Locked discuss decisions

Do not reopen v0.3 Phase 1 roles, TUI park, Symposium, five departments, or RAG.

1. **CLI stays `kiana run --packet <path>`.** No `kiana company spawn`. Packet path is relative to the project root. `--packet` cannot combine with a prompt, `--continue`, `--cancel`, or `--receipt`. `--role` other than builder (or omitted, which becomes builder) → `packet_role_must_be_builder`.
2. **WorkPacket is a domain contract**, schema `kiana.work-packet.v1`. Do **not** promote `kiana-tasks` swarm packets to the product path. Shape is COMPANY.md: id, from/to department, assignee_role, goal, path_allow, acceptance, forbidden.
3. **Spawn is a new protocol body**, not `Continue`. Same host, **new session**. If the inbound `session_id` is already in the session index → `spawn_session_not_fresh`. Assignee must be `builder` / `executing`.
4. **Packet is the only model-visible user input.** Prompt is assembled from packet fields. No planner transcript, no extra CLI prompt. RoleSpec builder prompt remains the role system prompt, not orchestrator chat.
5. **Packet `path_allow` is recorded and injected into the prompt this phase; it does not yet intersect Builder policy.** Path locks wait. TeamCreate / SendMessage stay frozen.

## Demo

```bash
kiana trust .
# packet/TASK.json is a kiana.work-packet.v1 assigning builder
kiana run --packet packet/TASK.json --sandbox workspace-write --json
# receipt: new session_id, role_id=builder, work_packet_id set, file from goal appears
```

Same-host proof is in-process DaemonHost: planner session first, spawn second, planner secret absent from builder model messages.

## Requirements this phase

ORCH-01, ORCH-03. ROLE-01..03 still true for the spawned Builder.

## Frozen

- Symposium / DecisionRecord / blackboard
- five departments / six-layer RAG
- TeamCreate / SendMessage
- `kiana-tools` new family
- packet path isolation in policy
- eval / install as completion
- migrating TUI
