# DEP-31 post-migration rebuild baseline (partial)

The domain now evaluates source/projection/index/receipt invariants before a ready gate opens.
The report binds the MigrationRegistry digest and checks source cursor equality, projection
generation, index generation, receipt generation, source replay digest, projection digest, index
presence and receipt source references. Projection lag or overrun, stale generation, replay
divergence, receipt generation/source mismatch and registry drift return stable remediation.

The report is read-only and never rewrites facts, opens a broker, or claims an index is authority.
It is a pure proof that a later adapter may publish read-only handles.

DEP-31 remains partial with source/static evidence only. No projector/index rebuild, EventLog
replay, durable receipt projection, quarantine or ready-admission integration is wired yet.
