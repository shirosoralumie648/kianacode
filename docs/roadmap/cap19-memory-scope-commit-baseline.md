# CAP-19 memory scope and reliable commit baseline

CAP-19 records the existing memory adapter boundary as one CI-gated step: server-derived
execution scope determines collection/session/role/department/project access; model arguments can
only request a subset. The adapter captures the trusted storage root, rejects snapshot drift and
symlink/governance violations, commits `memory.fact` through EventStore idempotent/CAS before
appending its JSONL projection, and refuses unjournaled or lagging reads. Candidate/review and
revoke/data-epoch paths remain explicit.

The write path is bounded by the existing blocking adapter boundary and mutation revision ledger;
this step does not introduce a second memory model or direct shell/MCP access to the store.
Cross-process writer leases and full crash recovery remain later persistence evidence.

GitHub Actions runs existing J3/CM scope, mutation, retrieval and EventStore fixtures, CAP-19
source guards and workspace compilation. No local runtime tests or smoke commands were run.
