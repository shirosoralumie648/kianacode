# NM-12 durable inbox rebuild and notification lifecycle baseline

> 快照日期：2026-09-25。测试只由 GitHub Actions 执行；本步骤不在本地运行测试、build、check、
> clippy 或 smoke，且不等待 CI。

`NotificationLifecycleFact` models append-only withdraw, supersede and expire decisions bound to
notification id, source event/cursor, data epoch, reason and digest. `NotificationProjector` clones
the committed materializer, validates lifecycle facts, folds exact replay idempotently, rejects epoch
or cursor regressions and refuses terminal lifecycle rewrites before atomically replacing its
projection. Current inbox visibility hides only a validated lifecycle-marked item; source history is
never deleted and replacement notifications remain independent source objects.

`feature_status=implemented`; `proof_level=source`. The projector is rebuildable but its adapter is
still in-process: no durable checkpoint, cross-process retention store, legal-hold executor, artifact
deletion, provider effect or live receipt is claimed. Retention/withdraw/supersede is projection
governance, not HumanTask/Approval mutation.
