# Workflow Transition And Completion Design

**Date:** 2026-07-11

## Problem

Kiana can initialize, inspect, resume, seal, and verify a `WorkflowRun`, but it has no user-facing command that advances the current node through the persisted DAG or completes the run through the Evidence/Verification gates. As a result, a run can remain permanently at `capture`, and commercial release proof cannot point to an actually completed WorkflowRun without manually editing state or EventLog files. Manual edits are forbidden because they bypass the append-only event chain and integrity verification.

## Product Contract

Add two commands:

```text
kiana tasks workflow advance [--json] --decision <decision> [--evidence <workflow-relative-path>]... [--note <text>] <run_id>
kiana tasks workflow complete [--json] --verification <verification_id> <run_id>
```

`advance` is the only normal CLI path for moving between DAG nodes. `complete` is the only normal CLI path for emitting `workflow_completed`.

## Advance Rules

1. Reload the run through `resume_workflow_run`; any state/EventLog conflict blocks the command.
2. Require `status=running`. Blocked, cancelled, or completed runs cannot advance.
3. Load the run's persisted `workflow_dag.json` and resolve exactly one edge where `from=current_node` and `decision` equals the requested decision.
4. Reject unknown, missing, or ambiguous decisions and report the allowed decisions for the current node.
5. Terminal node `completed` has no outgoing transition.
6. Gate and router nodes require at least one `--evidence` path. Evidence paths are relative to the Workflow artifact directory, may not be absolute or contain `..`, must resolve inside the run directory, must be regular files, and may not be empty header-only placeholders.
7. Record evidence path, byte count, and SHA-256 in the transition events. Do not embed file contents.
8. Append `gate_evaluated` when the source node is a gate/router, followed by `node_exited` and `node_entered`. All three events share one `transition_id`, source node, destination node, decision, evidence descriptors, and optional bounded note.
9. Append the transition batch while holding one Workflow writer lease. Persist `state.json` only after the entire event batch is durable.
10. A retry with the same source node and decision after the destination was entered returns an idempotent result only when the last transition metadata matches; otherwise it reports the current state rather than appending a duplicate transition.

## Completion Rules

1. Reload the run through `resume_workflow_run` and require `status=running` with `current_node=completed`.
2. Resolve `verification/<verification_id>.json` inside the same run directory.
3. Read the run's Evidence Ledger and re-run `validate_verification_packet_integrity` plus `validate_verification_packet_completion`.
4. Require packet `workflow_id` and `run_id` to match, `final_status=pass`, at least one required check, no failed/blocked/skipped/unknown required checks, and complete evidence references.
5. Compute the VerificationPacket SHA-256 and append one `workflow_completed` event containing the verification ID, packet path, hash, check counts, and final status.
6. Completion is idempotent only when the existing final `workflow_completed` event references the same verification ID and packet hash. A different packet is rejected.
7. The command never creates or modifies a VerificationPacket. Users must run `kiana validate --workflow <run_id> --json` first.

## Reports

`advance --json` emits `kiana.workflow-transition.v1` with run/workflow IDs, transition ID, source, destination, decision, evidence descriptors, appended event sequence range, and resulting state.

`complete --json` emits `kiana.workflow-completion.v1` with run/workflow IDs, verification ID/hash, event sequence, resulting state, and `idempotent`.

Both schemas use `additionalProperties: false` and are shipped with release packages.

## Failure And Recovery

- Invalid evidence, DAG decisions, VerificationPackets, and inconsistent runs fail before mutation.
- The transition event batch uses the existing writer lease and signed-event path, preserving HMAC sequence and artifact descriptors.
- A write or sync failure leaves `state.json` unchanged. EventLog reconciliation remains authoritative.
- No command edits historical events, rewrites Workflow IDs, or accepts `--force`.

## Commercial Boundary

This feature enables a real completed WorkflowRun and integrity proof. It does not make the tracked tree clean, create a Git commit, fabricate external acceptance, or satisfy signing/channel/live-service blockers.

## Acceptance Criteria

- Legal DAG decisions advance and record signed, evidence-bound transition events.
- Illegal decisions, placeholder evidence, path escape, inconsistent state, and terminal re-advance fail closed.
- Completion rejects missing, foreign, tampered, or non-pass VerificationPackets.
- Completion produces `workflow_completed` as the final event and remains idempotent for the same packet.
- Integrity verification passes after transitions and completion.
- CLI help, schemas, USAGE, preflight, focused tests, workspace tests, release smoke, and package lifecycle all pass.
