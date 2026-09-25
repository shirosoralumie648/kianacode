# P4-J7-30 · Provider product chain and four-surface regression baseline

This source slice records the regression contract for a provider-backed coding round. It does not
run a model, modify a workspace, dispatch a capability or elevate a loopback cassette to live
evidence.

## Source contract

- `ProviderProductChainEvidence` binds bounded digests for the model request/reply, controlled
  capability dispatch, tool result, runtime event and Receipt. Coding scenarios also bind a
  server-observed file-effect digest and usage digest; no path, prompt, key or raw output is
  stored in the evidence contract.
- The matrix requires five explicit cases: a two-model-request coding round trip with one tool
  dispatch/result, cancellation after model finish but before dispatch, slow subscriber terminal
  retention with late-increment fencing, continue/resume without a second completed invocation,
  and budget denial before a provider request.
- Four surface snapshots (`cli`, `tty`, `web`, `desktop`) must share terminal state, Receipt,
  file/usage digests, cursor and `result_unknown`. The client fixture reuses the existing
  read-only `SurfaceParity` comparator; a surface cannot repair a server terminal by issuing a
  second model request.
- The source guard checks the existing `DaemonHost → ControlPlane → KianaHarness → ProviderGateway`
  spine and rejects effect authority or a second loop in the product-chain contract.

## CI-only fixtures

`.github/workflows/p4-j7-30-provider-product-chain.yml` runs target formatting, the domain
product-chain matrix, four-surface parity fixtures, the Core source guard and workspace test-target
compilation. Its path filter includes current CM-36 `kiana-domain/src/memory_workbench.rs` so the
repository-wide formatting dependency is included in a fresh remote run. Local tests, builds,
checks, clippy and smoke commands are deliberately not run, and GitHub CI is not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus CI wiring. The loopback
fixture does not prove a real sandbox file/process observation, durable EventStore/reopen behavior,
provider invoice truth, live model output, external Receipt correctness or physical capability
execution. A matching four-surface projection is a contract check, not browser visual QA or live
transport evidence.
