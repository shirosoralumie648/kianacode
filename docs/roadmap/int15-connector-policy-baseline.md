# INT-15 Connector operation risk to policy/gate/approval baseline

## Scope

INT-15 maps the server-owned connector operation contract to the integration R0--R4 policy
vocabulary and reuses the existing ControlPlane policy, Gate and approval spine. R0/R1 remain
read-only, R2 requires a current data-processing grant, R3 pauses for one exact final-payload
approval, and R4 is default deny.

## Implemented source slice

- `ConnectorOperationRisk` and `connector_operation_risk` derive R0/R1/R2/R3/R4 from the immutable
  operation name/effect. Caller risk labels can only be rejected as downgrades.
- `ConnectorDataGrant` binds owner, project, data classes, data epoch, authority/policy epochs and
  expiry. `ConnectorOnceApproval` binds operation, one final payload digest, authority/policy
  epochs, expiry and single consumption state.
- `evaluate_connector_admission` checks binding status/expiry, authority and policy epochs,
  payload digest, data grant and once approval in a fixed order. Revoked, expired, mismatched,
  stale or default-denied decisions carry `broker_calls = 0`.
- ControlPlane stamps `final_payload_digest` from canonical payload before entering the existing
  `authorize_and_execute` path. `kiana-policy` maps the typed admission to Allow/Ask/Deny and the
  existing `kiana-gates` and approval continuation remain the only gate/approval authorities.
- Domain and policy fixtures plus a Core source guard cover deny-first behavior, deterministic R0/R1,
  R2 grant, R3 once approval/digest and R4 default deny. No connector adapter or second execution
  loop was added.

## Current-head payload digest encoding correction (2026-10-08)

`connector_payload_sha256` and the ControlPlane's `final_payload_digest` are deliberately bare
64-hex SHA-256 values, while older binding snapshots may use the historical `sha256:<hex>` form.
`ConnectorOnceApproval::validate` now accepts both forms for `payload_digest` through the shared
`connector_fixture_hash_valid` helper, while `approval_digest` remains strict `sha256:<hex>`.
Malformed payload digests remain denied. The INT-15 domain target is green at 6/6, including the
existing R3 final-payload/one-shot fixture and the bare/prefixed boundary fixture; the Core source
guard is green at 1/1.

## 2026-10-17 completion slice (CI-only)

The 2026-10-17 slice closes the hole the earlier session left in `kiana-core/tests/
int15_connector_policy_guard.rs`: that guard already `include_str!`-ed a daemon behaviour fixture
and a manual lane that did not exist, so the guard target could not even compile.

- `kiana-daemon/tests/int15_connector_policy.rs` drives the production
  `DaemonHost → ControlPlane → Broker → EventStore` path over a hash-pinned `local_fixture`
  binding (one server-owned operation per risk class) and the existing approval journal.
  `read_only_risk_is_allowed_and_denials_never_dispatch` proves R0/R1 read-only invokes complete
  without any approval, that R2 without a server-issued data grant is denied
  (`connector_data_grant_required`), that a wire-supplied `data_grant`/`connector_approval`
  argument is refused as `connector_arguments_invalid`, and that the R4 `delete_account` operation
  is denied (`connector_r4_default_denied`); every denial leaves the `connector.invoked` and
  `execution.result_committed` counters, the binding revision and the connector stream unchanged.
  `final_payload_approval_is_one_shot_and_payload_bound` proves R3 stages
  `connector_final_payload_approval_required` with zero effect, that a different final payload
  yields a different approval identity and request hash, that a drifted request hash or nonce is
  refused (`approval_request_hash_mismatch` / `approval_nonce_mismatch`) with zero effect, that
  only the exact staged final payload dispatches once with a receipt digest equal to
  `connector_payload_sha256(payload)`, that re-deciding the consumed approval is refused
  (`approval_not_active`) without a second dispatch, and that repeating the payload needs a fresh
  approval.
- `.github/workflows/int15-connector-policy.yml` is the manual `workflow_dispatch` lane for the
  domain/policy/daemon fixtures plus the Core source guard and the four target compilations.
- Automatic coverage stays on the unified `ci.yml` shards: `kiana-domain-s3/4`
  (`int15_connector_policy`), `kiana-core-s4/6` (`int15_connector_policy_guard`) and the whole-crate
  `kiana-policy` / `kiana-daemon` shards (`targets: null` auto-discovery). `scripts/ci/
  test-shards.json` is unchanged.

No `Cargo.toml` / `Cargo.lock` / `package.json` change, no new dependency and no second execution
loop. Local tests, build, check, clippy and smoke commands were not run; only `cargo fmt --all`
was executed and CI is not awaited.

## CI-only evidence

GitHub Actions runs the domain/policy/daemon fixtures, Core source guard, formatting and workspace
test target compilation. Local Cargo tests, build, check, clippy and smoke commands are
intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The mapping and deny contract are source-level fixtures. Durable grant/approval consumption,
reservation/CAS, effect-time authority/data fencing, provider receipts, external transport and
live/physical connector outcomes remain later INT/CP/ER/PD work. Operation-name classification is
conservative for the existing binding projection; richer INT-03 operation contracts can provide
their server-owned declared risk without widening the caller surface.

The digest compatibility correction is still a local validation boundary: it does not prove durable
approval consumption, external delivery, provider receipt reconciliation or live/physical connector
behavior.
