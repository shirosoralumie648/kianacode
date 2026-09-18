# ER-17 serializable RunSnapshot / pending writes baseline

## Scope

ER-17 pins the serializable, digest-bound pause material used by approval waits and explicit
resume: RunSnapshot schema/context/sandbox/role prompt, runner state digest, pending invocation,
Cell/resource checkpoint and resumable flag; InvocationResumeBinding keeps parameters/catalog,
owner/authority/project/sandbox and pending-batch identity stable; Runner checkpoint preserves
pending writes without replaying settled effects.

## Evidence gate

- `er_snapshot_digest_or_scope_mismatch_is_denied` checks snapshot/binding digests, owner/scope/
  authority/catalog/sandbox validation, runner checkpoint identity and recovery CAS/stale fences.
- `er_snapshot_with_redacted_input_is_non_resumable` checks the EventLog redaction boundary marks
  altered snapshot material non-resumable and requires explicit continuation material.
- `er_snapshot_after_terminal_is_not_restorable` checks terminal projection/stale snapshot guards,
  resume claim ordering and no automatic restore/execute path.

The GitHub workflow runs the existing CP-18/P0-F-03/P0-G-03/H13/H14 checkpoint and resume
fixtures, then this source guard and workspace test-target compilation.  Local runtime tests are
not run.

## Limits

Source plus CI-fixture evidence only: no power-loss/cross-process durable snapshot proof, physical
filesystem backup guarantee, live provider effect reconciliation or ER-18 workspace transaction
proof is claimed.
