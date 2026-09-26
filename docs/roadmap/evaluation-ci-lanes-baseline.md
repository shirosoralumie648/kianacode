# EQ-50 evaluation CI lanes baseline

## Scope

EQ-50 separates evaluation CI into three GitHub workflows:

- `eq50-pr-curated.yml` runs on pull requests, checks the EQ-49 wrappers, compiles the curated
  package set and runs daemon/core fixtures serially.
- `eq50-nightly-deep.yml` is scheduled, but the deep lane requires an explicit
  `workflow_dispatch` boolean `deep_enabled=true`; an unconfigured run exits non-successfully
  instead of silently passing.
- `eq50-release-candidate.yml` runs for version tags or an explicit `release_candidate=true`
  dispatch and applies the same serial daemon/core boundary.

All lanes use read-only GitHub permissions, the locked toolchain and explicit path scopes. They do
not pass ambient provider credentials, proxy variables or MCP configuration to the EQ-49 wrappers;
the wrappers provide their own `env -i` allowlist and route through the existing ControlPlane
command surface.

## Evidence and limits

- `scripts/tests/eq50-ci-lanes-static.sh` checks lane triggers, read-only permissions, explicit
  opt-in markers, serial test flags, the EQ-49 static guard and forbidden ambient variables.
- `.github/workflows/eq50-*.yml` are CI-only lanes. Local Cargo tests/build/check/clippy/smoke
  commands were not run and remote CI results are not awaited.

This slice is `feature_status=partial`, `proof_level=source`: it proves workflow configuration and
fail-closed opt-in rules only. It does not prove a durable EvalStore, real evaluation execution,
provider quality, report archival, promotion, live or physical outcomes.
