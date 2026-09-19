# CAP-26 local five-tool and Hook capability closeout baseline

CAP-26 is a GitHub-CI-only closeout gate for the existing product spine. The test matrix first
checks deny/Unknown/cancel/reconciliation boundaries, then compiles the `kiana` binary and runs
the cassette-driven golden path plus workbench/P0 smoke. The golden path proves the existing
trusted project → planning packet → Builder → apply_patch → file/receipt projection path at
`local_behavior`; the other fixtures cover shell/process, Memory, stdio MCP and Hook adapter
contracts through the same ControlPlane and Broker boundary.

All effectful adapters are registered in the composition root and receive an authorized request;
they do not start a model loop or call a handler from an entrypoint. Adapter results are bounded,
redacted and effect/stop-aware; cancellation, invalid scope, trust drift, MCP transport/schema
failure, Hook update-input unsupported, and Unknown outcomes remain explicit. Receipt and file
facts are read from the EventLog projection after execution, not inferred from stdout or UI text.

The workflow records command output and exit status in GitHub Actions. It deliberately does not
claim live provider, physical install, cross-provider exactly-once, or local test execution; this
closeout only promotes source/CI evidence to the repository's stated `local_behavior` ceiling.
