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
- `ProviderConfigCheck::{saved, static_validated, live_verified}` keep the three states distinct.
  Saving or static validation never contacts a provider; live verification requires an explicit
  gateway evidence digest.
- `ProviderSurfaceProjection` is the single renderer input for CLI, Workbench, Web and Desktop.
  `shared_digest()` excludes only the surface label, so every surface shows the same catalog,
  config state, entries, actionable errors, cursor and terminal replay. Desktop reuses the Web
  projection (`ProviderSurface::shares_web_projection`).
- `ProviderSettingsView::read_only` exposes no connection test. An explicit
  `ProviderConnectionTestRequest` must carry the catalog/config digests, authority/config epochs,
  `explicit=true`, `allow_inference=true` and the gateway admission digest before
  `connection_test_plan` can exist. A settings or catalog read cannot manufacture that request.
- `admit_provider_upstream_effect` returns `None` for offline catalog/Fake smoke and requires the
  same explicit request for live catalog/smoke/connection tests. This is the admission-shaped
  contract for the existing `model catalog` / `model smoke` call chain; it never opens a socket.
- `ProviderDiagnosticsCursor` binds a non-zero sequence to authority and configuration epochs.
  A stale epoch or non-contiguous reconnect sequence is rejected and requires a fresh snapshot.
  `ProviderTerminalReplay` exposes only event/receipt digests so a late subscriber can observe a
  committed terminal without issuing another model request.
- `ProviderUiAction` binds the displayed run/epoch/sequence. A stale action is rejected with
  `provider_ui_action_stale`; only `cancel` declares `mutates_run`, and it still requires the
  matching run id. Display refresh/retry never mutate a run.
- `ProviderStreamDisplay` folds already-observed frames into native/synthetic/buffered state. It
  has no gateway handle and no method that could send a model request; `request_attempts` can be
  `0` or `1` only, so collecting a stream cannot produce a second attempt.
- `kiana-core::project_provider_diagnostics` and
  `kiana-core::replay_provider_terminal` remain read-only projection helpers. The typed client
  clears its state on cursor gaps, rejects stale reconnects/epochs/actions and never retries
  provider work.
- `kiana-entrypoints::provider_diagnostics` exposes `render_provider_surface`,
  `render_provider_settings`, `render_provider_connection_test` and `render_provider_config_check`.
  Rendering validates first and serializes only the diagnostic view; protocol bodies and credential
  values stay outside the boundary.

## Acceptance fixtures

Domain (`kiana-domain/tests/p4_j7_28_provider_diagnostics.rs`):

- `three_surfaces_show_same_provider_diagnostics`
- `model_settings_view_does_not_trigger_inference`
- `stale_provider_ui_action_cannot_mutate_active_run`
- `late_subscriber_observes_terminal_from_receipt`
- `no_stream_collects_without_second_model_request`
- `live_provider_effects_require_explicit_gateway_admission`
- `save_static_validate_and_live_verify_are_distinct_states`
- existing `snapshot_binds_catalog_config_and_cursor_without_secrets`,
  `authority_or_config_epoch_change_discards_projection`,
  `diagnostics_cursor_rejects_zero_sequence`,
  `settings_view_cannot_create_connection_test_admission`,
  `terminal_receipt_replays_without_another_model_request`,
  `unknown_usage_and_actionable_error_are_explicit`

Client (`kiana-client/tests/p4_j7_28_provider_diagnostics.rs`):

- `web_reconnect_never_retries_model_request`
- `settings_view_is_read_only_until_an_explicit_connection_test`
- `stale_provider_ui_action_cannot_mutate_active_run`
- `late_subscriber_observes_terminal_from_receipt`
- existing `reconnect_rejects_a_future_sequence_gap_and_clears_old_projection`

Entrypoints (`kiana-entrypoints/tests/p4_j7_28_provider_surfaces.rs`):

- `three_surfaces_show_same_provider_diagnostics`
- `model_settings_view_does_not_trigger_inference`
- `late_terminal_replay_and_buffered_collection_use_one_request`
- `rendered_surfaces_are_serializable_json_objects`

Core source guard (`kiana-core/tests/p4_j7_28_provider_diagnostics_guard.rs`) binds the markers,
the fixture names above and the manual lane to the domain/client/entrypoints/core files.

## CI-only fixtures

`.github/workflows/p4-j7-28-provider-diagnostics.yml` is a manual (`workflow_dispatch`-only) lane:
it runs `cargo fetch --locked`, `cargo fmt --all --check`, the domain and client fixtures, the
entrypoints surface fixtures, the Core source guard and a touched-crate `cargo check --tests`.

Automatic coverage stays in the unified `ci.yml` shards: `kiana-domain-s3/4` claims
`p4_j7_28_provider_diagnostics`, `kiana-core-s5/6` claims
`p4_j7_28_provider_diagnostics_guard`, and the whole-crate `kiana-client` / `kiana-entrypoints`
shards run both client and surface fixtures. Local tests, builds, checks, clippy and smoke commands
are intentionally not run (CI-only execution policy).

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus GitHub CI wiring. It does
not claim a live three-surface transport integration, browser/TTY/Desktop rendering, a durable
cross-process diagnostic checkpoint, provider invoice truth, a real connection test, live model
effect or physical capability proof. The admission helper is the contract the existing
catalog/smoke handlers must call before a live effect; wiring the daemon-side gateway dispatch and
actual UI caches remains in the later product-chain step. Offline catalog reads and Fake smoke need
no admission and are unaffected.
