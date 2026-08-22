# v0.4 Phase 1 Context — Reviewer ≠ author

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is: monitoring Reviewer reviews a completed Builder run in a **new session**. Reviewer session_id must not equal the author's. Orchestrator writes `gate/REVIEW.json`. Reviewer cannot `apply_patch` src. No model is required for this gate (deterministic, like anti-meeting).

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 symposium/packet, TUI park, five departments, RAG, MCP, or TeamCreate/SendMessage.

1. **CLI is `kiana run --review <author_session_id>`.** Exclusive with `--symposium` / `--packet` / `--continue` / `--cancel` / `--receipt`. Prompt forbidden (`review_prompt_conflict`). Omitted `--role` becomes reviewer. `--role` other than reviewer → `review_role_must_be_reviewer`. `--review` with empty id → `review_author_required`.
2. **RoleSpec `monitoring/reviewer`.** Tools empty; sandbox `read-only`; no `apply_patch` / `shell`. Department catalog adds `monitoring` (only reviewer this phase). Initiating/Closing stay closed.
3. **New session, author avoidance.** Reviewer session is freshly generated. If it equals the author session → `review_author_session_denied`. Author must be Builder (`review_author_must_be_builder`). Missing author receipt → `review_author_not_found`.
4. **Deterministic gate, not a fused transcript.** Orchestrator reads the author receipt (files_changed, role, session) and writes `kiana.review-packet.v1` to `gate/REVIEW.json`. No runner/model turn. Builder transcript is not copied into the reviewer session.
5. **Coding pack matrix / MCP / skills wait.** This phase is `REV-01` only. `docs/coding-pack-matrix.md` is Phase 2.

Demo:

```bash
kiana trust .
kiana run --sandbox workspace-write --json -- "create GOLDEN_PATH.txt containing hello"
# capture session_id
kiana run --review "$SESSION" --json
# gate/REVIEW.json; role_id=reviewer; department_id=monitoring; session_id != author
```

Same-host proof: in-process DaemonHost builder then review; reviewer session ≠ author; UnavailableRunner is not called for review.

## Requirements this phase

REV-01. PATH/TRUST/ROLE/ORCH/SYMP/WB still true as regression.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- MCP client / skills-on-harness / provider live matrix
- `kiana-tools` new family
- migrating TUI
- using kiana-tasks ReviewPacket as the product path
