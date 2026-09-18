# ER-20 restart projector / default pause baseline

## Scope

ER-20 treats restart as fact validation and read-model rebuild: validate EventLog integrity,
rebuild Run/Invocation/Attempt/resource/audit projections and pending/Unknown state, fence stale
resources, then expose a read-only recovery view.  Startup never resumes a pending run or issues a
permit; explicit Resume remains the later ER-21/CP-19 command.

## Evidence gate

- `restart_never_auto_resumes_pending_run` checks journal scan, projection/resource rebuild,
  pending reconstruction and DaemonHost readiness without auto-resume/permit/Broker calls.
- `corrupt_journal_does_not_boot_empty` checks JSONL corruption/torn-tail/identity handling and
  degraded readiness instead of silently treating a bad journal as an empty store.
- `rebuild_does_not_issue_permit` checks read-only resource/attempt/audit projection and unsupported
  projection-port behavior.

The GitHub workflow runs existing JSONL, projector, health, resource, audit and UI restart guards,
then this source guard and workspace test-target compilation.  Local runtime tests are not run.

## Limits

Source plus CI-fixture evidence only.  It does not claim cross-process projector durability,
power-loss recovery, physical journal repair or ER-21/ER-22 resume/cancel proof.
