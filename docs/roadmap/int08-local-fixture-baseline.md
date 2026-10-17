# INT-08 local fixture baseline

## Scope

INT-08 makes the existing local connector fixture contract explicit and domain-owned. It keeps the
fixture adapter behind the existing `DaemonHost → ControlPlane → Broker → EventStore` path and does
not add a network transport, a credential store, or a second execution loop.

## Implemented source slice

- `ConnectorFixture` and `ConnectorFixtureCase` implement strict
  `kiana.connector-fixture.v1` decoding with a 1 MiB byte bound, at most 32 operations and at most
  128 cases per operation. Unknown JSON fields, malformed identifiers and empty case lists fail
  closed.
- Fixture bytes are checked against the binding's SHA-256 before decoding. The adapter accepts the
  historical `sha256:<hex>` spelling for compatibility but emits and compares the canonical raw
  64-character digest. Replacing the file or supplying a different digest returns
  `connector_fixture_hash_mismatch`.
- Payload matching canonicalizes object keys and rejects duplicate canonical payloads. An operation
  absent from the fixture and a payload absent from its operation have separate stable errors.
- A fixture or case marked `external_effect=true` is rejected. Successful, failed and unknown cases
  are projected to deterministic `ProviderReceipt` values with canonical payload hashes,
  `source=local_fixture`, redacted result data and no external effect claim.
- Existing daemon bind, invoke, idempotent replay and explicit unknown reconciliation continue to
  append through EventStore CAS; the daemon only loads and validates the domain fixture contract.

## CI-only evidence

GitHub Actions runs the domain deny/success fixtures, the core/daemon source boundary guard and a
targeted compile for the local fixture path. Local cargo tests, builds, checks, clippy and smoke
commands are intentionally not run and CI is not awaited.

## Evidence boundary and limitations

```text
feature_status: implemented
proof_level: source
```

The fixture and receipt are deterministic local observations. They do not prove an external provider
received an effect, exactly-once behavior across processes, durable connector registry recovery, or
live/physical business outcomes. HTTP/OAuth/MCP remote transports, provider query receipts and
cross-process lease fencing remain later INT steps.

## 2026-10-17 completion slice (CI-only)

The completion slice turns the source contracts into deny-first acceptance:

- Domain deny matrix: `connector_fixture_operation_limit` (33 operations and empty operation map),
  `connector_fixture_case_limit` (129 cases and empty case list), unknown fields at fixture and case
  level → `connector_fixture_invalid`, `kiana.connector-fixture.v2` →
  `connector_fixture_schema_unsupported`, malformed connector/account identities →
  `connector_fixture_identity_mismatch`, malformed `receipt_id` → `connector_fixture_invalid`, and a
  fixture operation that the definition does not register → `connector_fixture_operation_unregistered`.
- Hash pinning: both the historical `sha256:<hex>` spelling and the raw 64-character lower-case hex
  digest are valid; uppercase spelling and `sha256:not-a-digest` are rejected, and replaced bytes
  fail `connector_fixture_hash_matches` for both spellings while the pinned bytes still match.
- Determinism: payload matching canonicalizes object-key order, and succeeded / failed / unknown
  cases produce byte-identical `ProviderReceipt` values across repeated calls, with
  `final_payload_sha256` equal to the canonical payload digest.
- Daemon behaviour: `DaemonHost → ControlPlane → Broker → EventStore` fixtures drive an approved
  bind (challenge risk `ExternalSideEffect`), a directly allowed read-only invoke, same-key replay
  (original receipt and event id, `replayed=true`), an unknown invocation that stays
  `ResultUnknown` with one `unresolved` entry, and an approved `action=reconcile` that consumes a
  hash-pinned manual receipt file and appends a `connector.reconciled` fact without overwriting the
  original unknown event. Replaced bytes, a missing definition operation and an unregistered
  fixture operation are refused with zero dispatch and an unchanged registry.
- Deny fixtures: `kiana-domain/tests/int08_local_fixture.rs`
  (`fixture_schema_bounds_and_unknown_fields_fail_closed`,
  `fixture_replacement_and_hash_spelling_are_pinned`,
  `fixture_receipts_are_byte_stable_for_every_outcome`) and
  `kiana-daemon/tests/int08_local_fixture.rs`
  (`fixture_bind_invoke_replay_and_reconcile_are_deterministic`,
  `fixture_identity_replacement_and_unknown_operation_are_refused_before_any_effect`). Source guard:
  `kiana-core/tests/int08_local_fixture_guard.rs` (deny vocabulary, fixture obligations, local-only
  negative markers). Manual lane: `.github/workflows/int08-local-fixture.yml`
  (`workflow_dispatch`-only; fetch, fmt gate, three INT-08 targets, two existing connector
  regressions and `--no-run` compiles).
- Automatic coverage: the `ci.yml` whole-crate shards for `kiana-domain` / `kiana-core` /
  `kiana-daemon` discover the new integration targets. No manifest/lockfile/package.json change, no
  new dependency, no second execution loop, and the five model-visible tools are unchanged.

Evidence stays `feature_status=implemented`, `proof_level=source`; this slice runs no local
tests/build/check/clippy/smoke (only `cargo fmt --all`), GitHub Actions remains the test authority
and its receipt is not awaited. Still open: HTTP/OAuth/MCP remote transports, provider query
receipts, cross-process idempotent replay, durable connector registry recovery and live/physical
connector effects.
