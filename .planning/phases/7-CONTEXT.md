# v0.3 Phase 3 Context — One bounded symposium

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`

## Classify

Phase, not spike. User-visible completion is: the same `DaemonHost` runs one planning Symposium (PM chair + Architect). Speakers keep private sessions and share a blackboard. Close writes a DecisionRecord and one WorkPacket. Builder is not an attendee. Anti-meeting can skip debate and still emit the packet.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 Phase 1 roles, Phase 2 spawn, TUI park, five departments, or RAG.

1. **CLI is `kiana run --symposium`.** Goal is the prompt. Cannot combine with `--packet`, `--continue`, `--cancel`, or `--receipt`. Omitted `--role` becomes PM (chair). `--role` other than pm → `symposium_chair_must_be_pm`. `--anti-meeting` only with `--symposium`.
2. **Attendees are fixed:** `pm` + `architect`. Chair is PM. Builder is never seated. `builder_present=false`. Do not unfreeze TeamCreate/SendMessage.
3. **Private sessions + blackboard.** Each speaker has their own session. User-visible model input is agenda + blackboard, not the other speaker's transcript. Meeting turns are read-only; the orchestrator writes artifacts after close.
4. **Hard cap `max_rounds`.** Default 4, max 8, min 1. Round-robin PM then Architect. Close always writes `plan/DECISION.json` + `packet/TASK.json` (`kiana.decision-record.v1` + `kiana.work-packet.v1`).
5. **Anti-meeting skips debate.** `--anti-meeting` writes DecisionRecord (`skipped_meeting=true`) and a Builder WorkPacket from the goal. No speaker model turns. Fail-closed if it cannot write a decision.

Demo:

```bash
kiana trust .
kiana run --symposium --anti-meeting --sandbox workspace-write --json -- "create GOLDEN_PATH.txt containing hello"
# plan/DECISION.json + packet/TASK.json; builder not seated
kiana run --packet packet/TASK.json --sandbox workspace-write --json
```

Same-host proof: convene then spawn on one DaemonHost; Architect user text has blackboard claims, not PM transcript secret.

## Requirements this phase

SYMP-01, SYMP-02, SYMP-03. ORCH-01/ORCH-03 still true.

## Frozen

- five departments / six-layer RAG / JointSymposium
- TeamCreate / SendMessage
- `kiana-tools` new family
- packet path isolation in policy
- eval / install as completion
- migrating TUI
