# PD-33 persistence lifecycle UAT baseline (partial)

PD-33 adds a CI-only persistence UAT matrix for backup, restore, upgrade, restart and governance
delete across CLI, Web and Workbench. Success rows bind StorageRoot/EventStore identity, source and
projection cursors, Receipt evidence, backup manifest verification, quarantine, auth re-admission,
projection/Receipt parity, old-root retention, migration/journal evidence and legal-hold/delete
authorization. `result_unknown` rows require reconcile and forbid automatic retry.

The workflow reuses the existing workspace checkpoint restore guard, entrypoint parity and
DaemonHost spine fixtures. It does not write a backup, restore a root, apply an upgrade, restart a
process or delete governed data. Fake/source evidence is not durable proof; cross-process,
power-loss, SQLite/provider and live external effects remain open. PD-33 is partial.
