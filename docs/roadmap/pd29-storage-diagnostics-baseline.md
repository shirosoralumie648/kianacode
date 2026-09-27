# PD-29 - Storage health, projection lag and diagnostic DTOs baseline

> Snapshot date: 2026-09-28. This slice owns the *display* contract for storage health,
> projection lag, backup/migration/retention metrics and the diagnostic DTOs a UI renders. Local
> Cargo test/build/check/clippy commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`PD-29`](../roadmap.md) → `persistence-data-layer.md#step-pd-29` |
| source snapshot | master plus this PD-29 diagnostic slice |
| feature_status | `partial` for source-level diagnostic contract |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | store evidence → ordered status derivation → redacted diagnostic DTO → UI view |

`kiana-core/src/storage_diagnostics.rs` is a pure reducer over server-owned evidence. It opens no
store, reads no byte, schedules no maintenance and makes no admission decision. `kiana-daemon` is
unchanged by this step: the DTO is produced and validated in core, and a surface consumes it as
read-only input. Nothing in this slice adds a second execution path.

## Three refusals, enforced structurally

**A health report is not an authority.** The report carries `display_only: true` and
`StorageDiagnosticReport::authorizes` is a `const fn` hard-wired to `false`; the UI view repeats
the refusal as a checked `authorizes: false` field. Flipping either raises a named error rather
than being silently ignored. The source guard additionally forbids the words `permit`, `ControlPlane`,
`GateEngine` and `admission_allowed` from the module, so a later edit cannot quietly rejoin the
diagnostic to the authority path.

**Stale, unknown and corrupt never render as healthy.** Store status follows one fixed order —
corrupt → unavailable → unknown → degraded → `Ok` — and the aggregate report status is the *worst*
of the store status and every signal status. Three separate checks close the remaining gaps: a
checkpoint state that contradicts the cursor is refused, `Ok` with a non-empty limitation list is
refused, and a re-sealed report whose aggregate no longer matches its components is refused. A
"green with a caveat" report is therefore not representable, not merely discouraged.

**Output is redacted.** Every operator-visible string passes one validator: bounded length, no NUL,
not a serialized payload, `redact_text`-stable, `scan_secret_sentinels`-clean, and not a path. The
adapter-supplied incident `code` is deliberately never copied — only counts and classes cross the
boundary, so an adapter cannot smuggle a payload, a secret or a filesystem location into a UI.

## What the card rejects, and how

| Rejected | How |
|---|---|
| health used to authorize | `display_only` must be true (`storage_diagnostics_health_is_display_only`); a view claiming `authorizes` is refused (`storage_diagnostics_view_authorizes`); `authorizes()` cannot return anything but `false` |
| stale shown as healthy | projection lag and a non-caught-up checkpoint force `Degraded`; a checkpoint state contradicting the cursor is refused (`storage_diagnostic_checkpoint_state_contradicts_cursor`); a cursor ahead of the facts is refused (`storage_diagnostics_projection_cursor_ahead`) |
| unknown shown as healthy | `StorageHealthStatus::Unknown`, an open `Unknown`/`ResultUnknown` incident, or unclassified observations all force `Unknown`; a never-observed backup or migration is refused (`storage_diagnostic_maintenance_not_observed`) |
| corrupt shown as healthy | a corrupt store or an open `Corrupt` incident forces `Error`; a resolved incident stops degrading the store |
| payload / secret in output | note validator refuses serialized JSON, `redact_text` changes, sentinel hits and paths (`storage_diagnostic_note_payload` / `_secret` / `_path`); incident `code` is never copied |
| facts smuggled through metrics | the metric catalog is closed, its channel is part of the entry (`storage_diagnostics_metric_not_in_catalog`, `storage_diagnostics_channel_mismatch`), and reserved fact prefixes are refused outright (`storage_diagnostics_fact_signal_rejected`) |
| a report that does not match its evidence | `validate_against` re-derives the report from the sealed input (`storage_diagnostics_report_binding_mismatch`); cursors from two different stores are refused (`storage_diagnostic_input_cursor_mismatch`) |
| "healthy" maintenance that never ran | `Ok` requires a bounded `last_ok_at_unix_ms` (`storage_diagnostic_maintenance_not_observed`), and a failure newer than the last success is refused (`storage_diagnostic_maintenance_stale_status`); a non-`Ok` subject always carries a reason |

