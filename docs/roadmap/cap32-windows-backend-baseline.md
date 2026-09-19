# CAP-32 Windows backend baseline (partial)

The shared EnvironmentPort, JobHandle and ProcessSupervisor contracts are platform-neutral, but
the current product execution implementation is Unix/Linux oriented. No Windows handle/ACL/Job
Object backend is implemented in this slice, and Linux compilation cannot prove Windows behavior.

CAP-32 now also consumes the shared typed platform disposition: a Windows TargetOnly or
NotSupported report must carry limitations, behavior_verified=false and no_host_fallback=true;
only a target-backed implementation may claim Implemented.

The target CI job compiles the workspace and runs a source guard only. Required negative cases stay
explicit: Windows reparse/UNC escape, Job Object breakaway, and cross-process lock failure must
deny or be observable; an unavailable backend must not return fake success. CAP-32 remains
`partial` with `behavior_verified=false` until target backend and fixtures exist.
