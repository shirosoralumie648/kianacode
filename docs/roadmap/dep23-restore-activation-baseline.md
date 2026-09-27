# DEP-23 - Restore activation, new lease/fence and readiness gate baseline

> Snapshot date: 2026-09-28. This slice consumes DEP-22's "eligible for activation" and decides
> whether a quarantined root may actually be served: it mints a new lease and a new fence token,
> leaves the replaced root in place read-only, and keeps new commands out until readiness is
> established. Local Cargo test/build/check/clippy/smoke commands are intentionally not run;
> GitHub Actions owns fixtures and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`DEP-23`](../roadmap.md#step-dep-23) |
| source snapshot | master plus this DEP-23 restore activation slice |
| feature_status | `partial` for source-level activation, fencing and readiness contracts |
| proof_level | `source`; no `local_behavior`, durable, live or physical promotion |
| canonical path | `Ready` root + complete scan + explicit decision -> report -> record -> activation state -> command admission |

DEP-22 deliberately stopped at "eligible for activation": a `Ready` root with a complete scan and
nothing disqualifying. This is the step that acts on that eligibility. Nothing about a restore
finishing activates a root; activation is a separate, explicit operator decision with its own
preconditions, and the two are never collapsed.

The vocabulary is borrowed, not reinvented. Leases and fence tokens are `kiana_domain::
OperationLease` and `FenceTokenId`, minted through the existing `OperationLeaseCas`, so the token
here is the same kind of token `OperationLeaseCas::acquire` already refuses to reissue. The
quarantine stages, the scan status and `activation_eligible` are DEP-22's, used as-is.

## What activation is

Four things happen together, and all four are required:

1. **A new lease and a new fence token.** The minted token may never equal the replaced writer's
   token. The old token is burned, not rotated forward.
2. **The replaced root becomes read-only and is retained.** It is never deleted, so the
   pre-restore state stays auditable after the restore.
3. **The old writer is actually fenced.** Not asserted: derived from the lease state. A writer
   holding an `Active` lease is refused no matter what a boolean says.
4. **The readiness gate opens only after all of the above.** Before activation nothing is ready;
   after it, only the new root under the new fence takes writes.

## What the card rejects, and how

| Rejected | How |
|---|---|
| 旧 writer 未 fence (old writer not fenced) | the first rule in `derive`: a non-terminal lease is `restore_activation_old_writer_not_fenced`; an unconfirmed fence is `restore_activation_old_writer_fence_unconfirmed`; a still-writable replaced root is `restore_activation_old_root_not_read_only`; a root about to be discarded is `restore_activation_old_root_not_retained` |
| 相同 identity 不同 hash (same identity, different hash) | `ActivationLedger::record` treats a repeated `activation_id` with a different `record_digest` as a contradiction, not an update: `restore_activation_identity_digest_conflict`. The identical record is idempotent |
| 未有 explicit activate (no explicit activate) | `restore_activation_explicit_activate_required`. A blocked report cannot mint a lease at all, because `RestoreActivationRecord::activate` refuses it |
| ready 前接新命令 (a command admitted before ready) | `restore_activation_command_admitted_before_ready`, and the count may not float free of its evidence (`restore_activation_command_refs_invalid`). Independently, `admit_command_after_activation` refuses every command while the state is `Prepared` |

Plus the invariants no operator intent can waive: a reissued fence token
(`restore_activation_fence_token_reused`), a reused instance identity
(`restore_activation_instance_identity_reused`), and authority or data epochs that do not strictly
advance past the replaced writer's (`restore_authority_epoch_not_advanced`,
`restore_data_epoch_not_advanced`).

### The decision order is fixed

`derive` checks in one order so the reported reason is the first violated rule and therefore the
same for the same facts. The first four are **positive observations** about the instance being
replaced -- a live writer, an unconfirmed fence, a writable root, a root about to be destroyed.
Each is a live writer or a lost audit trail rather than an absence of proof, so each outranks
what follows. Then the identity and epoch invariants. Then the three absences of proof, ordered by
how cheap the caller's fix is: a command already admitted, a missing explicit flag, an ineligible
root.

## Success path

`activate` mints the new lease through `OperationLeaseCas` rather than hand-building one, so the
new lease carries the same shape, digests and revalidation rules as every other single-writer
lease. `ActivationState::from_record` then turns the sealed record into the readiness gate.
`admit_command_after_activation` accepts a command only against the activated root, under the new
instance, the new fence token and the new epochs. `admit_audit_read_after_activation` is the
deliberate counterpart: the replaced root stays readable, bounded to roots this activation names.

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| success path | a new lease under a new fence on the restored root; old root retained read-only |
| old writer not fenced | an `Active` lease is refused even with `writer_fenced: true` |
| fence unconfirmed | `restore_activation_old_writer_fence_unconfirmed` |
| old root writable | `restore_activation_old_root_not_read_only` |
| old root discarded | `restore_activation_old_root_not_retained` |
| fence token reused | `restore_activation_fence_token_reused` |
| instance identity reused | `restore_activation_instance_identity_reused` |
| authority epoch flat | `restore_authority_epoch_not_advanced` |
| data epoch flat | `restore_data_epoch_not_advanced` |
| command before ready | `restore_activation_command_admitted_before_ready` |
| command count unevidenced | `restore_activation_command_refs_invalid` |
| no explicit activate | `restore_activation_explicit_activate_required` |
| root not ready | `restore_activation_root_not_eligible` |
| root admitted against another active root | `restore_activation_superseded_root_mismatch` |
| report rewritten | a stale or re-derived refusal both fail `restore_activation_report_binding_invalid` |
| record claims writable old root | `restore_activation_record_old_root_writable` / `..._not_retained` |
| same identity, different hash | `restore_activation_identity_digest_conflict`; identical replay is idempotent |
| readiness gate | not ready before activation; only the new root under the new fence after; old root readable but not writable; old token and stale epochs refused |
| schema version | minor stays compatible, major does not |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

This is a source contract over adapter-reported facts. It **does not** take an OS lock, copy or
delete a file, mark a filesystem read-only, revoke a token inside a live instance, append an event,
or revalidate anything at an effect boundary. `writer_fenced` and `write_mode` are claims the
adapter makes; the domain can only require that they be present and consistent, not that they are
true, so an adapter that lies about fencing defeats the check the same way a lying
`present_artifacts` inventory defeats DEP-22's scan. Minting a `OperationLease` value here does not
persist it: a later step must do that, exactly as `OperationLeaseCas` documents for itself, and
must revalidate at the effect boundary. The projector/index rebuild DEP-22 represents as a stage
transition remains a stage transition, not a rebuild. The restore/backup fault fixtures, RPO and
RTO measurement are DEP-24. Nothing here is `local_behavior`, durable, live or physical evidence.
