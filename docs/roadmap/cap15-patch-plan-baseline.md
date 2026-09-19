# CAP-15 typed patch plan and preview baseline

CAP-15 keeps one parser/overlay/precondition `PlannedPatch` for both a read-only
`apply_patch.preview` capability and the real commit path. The preview reports a digest-bound
operation list, both sides of moves, affected paths, before content hashes, patch byte/hunk counts
and `read_only=true`; it acquires no patch lock and performs no write. The commit returns the same
plan digest/path/before-hash projection after its existing journaled transaction.

Patch bytes, NULs and hunk count are bounded before planning. The existing in-memory overlay means
late malformed/context-mismatch hunks fail before any write; move source and destination enter the
same precondition/scope set. Handler path/data-policy checks are shared by preview and commit.

GitHub Actions runs the daemon preview/commit fixture, CAP-15 source guards and workspace
compilation. No local runtime tests or smoke commands were run.
