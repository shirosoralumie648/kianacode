# BQ-25 redaction, data class and telemetry separation baseline

> Snapshot date: 2026-09-28. This slice owns the decision that **the same value has a different
> fate in each place it could be written**, and that the classification behind that decision is
> derived by the server rather than asserted by whoever produced the value. Local Cargo
> test/build/check/clippy/smoke commands were not run as tests; GitHub Actions owns fixtures and
> the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`BQ-25`](#step-bq-25) |
| code landing | `kiana-core/src/data_class.rs`, `kiana-core/src/telemetry_separation.rs`, registered in `kiana-core/src/lib.rs` |
| fixtures | `kiana-core/tests/bq25_telemetry_separation.rs`, `kiana-core/tests/bq25_telemetry_separation_guard.rs` |
| feature_status | `partial` for the source-level classification and separation contracts |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The problem this card is actually about

A prompt fragment is fine in a log line and fatal in a metric label. A user id is fine in a trace
attribute and ruins a metric, because every distinct value creates a new time series and the
cardinality explodes. A secret must appear in **none** of them. Treating "logging" as one thing is
what makes these leaks happen, so BQ-25 splits the destinations apart and gives each one its own
admission rule.

```text
a field (key, value, is_label, is_reference)
   ↓  derive_sink_class(field, sink)        <- server-side, per sink
   DataClass { Public | Internal | Confidential | Restricted }
   ClassBasis { BoundedLabel | BoundedText | OpaqueReference |
                HighCardinalityIdentifier | PayloadShape | SecretContent }
   ↓  admit_field / evaluate_sink_admission
   per-sink decision + a stable refusal
```

## Classification is derived, never accepted

`ClassClaimOrigin::CallerSupplied` exists **to be refused**. A field that arrives already classified
is not re-classified and downgraded — it is rejected, because a caller able to assert a class is a
caller able to assert `Public` for a prompt. This is the same reasoning as the rest of the
repository's trust boundaries, applied to telemetry: the untrusted side states facts, the server
derives conclusions.

The derivation order in `derive_sink_class` is fixed, and the order is the security property:

| Order | Condition | Class | Why it must be first |
|---|---|---|---|
| 1 | redaction changes the value, or a secret sentinel is found | `Restricted` | a secret outranks every shape heuristic |
| 2 | the key looks like payload (`prompt`, `content`, …) | `Restricted` | payload shape is restricted regardless of length |
| 3 | the field is a reference | `Internal` | a reference is an id, not content |
| 4 | label with a high-cardinality key (id, email, path, …) | `Confidential` | a metric label must stay low-cardinality |
| 5 | any other label | `Public` | bounded labels are the only thing metrics may carry |
| 6 | everything else | `Internal` | bounded free text is not public |

## Cardinality is a budget, and it is shared

`metric_label_budget()` and `metric_series_budget()` read the domain constants
`MAX_METRIC_LABEL_VALUES` / `MAX_METRIC_SERIES` rather than restating them, and
`observation_label_budget()` in the separation module delegates to the same bound. That is
deliberate: a source-level claim made in one sink must not be able to exceed what the runtime
guard already accepts in another. A limit that exists in two places is a limit that will drift.

The refusals `LabelForbidden`, `LabelValueLimit`, `LabelValueInvalid` and `SeriesLimit` are what
turn "high cardinality is bad practice" into a decision.

## The three observation sinks

`OBSERVATION_SINKS` is `[Log, Metric, Trace]`. Event and receipt are **not** in that list on
purpose: they are governed by the append-only schemas and the redaction contract that runs before
an event is admitted, not by the telemetry contract. Putting them in the same list would have
created a second, weaker path for the same data.

`telemetry_separation::separate` then keeps the three apart: a field may be admitted to a log line
and refused as a metric label in the same pass, `SubstitutionRefusal` covers a destination that is
not an observation sink, was never declared, or is the source itself, and `SinkObservation` records
whether each sink was reachable at all. **An unreachable sink stays `ObservationOutcome::Unknown`**
— it never defaults to "fine", because defaulting is how a missing exporter turns into a silent
data loss.

## Observation failure must not change business state

`is_actionable_refusal` is the split that keeps a telemetry refusal from becoming a business error.
A metric label that busts the cardinality budget means *that label is dropped*, not *the operation
that produced it failed*. Without this split, every observability limit would be a new way for
production traffic to fail, which is the failure mode the card names when it says cost/queue/retry
are low-cardinality metrics and traces are correlation refs only.

## Honest limitations

This is a source contract over supplied fields. It **does not** start an exporter, open a socket,
write a log line, register a metric, create a span, or prove that any real logging backend, metrics
backend or tracing implementation honours these rules. The classification is a decision over a
`SinkField` somebody constructed; nothing here observes how that field was produced, and a caller
that never routes its telemetry through this module is unaffected by it. Redaction reuses
`kiana_domain::redact_text` and `scan_secret_sentinels` rather than defining a second masker, but
this slice does not prove that every existing call site already uses them. Nothing is routed
through `ControlPlane::handle_command`, so the wiring from a real emit path into this decision is
still open, and no Event/Log/Metric/Trace/Receipt sink was migrated to enforce the report. There is
no exporter, no live backend and no durable evidence of any kind.
