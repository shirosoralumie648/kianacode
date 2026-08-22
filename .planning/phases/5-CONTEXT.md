# v0.3 Phase 1 Context — Role catalog + policy by role

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is: the same `DaemonHost` spine dispatches three RoleSpecs (`planning/pm`, `planning/architect`, `executing/builder`); policy fail-closes `apply_patch` to src for planning roles; default `kiana run` is still Builder.

## Locked discuss decisions

Do not reopen v0.2 golden path, TUI park, five departments, Symposium, or RAG.

1. **CLI stays `kiana run`.** Default role remains `builder` / `executing`. Add `--role builder|pm|architect`; department is inferred from the catalog. Unknown role → `role_unknown`. No `kiana company run`.
2. **PM may use path-restricted `apply_patch`.** PHASES said "no apply_patch on src". v0.3 Phase 1 does not add a new write-artifact tool. PM tools = `apply_patch` only; `path_allow` = `charter/` `plan/` `packet/`. Architect tools empty; sandbox `read-only`. Builder stays `shell` + `apply_patch` over `.`.
3. **Policy is the enforcement.** Harness may still advertise both tools this phase. Denied `role_*` reasons fail the run (file must not appear). Do not wait for independent spawn to enforce rooms.
4. **RequestContext carries role.** Receipts read `context.role_id` / `department_id`, not a hardcoded `RoleSpec::builder()`. Empty role id defaults to builder for v0.2 compatibility.
5. **Workbench eval/install wait.** This phase is ORCH-WP1 + ORCH-02 policy, not v0.3.1/v0.3.2.

## Demo

```bash
kiana trust .
kiana run --sandbox workspace-write -- "create GOLDEN_PATH.txt containing hello"
# still builder; file appears

kiana run --role pm --sandbox workspace-write -- "create GOLDEN_PATH.txt"
# fail closed; GOLDEN_PATH.txt absent; error role_path_denied or role_tool_denied

kiana run --role pm --sandbox workspace-write -- "write plan/WORK.md"
# apply_patch under plan/ is allowed
```

CI equivalent: cassette + DaemonHost/CLI tests with isolated `$KIANA_HOME`.

## Requirements this phase

DEPT-01, ORCH-02, ROLE-01..03 (still true for default Builder).

## Frozen

- Symposium / DecisionRecord / blackboard
- independent Builder spawn from a WorkPacket
- five departments / six-layer RAG
- TeamCreate / SendMessage
- MCP / IDE / Desktop / `kiana-tools` new family
- eval harness / install.sh as completion
- migrating TUI
