# ER-21 explicit resume preflight and claim baseline

ER-21 makes recovery an explicit, server-owned command. `resume_run` rebuilds the
exact committed snapshot and pending invocation, rechecks owner/role/sandbox/path,
authority and data epochs, then claims `run.resume_prepared` with the observed run
stream version before restoring the runner. A snapshot claim is single-use: a
sequential or concurrent duplicate returns the stable `run_resume_claim_conflict`
response and cannot install a second runner.

The protocol, DaemonHost, harness, Workbench, Web and product/CLI routes all use the
same `ResumeRequest`/`RequestEnvelope::resume_run` path. CI runs the existing P0-G-03,
CP-19, H14 and CLI resume fixtures plus the ER-21 source guard and workspace target
compilation. Local runtime tests and smoke commands are intentionally not run.

This is source/static evidence only. It does not claim durable, live or physical
proof for concurrent cross-process recovery, runner restore failure injection,
power-loss recovery, or external effect reconciliation.
