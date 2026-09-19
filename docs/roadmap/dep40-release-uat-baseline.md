# DEP-40 release and recovery cross-entrypoint UAT baseline (partial)

DEP-40 adds a typed UAT matrix for release/upgrade/rollback/backup/restore/migration/health
across CLI, Web, Workbench and Desktop. Every entrypoint/scenario pair must carry deny and
success rows; restart recovery, replay and `result_unknown` rows are required, and Unknown cannot
be automatically retried. Each row binds the same DaemonHost, ControlPlane and KianaHarness
spine digests and distinguishes fake-provider evidence from explicitly approved live opt-in.

`ReleaseUatEvidence` now binds the matrix digest to a source snapshot and CI run reference,
provider mode, proof level, receipt references, reviewer and Unknown reconciliation. Fake rows
cannot claim live/physical proof; a verified bundle requires receipts and reconciliation, while
fixture/blocked/unknown dispositions retain limitations.

GitHub Actions runs the matrix fixture plus the existing entrypoint parity and DaemonHost spine
 fixtures. The gate is fake provider/source-bound: it does not perform a release, upgrade, backup, restore,
migration or external health effect, and it is not a live deployment. Durable cross-process
fixtures, real provider/account receipts, Desktop runtime packaging and physical/live UAT remain
open; DEP-40 is partial.
