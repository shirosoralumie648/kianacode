# P4-J7-28 · Provider selection, diagnostics and event projection baseline

This source slice defines the shared, secret-free provider catalog/configuration/diagnostics view
used by the protocol client and all UI surfaces. It is a projection contract; it does not open a
provider connection, perform inference, authorize an action or replace the EventLog/ControlPlane.

## Source contract

- `ProviderDiagnosticsSnapshot` carries the existing `ProviderConfigSnapshot` and `ModelCatalog`,
  explicit `ProviderConfigCheckState` (`saved`, `static_validated`, `live_verified`), bounded
  connection/model rows, stream display mode (`native`, `synthetic`, `buffered`), status values
  (`queued`, `retrying`, `cancelling`, `failed`, `terminal`), queue/retry/cancellation/error fields
  and explicit known/unknown usage.
- `ProviderConnectionTestRequest` requires an explicit gateway admission digest. A settings or
  catalog read cannot manufacture this request, and secret values never appear in any DTO.
- `ProviderDiagnosticsCursor` binds a non-zero sequence to authority and configuration epochs.
  A stale epoch or non-contiguous reconnect sequence is rejected and requires a fresh snapshot.
  `ProviderTerminalReplay` exposes only event/receipt digests so a late subscriber can observe a
  committed terminal without issuing another model request.
- `kiana-core::project_provider_diagnostics` and
  `kiana-core::replay_provider_terminal` remain read-only projection helpers. The typed client
  clears its state on cursor gaps and rejects stale reconnects; it never retries provider work.

## CI-only fixtures

`.github/workflows/p4-j7-28-provider-diagnostics.yml` runs the domain fixtures, Core source guard,
formatting and client/protocol/core test-target compilation on GitHub Actions. Its path filter
includes current CM-36 `kiana-domain/src/memory_workbench.rs` so a fresh remote run can clear the
repository-wide fmt dependency; that result is pending and unobserved. Local tests, builds, checks,
clippy and smoke commands are intentionally not run.

The fixtures cover secret-free catalog/configuration round trips, zero/non-contiguous cursor
rejection, stale authority/config epoch rejection, explicit connection-test admission, terminal
replay without a second model request and unknown usage/actionable error states. The client
fixture proves a future cursor gap clears the previous projection; the source guard checks the
single projection boundary and forbids provider clients, network calls and secret fields in the
domain contract.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus GitHub CI wiring. It does
not claim a live three-surface transport integration, durable cross-process projection checkpoint,
provider invoice truth, real connection test, live model effect or physical capability proof.
Existing catalog/smoke commands and future UI handlers must route explicit tests through the current
ProviderGateway/ControlPlane admission path before promotion beyond this source evidence.
