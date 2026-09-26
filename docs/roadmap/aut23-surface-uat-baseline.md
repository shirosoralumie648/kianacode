# AUT-23 five-surface DaemonHost UAT baseline

## Scope

AUT-23 adds a source-level UAT parity contract for CLI, Web, Workbench, Desktop and MCP. Each
surface carries the same command digest, AutomationSnapshot digest, source cursor and authority
epoch; denied requests require zero Broker/handler calls, and direct alternate routes are refused.
The contract reuses the existing DaemonHost/ControlPlane spine and AUT-22 snapshot.

## CI-only evidence

`.github/workflows/aut23-surface-uat.yml` runs formatting, domain surface fixtures, the Core spine
guard and affected test-target compilation on GitHub Actions. Local Cargo tests, builds, checks,
clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `five_surfaces_share_one_snapshot_and_denial_has_zero_calls` | five surfaces share cursor/digest/epoch and deny has zero effects |
| `foreign_snapshot_or_direct_route_is_rejected` | snapshot drift and alternate direct route fail closed |
| `missing_surface_and_denied_effect_are_rejected` | incomplete surface set and forged effect are rejected |

## Limitations and handoff

- This is a source/fixture parity contract, not browser/PTY/Electron/MCP E2E or cross-process
  durable UAT; no provider or physical effect is executed.
- AUT-24 durable/live release evidence remains open and requires separate evidence blocks.
- CI results are intentionally not awaited; proof level remains `source`.
