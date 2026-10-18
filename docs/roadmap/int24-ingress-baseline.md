# INT-24 Webhook/A2A ingress baseline

> Snapshot date: 2026-10-19. This slice adds a bounded, signed Webhook/A2A occurrence contract, a
> daemon verifier, a durable replay window behind a port, and the missing production bridge from a
> verified occurrence into `ControlPlane`. Roadmap row 579 stays `🔄`: the ControlPlane route now
> exists and is tested, but the card's `event→occurrence→Workflow/Run` and "signature evidence
> queryable" exit conditions are not both met (see Limitations).

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`INT-24`](../roadmap/integrations-connectors.md#step-int-24) |
| worktree / branch | `D:/code/kiana-steps/int24`, `step/int24-20261018-1` |
| feature_status | `partial`: auth/signature/timestamp/nonce/durable dedupe and the ControlPlane route are implemented; key provisioning/rotation, rate limiting, Workflow/Run routing and a query surface are not |
| proof_level | `local_behavior` for the focused targets below; `source` for everything else |
| canonical path | untrusted Webhook/A2A event + server clock → daemon source/key/HMAC verifier → durable replay port → typed occurrence → server-resolved binding/intent → `DaemonHost::handle` → `ControlPlane` → Broker |

### What changed in this slice

1. **The clock check is real.** `ConnectorIngressOccurrence::from_verified(event, policy, now_unix_ms)`
   takes the server clock as a caller parameter. It previously derived "now" from
   `event.occurred_at_unix_ms`, which the sender controls, so the skew check could never fail.
   The daemon verifier passes the admission-time server clock.
2. **Durable, bounded replay.** `kiana-ports::ConnectorIngressReplayStore` is the port;
   `JsonlConnectorIngressReplayStore` (`kiana-daemon/src/connector_ingress_replay.rs`) is the file
   adapter. Entries are digest-only (`source_id`, `event_id`, `nonce`, `ingress_digest`,
   `accepted_at_unix_ms`, `expires_at_unix_ms`, self-digest). The store is bounded by time (default
   24 h, max 30 d) and by 100 000 entries; when full it prunes expired entries and then fails closed
   with `connector_ingress_replay_capacity_exceeded` rather than evicting live evidence. Writes are
   temp-file + rename + fsync under an OS-level exclusive lock (flock / LockFileEx), so a daemon
   restart or a second process cannot accept the same occurrence again.
3. **Occurrence → ControlPlane route.** `DaemonHost::with_connector_ingress_route` attaches a
   verifier and a server-owned `ConnectorIngressRouteConfig`
   (`kiana-daemon/src/connector_ingress_route.rs`). `DaemonHost::handle_connector_ingress(event, now)`
   verifies, re-checks the payload digest against the verified occurrence, re-checks project trust,
   resolves the `AccountBinding` from the EventLog, derives
   `ConnectorIngressIntent::from_verified_occurrence`, and submits a normal
   `RequestEnvelope::command("connector.invoke")` through `DaemonHost::handle`. The route never
   calls a connector adapter, the Broker or the EventLog directly; there is one execution edge.
   The handle future is boxed so the route's own frame stays small (Windows debug stack).
4. **Dispatch lifecycle idempotency (necessary to reach the Broker).** The shared
   `ConnectorEventJournal::append_lifecycle` appended the four dispatch-lifecycle facts without an
   idempotency key, so every `append_idempotent_expected` was rejected with
   `event_idempotency_key_required` before the fixture adapter ran. Each stage now carries
   `connector-dispatch-lifecycle:<invocation_id>:<attempt>:<stage>`, which keeps same-stage retries
   idempotent while still allowing the next stage to append.

Deny-first order in `handle_connector_ingress`: route availability → wire source vs route source
(`connector_ingress_source_route_mismatch`) → structure/source allowlist/tenant/event kind/clock/HMAC
→ durable replay (`Accepted` / `Duplicate` / `connector_ingress_dedupe_payload_conflict` /
`connector_ingress_nonce_replay`) → payload digest (`connector_ingress_payload_digest_mismatch`) →
project trust (`project_untrusted`) → binding resolution (`connector_binding_missing`,
`connector_binding_scope_mismatch`, `connector_binding_revoked`, `connector_operation_unregistered`)
→ intent/audience → `ControlPlane`. Every refusal returns before `ControlPlane`; a `Duplicate`
returns `broker_calls = 0` and `response = None`.

## Failure-first fixture matrix

`kiana-daemon/tests/int24_connector_ingress_route.rs` asserts EventStore fact counts around each
refusal, which is what proves `ControlPlane` was never entered (the route returns before `handle`).

| Order | Fixture | Stable refusal / outcome |
|---|---|---|
| 1 | `stale_occurrence_is_refused_by_the_server_clock_before_any_effect` (server now is 5 s past the event; 100 ms skew policy) | `connector_ingress_clock_skew` |
| 2 | `nonce_replay_and_payload_conflict_are_refused_before_any_effect` (second event reuses the nonce) | `connector_ingress_nonce_replay` |
| 3 | same fixture, same `source_id+event_id` with a different payload | `connector_ingress_dedupe_payload_conflict` |
| 4 | `route_mismatch_and_untrusted_project_are_refused_before_any_effect` (wire source ≠ route source) | `connector_ingress_source_route_mismatch` |
| 5 | same fixture, trusted authority returns `false` | `project_untrusted` |
| 6 | `durable_replay_survives_a_daemon_restart_and_never_reaches_control_plane_twice` (second host, same replay file) | `duplicate = true`, `broker_calls = 0`, no new fact |
| 7 | `admitted_occurrence_enters_control_plane_and_completes_through_the_broker` (success, last) | `Completed`, `receipt.outcome = succeeded`, `external_effect_performed = false`, `connector.invoked` fact present |

Other targets:

| Target | Coverage |
|---|---|
| `kiana-domain/tests/connector_ingress.rs` | occurrence construction with an explicit server clock; existing contract fixtures |
| `kiana-domain/tests/sc17_connector_intent.rs` | intent derivation still requires the verified occurrence |
| `kiana-daemon/tests/int24_connector_ingress.rs` | in-process verifier: authentication, same-digest idempotent replay, payload conflict, bad signature, unknown source |
| `kiana-daemon/tests/int24_connector_ingress_guard.rs` (2 tests) | source guard extended to scan `connector_ingress_replay.rs` and `connector_ingress_route.rs` with the original forbidden list plus EventLog/Broker markers; a second test pins the single `DaemonHost::handle` execution edge and the route's refusal codes |
| `kiana-daemon/tests/int19_connector_dispatch.rs`, `int23_connector_cancellation.rs` | compile gate for the shared connector spine (Unix-only behaviour tests) |

## Evidence

Commands run locally on 2026-10-19 (Windows 11, offline, worktree `D:/code/kiana-steps/int24`):

```text
cargo test -p kiana-domain --test connector_ingress --offline                                  -> 2 passed
cargo test -p kiana-daemon --test int24_connector_ingress --offline -- --test-threads=1        -> 2 passed
cargo test -p kiana-daemon --test int24_connector_ingress_guard --offline                      -> 2 passed
RUST_MIN_STACK=8388608 cargo test -p kiana-daemon --test int24_connector_ingress_route --offline -- --test-threads=1 -> 5 passed
cargo check -p kiana-daemon --tests --offline                                                  -> exit 0
rustfmt --edition 2021 --check <touched files>                                                 -> exit 0
```

`int16/int17/int18/int19_connector_*_guard` were re-run and stay green after the lifecycle
idempotency-key change.

The route binary needs `RUST_MIN_STACK=8388608` on Windows: the debug test stack is 1 MiB and the
composed `DaemonHost::handle → ControlPlane → policy → Broker` future (which the success fixture
must actually execute) overflows it. The route boxes its own `handle` future, but the remaining
composition still exceeds 1 MiB. This is a Windows debug-stack limitation, not a second execution
path; the `int23` fixtures need the same override.

## CI

`.github/workflows/int24-ingress.yml` is `workflow_dispatch`-only with `permissions: contents: read`.
It runs the fmt gate, the domain target, all three daemon targets, and compiles the connector
dispatch/cancellation spines. CI results are not awaited.

## Limitations

- **Row 579 stays `🔄`.** The route into `ControlPlane` exists and is tested (the stated flip
  condition), but the card's success column also requires `event→occurrence→Workflow/Run` replay and
  queryable signature evidence. The route submits `connector.invoke`, not a Workflow/Run command,
  and there is no CLI/web query surface over the replay window (`replay_window` exists on the
  verifier but no entrypoint exposes it). Those two items are missing.
- Key provisioning/rotation by `key_id` is not implemented: the verifier accepts one static key per
  source at construction time. A key id is authenticated but there is no rotation state, overlap
  window, or revocation record.
- Per-source rate limiting is not implemented. `ConnectorDefinition::rate_limit_per_minute` is a
  connector-side quota, not an ingress limiter.
- The replay store is a single file with a global lock; it fails closed at capacity instead of
  sharding. Worst-case retained bytes are bounded by the entry count, not by a byte budget.
- No network HTTP/A2A listener: the route is a `DaemonHost` method. Wiring a real listener is a
  separate entrypoint slice and must still call `handle_connector_ingress`.
- No external effect, live provider evidence, or cross-process test: the success fixture uses the
  `local_fixture` adapter and asserts `external_effect_performed = false`.
- The dispatch-lifecycle idempotency key change touches shared connector code; `int19`/`int23`
  behaviour tests are Unix-only in this checkout, so on Windows only their compilation is proven.
- `int23`'s second test (`a_settled_attempt_reconstructs_from_durable_facts_and_fences_late_results`)
  failed on this Windows host with `action_authority_missing` when the disk-host registry `list`
  ran. It is unrelated to this slice (the failure reproduces at the merge base), so it is recorded
  rather than fixed here.
