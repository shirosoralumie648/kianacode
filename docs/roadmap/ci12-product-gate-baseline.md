# CI-12 product-chain deny-first and UAT baseline

## Scope

CI-12 adds a ten-scenario product evidence matrix for missing authentication, untrusted project,
cross-project scope, expired approval, secret leakage, TOCTOU drift, replay, result Unknown, fake
provider success and live opt-in. Denial cases require zero handler/provider/effect calls; Unknown
requires a receipt and reconciliation limitation; fake success remains offline; live opt-in needs
typed operator approval and provider evidence before any live claim.

The matrix reuses the existing DaemonHost → ControlPlane spine and entrypoint/company flow. It does
not create a second loop, provider client, approval path or external effect.

## CI-only evidence

`.github/workflows/ci12-product-gate.yml` runs formatting, the domain deny-first matrix, the Core
source guard and affected test-target compilation on GitHub Actions. Local Cargo tests, builds,
checks, clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `ci12_matrix_covers_deny_first_fake_success_and_live_opt_in_ceiling` | ten required scenarios exist; denial has zero effect and fake/live ceilings stay explicit |
| `forged_deny_effect_unknown_and_live_claims_fail_closed` | denial effect, Unknown missing receipt and live approval/evidence drift fail closed |
| `duplicate_scenario_and_secret_text_are_rejected` | duplicate scenario and non-secret invariant failures are rejected |

## Limitations and handoff

- The matrix is a source/fixture gate and does not run CLI/Web/Workbench/Desktop, a provider,
  external delivery, durable restart or physical effect.
- Live opt-in remains unverified until a separately authorized connection produces an independent
  evidence block; CI success cannot promote it.
- CI results are intentionally not awaited; proof level remains `source`.
