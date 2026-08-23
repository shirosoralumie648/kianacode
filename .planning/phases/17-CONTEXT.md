# v0.5 later Context — Department symposiums

Date: 2026-08-23
Status: locked
Proof ceiling: `local_behavior`
Requirements: SYMP-04
Chosen slice: five department-bounded symposiums, not JointSymposium

## Classify

Phase, not spike. User-visible completion is: Initiating / Planning /
Executing / Monitoring / Closing can each convene a bounded symposium.
The chair is a role that `can_convene` in that department, not always PM.
Planning still emits `plan/DECISION.json` + `packet/TASK.json` with
PM+Architect, Builder absent, and anti-meeting skip still writes those
artifacts. Non-planning meetings write that department's decision file
and do not hand a WorkPacket to Builder or pull Builder into planning
debate.

This is not JointSymposium. This is not staffing every COMPANY.md role.
This is not auto-promoting decisions into department RAG. This is not a
new CLI flag.

## Locked discuss decisions

Do not reopen v0.2 write path, v0.3 planning symposium/packet, v0.4
review/MCP/skills/provider, v0.5.1 five departments, v0.5.2 six-layer
memory, LONG-02 compact/resume, TUI park, or TeamCreate/SendMessage.

1. **Chair follows the department, not a hardcoded PM.** Department is
   the chair RoleSpec's department. Chair must have `can_convene`.
   Planning chair remains PM (`symposium_chair_must_be_pm` when a
   planning role other than PM tries to chair). Architect still cannot
   convene. Builder may convene **executing** only.
2. **Attendees are that department's catalog roles.** Planning stays
   PM+Architect. Builder is not an attendee outside executing
   (`symposium_builder_not_attendee`). Mixed-department attendee lists
   fail closed as `joint_symposium_frozen`. Do not implement
   JointSymposium.
3. **Artifacts are departmental. WorkPacket is planning-only.**
   initiating → `charter/DECISION.json`; planning → existing
   `plan/DECISION.json` + `packet/TASK.json`; executing →
   `receipt/DECISION.json`; monitoring → `gate/DECISION.json`;
   closing → `lessons/DECISION.json`. Core writes these files.
   Non-planning close does not create a Builder WorkPacket.
4. **Anti-meeting skip remains charter-shaped.** Skip still writes the
   department decision (and planning packet). No model turn. Promotion
   into department RAG is explicit `memory.write` later, not this slice.
5. **Product proof is DaemonHost.** Same-host tests, not a CLI compile.
   Leave `cli.rs` / `harness_run.rs` planning-PM CLI gate alone.

Demo (same-host):

```text
PM + anti-meeting
  → plan/DECISION.json + packet/TASK.json
  → builder_present=false
sponsor + anti-meeting
  → charter/DECISION.json; no packet/TASK.json
builder chair
  → receipt/DECISION.json; not plan/DECISION.json
reviewer chair
  → gate/DECISION.json
closer chair
  → lessons/DECISION.json
architect chair
  → symposium_chair_must_be_pm
planning attendees still exclude Builder
default Builder can still write GOLDEN_PATH.txt
```

## Requirements this phase

SYMP-04. PATH/TRUST/SESS/EVD/ROLE/ORCH/SYMP-01..03/WB/REV/CODE-01..04/
DEPT-02/MEM-01..04/LONG-02 still true.

## Frozen

- JointSymposium / staffing every COMPANY.md role / Librarian
- auto-ingest of decisions into memory JSONL
- TeamCreate / SendMessage
- new CLI flags / formatting `cli.rs`
- expanding `kiana-tools`
- live provider / unsupported_streaming
- HTTP/SSE MCP
- migrating TUI
- vector DB / kiana-query as a meeting engine
