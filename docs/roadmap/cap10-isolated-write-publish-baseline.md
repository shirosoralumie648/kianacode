# CAP-10 isolated write layer and controlled publication baseline

CAP-10 keeps shell/patch/MCP writes in an `isolated_staged` workspace with baseline
file identities and an exact `path_allow` set. Changes are represented as bounded
`PublishedFile`/changeset records; new files use a confined parent and never authorize
sibling paths. Publication rechecks source revision/identity, commits through the
existing descriptor-relative patch journal, and emits a bounded host-effect receipt.

Failure, cancellation, timeout, outside-scope changes, concurrent host edits and
rollback ambiguity do not clean/reset or overwrite the host: they remain unpublished,
reconciled or `result_unknown`. Workspace checkpoints reuse the same revision/data-epoch
and rollback boundary. GitHub Actions runs CAP-07/08/09 guards, daemon harness runtime
fixtures and CAP-10 source guard plus workspace compilation; local runtime tests and
smoke commands are intentionally not run.

This is source/static evidence only; disk-full/power-loss and physical filesystem race
proof remain open.
