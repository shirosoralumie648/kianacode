# CAP-07 EnvironmentPort and verifiable backend baseline

CAP-07 defines the environment boundary as `probe → plan → prepare → execute →
quiesce → dispose`. Scope, owner, backend/version and plan digest are server-owned
inputs; the port defaults fail-closed when a phase or backend is unsupported. A pure
plan never starts a process.

The current Linux backend is the bwrap plan in `harness_sandbox`: it resolves an
explicit executable or PATH candidate, pins file identity, rejects missing/wrong or
non-Unix backends, requests unshare/proc/dev/tmpfs/cap-drop/clearenv/network isolation,
filters environment variables and refuses path/symlink/private-scope escapes. The
execution-control inspection reports backend and limits without claiming
`behavior_verified`; host enforcement is not inferred from `KIANA_SANDBOX_BACKEND`.

GitHub Actions runs CAP-07 source guards plus existing sandbox/environment and scope
fixtures and workspace target compilation. Local runtime tests and smoke commands are
intentionally not run.

This is source/static evidence only; real kernel/userns enforcement, non-Linux backends,
cross-host process isolation and physical sandbox guarantees remain open.
