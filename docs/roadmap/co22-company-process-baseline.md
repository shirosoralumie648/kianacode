# CO-22 deterministic Company ProcessManager baseline

## Scope

CO-22 adds a typed Company business planner over the existing durable workflow planner. The fixed
`CompanyProcessTemplate` binds version/hash and role order. `CompanyProcessState` records the
current gate, revision, assignment and associated WorkflowInstance. Typed Company facts advance
only the declared node transition and produce a stable `CompanyProcessIntent`.

The planner is pure: it does not parse model text, call tools, invoke a Runner/Broker or complete a
gate from a self-report. Sponsor decisions produce a HumanTask intent; Builder/Reviewer/Closer
steps carry their fixed role and assignment reference. Replayed fact digests return the same state
without a second intent. Core's adapter delegates to `plan_company_process` and the existing
`kiana-workflow` `plan_command_intent` path remains the only workflow planner/executor boundary.

## Implemented source slice

- Fixed template hash/version and role sequence: Sponsor → PM → Handoff → Builder → Reviewer →
  Sponsor HumanTask → Closer → Close.
- Typed intake/charter/plan/handoff/run/review/sponsor/delivery/incident events with evidence refs;
  unknown node/event, template drift and private evidence fail closed.
- Stable process revision, workflow instance association, consumed fact digest set and intent ID;
  duplicate facts are replay no-ops and never dispatch effects.
- Core `company_process.rs` is a pure adapter only; no second execution loop was introduced.

## CI-only evidence

`.github/workflows/co22-company-process.yml` runs formatting, the domain process fixtures, the Core
source guard and domain/core/workflow/daemon test-target compilation on GitHub Actions. Local Cargo
tests, builds, checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- The process planner is a typed source slice; durable Company/process stream materialization,
  wakeup queue and cross-stream intent consumption remain CO-23 work.
- Role assignment, packet attempts and human approvals are references validated at the boundary;
  external provider effects, live/physical outcomes and durable process recovery remain open.
