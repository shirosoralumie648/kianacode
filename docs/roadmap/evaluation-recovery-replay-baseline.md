# EQ-31 recovery and replay evaluator baseline

## Scope

EQ-31 adds `RecoveryReplayEvaluator` to `kiana-quality`. It checks crash/restart evidence,
monotonic replay cursors, explicit resume, old/new fence separation, logic-version compatibility,
replay digest divergence, and `result_unknown` reconciliation rules. Replay divergence and unknown
external outcome are emitted as separate stable findings; an unknown result requires a
reconciliation reference and never permits retry before reconciliation.

The evaluator consumes evidence only. It does not restart a process, read or rebuild an EventLog,
revoke or issue a fence, query an external effect, reconcile an operation, or authorize a retry.

## Completion slice (2026-10-17, CI-only)

The original slice existed in source; this completion slice closes the card's exit conditions and
turns the first version into a full, reject-first recovery contract:

- Crash/restart: restart cursors must be positive and monotonic; a crash without a restart is
  `replay.restart_missing`; a restart without an explicit resume is `replay.resume_missing`; the
  explicit resume must cite a bounded reference (`replay.resume_ref_missing`) while a stray
  reference without a resume is `replay.resume_ref_unexpected`; and the restart must cite a strict
  `sha256:` checkpoint digest at the pre-restart cursor (`replay.checkpoint_invalid`,
  `replay.checkpoint_cursor_mismatch`).
- Fence: both fence digests must be strict `sha256:` values, the old fence must be revoked and a
  different new fence bound (`replay.fence_invalid`), the digest may not be reused
  (`replay.fence_reused`), the epoch must advance (`replay.fence_epoch_not_advanced`), and both
  scopes must be bounded and identical (`replay.fence_scope_invalid`,
  `replay.fence_scope_mismatch`).
- Restart attempts: attempts are ordered by fence epoch (then identity and cursor) before
  evaluation, with unique bounded identities, strictly increasing epochs, non-decreasing cursors,
  no attempt after a terminal attempt, and the last attempt epoch pinned to the new fence epoch.
  The finding set does not depend on the caller's array order, while duplicate evidence stays
  visible under distinct `replay:attempt:<index>` references.
- Logic version and divergence: logic versions are bounded identities; drift is reported
  separately as `replay.logic_version_drift`. Digest pairs must be strict `sha256:` values
  (`replay.digest_invalid`); a mismatch is its own finding `replay.divergence` that must cite the
  recorded divergence evidence (`replay.divergence_ref_missing`), and a divergence reference
  without a divergence is `replay.divergence_ref_unexpected`.
- Replay coverage: the replayed cursor range must be positive and ordered
  (`replay.cursor_invalid`) and must cover the checkpoint and the post-restart cursor
  (`replay.cursor_not_covered`).
- `result_unknown`: the unknown outcome always stays visible as `replay.result_unknown`, demands
  `reconcile_required` (`replay.reconcile_missing`), a known reconciliation outcome
  (`replay.reconciliation_outcome_invalid`) and a bounded reconciliation reference
  (`replay.reconciliation_ref_missing`; a stale reference is
  `replay.reconciliation_ref_unexpected`). Retry is only permitted once reconciliation has proven
  the effect absent (`reconciled_absent`); every other outcome is
  `replay.unknown_retry_forbidden`. A known result may not carry an unknown retry contract
  (`replay.unknown_contract_invalid`).
- Hard input bounds live in `RecoveryReplayInput::validate`
  (`recovery_replay_input_schema_invalid`, `recovery_replay_attempt_limit_exceeded`) so structural
  drift fails closed before any finding; entry-shape problems remain findings.
- Findings carry per-section evidence references (`replay:crash-restart`, `replay:fence`,
  `replay:attempt:<index>`, `replay:replay`, `replay:unknown`), and every caller-supplied value
  goes through `Finding::redacted`.

## Evidence and limits

- `kiana-quality/tests/eq31_recovery.rs` (21 rejection-first fixtures) covers equivalent restart
  replay, the card's acceptance scenario
  `replay_divergence_and_unknown_are_distinct_findings`, crash/restart and resume denial, resume
  and checkpoint bounds, fence revocation/reuse/epoch/scope, attempt epoch/cursor/terminal
  ordering, last-attempt fence binding, replay cursor coverage, logic-version drift versus digest
  divergence, the complete Unknown/reconciliation contract (including the only sanctioned retry
  path), schema drift on every section, unknown-field/envelope drift, bounded references and
  versions, oversized input rejection, order independence with duplicate attempts, the redaction
  boundary over caller values, typed round-trip, and deterministic evidence-bound registry
  output.
- `kiana-quality/tests/eq31_recovery_guard.rs` protects the pure fail-closed boundary (no
  filesystem/network/process/provider/store/broker dependency), the required markers and stable
  codes, the fixture matrix, the manual lane's exact steps and the automatic `kiana-quality` shard
  (`targets: null`).
- `.github/workflows/eq31-recovery-replay.yml` runs `cargo fetch --locked`, the format gate, both
  fixture targets and two `--no-run` compile steps in GitHub Actions; local tests are
  intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not claim real crash or
power-loss recovery, durable checkpoint/replay, external reconciliation, provider receipts,
cross-process fence proof, promotion authority, live or physical evidence.

## Known limits

- Findings are diagnostics over caller-supplied values only; they never restart a session, read a
  durable event log, advance a fence, query an effect outside the process, or reconcile anything.
  Mapping them into blocking rules belongs to EQ-42.
- `crash_observed`, `restart_observed`, `explicit_resume` and the checkpoint digest are
  caller-asserted evidence. The evaluator can refuse an incoherent, reused or unverifiable shape,
  but it cannot prove that a real crash or power loss happened.
- `reconciled_absent` is the only outcome that sanctions a retry; the evaluator never performs or
  verifies the reconciliation itself and never issues the retry.
- References are restricted to a bounded ASCII identity alphabet; unknown shapes are rejected as
  invalid rather than normalized.
- `Finding::redacted` covers known secret shapes; unmarked high-entropy values may still pass.
- The manual lane is `workflow_dispatch`-only; automatic coverage depends on the unified `ci.yml`
  `kiana-quality` shard (both targets are discovered because `targets` is null).
