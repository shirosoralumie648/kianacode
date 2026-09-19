# CAP-16 patch transaction, rollback and recovery baseline

CAP-16 records each patch transaction as a prepared journal record before file effects and a
resolved record only after commit or guarded rollback. Startup/explicit recovery enumerates
unresolved transactions with bounded limits, rechecks root identity and every serialized path,
and never re-executes the original authorization. Rollback requires the current file snapshot to
still match this transaction; an outside edit or journal/rollback ambiguity remains
`result_unknown` and is not cleaned or overwritten.

Descriptor-relative parent handles, precondition verification, `sync_all` and rename boundaries
are reused by the CAP-10/15 planned operation path. Ordinary multi-file host visibility is not
claimed atomic; only the transaction/reconciliation evidence boundary is covered here.

GitHub Actions runs existing late-hunk and commit-failure rollback fixtures, CAP-16 source guards
and workspace compilation. No local runtime tests or smoke commands were run.