Logs, metrics and traces stay separate signals: the three channels are distinct catalog entries, so
the report shows channel attribution without any of them being able to restate a fact.

## Failure-first fixture matrix

`kiana-core/tests/pd29_storage_diagnostics.rs` — one test per card rejection.

| Fixture | Assertion |
|---|---|
| healthy baseline | a fully observed store renders `Ok`, binds every catalog signal to its channel, and still authorizes nothing |
| UI view | carries source/projection cursor, lag, generation, data and authority epoch, and the adapter's declared frame/batch limits |
| health is display only | `display_only = false` is refused; a view with `authorizes = true` is refused, standalone and rebound to a report |
| stale | projection lag forces `Degraded` with a `projection_lag_present` limitation; a contradicting checkpoint state is refused both ways |
| unknown | unknown store status, an open `ResultUnknown` incident, and an unknown checkpoint each keep the report off `Ok` |
| corrupt | a corrupt store and a corrupt open incident both report `Error`; a resolved incident does not |
| aggregate | a hand-flipped `Ok` aggregate is refused; `Ok` plus a limitation is not representable |
| non-durable adapter | missing `durable_commits` forces `Degraded` and surfaces in the view |
| redaction | payload, secret and path note fixtures are each refused with their own code; a projector id is held to the same rule |
| no adapter text | an incident code containing a secret never appears in the serialized report, and every limitation is redaction-clean |
| fact/metric separation | fact-prefixed names, an uncatalogued name and a channel swap are each refused |
| maintenance | never-verified backup, `Ok` with a newer failure, a reasonless `Degraded`, and a future-dated observation are each refused; a blocked retention sweep surfaces bounded counters |
| input binding | mismatched cursors, a duplicate incident, a tampered digest and a report rebuilt from different evidence are each refused |
| round trip | the DTO re-decodes identically, re-validates against its input, and still authorizes nothing |

`kiana-core/tests/pd29_storage_diagnostics_guard.rs` is the source guard: it asserts the display-only
constant and the `const fn` body, the corrupt → unavailable → unknown ordering by index position, the
redaction markers, the closed-catalog/channel binding, the UI view fields, the module's absence of
every effect boundary, and the presence of the registration lines in `kiana-core/src/lib.rs`.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

This step is a source contract only. It does **not** prove, and must not be described as:

- **runtime or durable behaviour** — no adapter produces `StorageHealth`, `StorageProjectionLag`,
  `StorageMaintenanceObservation` or `StorageIntegrityIncident` values yet. The input is assembled
  by a caller; nothing here collects it from a real store;
- **live measurement** — the log/trace counters are emitted as `0` with a channel status inherited
  from incident state. They carry no observed volume, and a `0` here means "not reported", not
  "nothing happened";
- **a real backup / migration / retention pipeline** — the maintenance observations are adapter-reported
  counters. Nothing is scheduled, copied, applied, pruned or verified here, and this slice does not
  connect to PD-22/23/24/25 effects;
- **observability backends** — no exporter, collector, scrape endpoint or alert rule is implemented.
  The catalog is a name/unit/channel contract, not a metrics pipeline;
- **UI work** — `StorageDiagnosticUiView` is a DTO. No CLI, web or workbench surface renders it yet,
  so the "UI can show cursor/generation/limits" success column is satisfied at the type level only;
- **cross-store or cross-process validity** — the cursors, generation and epochs are bound within one
  sealed input. Nothing compares a report against another instance's store;
- **an authority for admission** — health remains display-only by construction; admission stays with
  the control plane and the gate/policy engines.

Status stays `⏳` on the card. The earned change is `PD-29` source slice complete, proof level
`source`, with the wiring in deliverables 3 and 4 below still outstanding.
