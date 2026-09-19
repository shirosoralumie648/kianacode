# AUT-06 Pure Planner Intent Baseline

## Scope

AUT-06 adds a deterministic `WorkflowPlanIntent` projection beside the existing pure
`plan_command`.  `plan_command_intent` returns the next state, a bounded intent kind
(`dispatch`/`reserve`/`wait`/`terminal`/`cancel`/`noop`) and the existing optional effect.  The
intent binds expected/next revision, definition digest, node input digest, deterministic queue key
and an authenticated-authority digest.

The planner consumes only the supplied `AutomationAuthority` snapshot; it does not read EventLog,
call Broker/Runner, inspect the current system clock, choose random IDs or create a second runtime
loop.  ControlPlane calls the wrapper before committing the workflow fact and remains the only
component allowed to dispatch the returned effect.

## Evidence and limits

- Domain `kiana.workflow-plan-intent.v1` is strict and digest-bound.
- GitHub-only fixtures distinguish terminal literal work from reserved/dispatch capability work and
  source-guard the no-I/O/no-second-loop boundary.
- Intent is currently an in-process projection; durable reservation/queue/claim/fence and indexed
  replay remain AUT-07/08/PD/ER.  No local tests, scheduler, external/live or physical proof is
  claimed.

This slice is `feature_status=implemented`, `proof_level=source`.
