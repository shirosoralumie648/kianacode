//! BQ-27 failure-first fixtures: legacy usage/cassette/config upcasters and migration.
//!
//! One named test per item in the card's rejected-first column, plus the success case the card
//! asks for. Nothing here runs a migration, touches a filesystem or proves a store migrated.

use kiana_domain::*;
use serde_json::{json, Value};

fn digest(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn usage_record(input: Option<u64>, output: Option<u64>) -> UsageRecord {
    UsageRecord {
        event_id: EventId::new(),
        request_id: RequestId::new(),
        run_id: RunId::new(),
        step: 1,
        provider_id: Some("provider:test".to_owned()),
        model_id: Some("model:test".to_owned()),
        input_tokens: input,
        output_tokens: output,
        elapsed_ms: 5,
    }
}

fn cost_ledger(cost_micros: u64, input: Option<u64>, output: Option<u64>) -> LegacyCostLedger {
    LegacyCostLedger {
        records: vec![usage_record(Some(12), Some(4))],
        input_tokens: input,
        output_tokens: output,
        cost_micros,
    }
}

fn cost_request(payload_schema: &str, payload_digest: &str) -> LegacyUpcastRequest {
    LegacyUpcastRequest::new(
        LegacyPayloadKind::CostLedger,
        payload_schema,
        payload_digest,
        Some("backup:bq27-fixture".to_owned()),
    )
    .expect("valid upcast request")
}

fn cost_report(cost_micros: u64, payload_digest: &str) -> LegacyUpcastReport {
    let request = cost_request(LEGACY_COST_LEDGER_SCHEMA, payload_digest);
    LegacyUpcastReport::evaluate_cost_ledger(&request, &cost_ledger(cost_micros, Some(12), Some(4)))
        .expect("legacy cost ledger upcasts")
}

/// Card rejection 1: an unknown major must not be silently accepted.
///
/// A declared schema that is not the registered legacy source — including another major of a
/// family this module knows, and including a name that is not a schema at all — is a typed
/// refusal. Nothing is parsed, and `classify_legacy_schema` says the same thing.
#[test]
fn rejects_an_unknown_schema_major_instead_of_best_effort_parsing() {
    for unknown in [
        "kiana.cost-ledger.v9",
        "kiana.cost-ledger.v1",
        "kiana.quota.v7",
        "kiana.provider-config.v2",
        "not-a-schema",
        "",
    ] {
        assert_eq!(
            classify_legacy_schema(unknown).unwrap_err(),
            "legacy_upcast_unknown_major",
            "unknown major must be refused, not parsed: {unknown}"
        );
    }
    // A same-major, different-name payload on a known family is still refused.
    let request = cost_request("kiana.cost-ledger.v0 ", &digest('a'));
    assert_eq!(
        LegacyUpcastReport::evaluate_cost_ledger(&request, &cost_ledger(0, Some(12), Some(4)))
            .unwrap_err(),
        "legacy_upcast_unknown_major"
    );
    // The known source and the known destination are the only two names that classify.
    assert_eq!(
        classify_legacy_schema(LEGACY_COST_LEDGER_SCHEMA).unwrap(),
        LegacySchemaClass::LegacySource(LegacyPayloadKind::CostLedger)
    );
    assert_eq!(
        classify_legacy_schema(NORMALIZED_USAGE_SCHEMA).unwrap(),
        LegacySchemaClass::CurrentDestination(LegacyPayloadKind::CostLedger)
    );
    // A request whose registry has drifted is refused before anything is read.
    let mut drifted = cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a'));
    drifted.registry_digest = digest('f');
    drifted.request_digest = drifted.digest();
    assert_eq!(
        LegacyUpcastReport::evaluate_cost_ledger(&drifted, &cost_ledger(0, Some(12), Some(4)))
            .unwrap_err(),
        "legacy_upcast_registry_drift"
    );
}

/// Card rejection 2: an old `cost_micros = 0` is an absent measurement, not a measured zero.
///
/// This is the core of the card. The legacy field is a bare `u64`, so the zero is the same bytes
/// as a free call. The upcast has to turn it into an unknown, and it has to keep it out of every
/// numeric total.
#[test]
fn rejects_a_legacy_zero_cost_being_treated_as_measured() {
    let report = cost_report(0, &digest('a'));
    report
        .validate_against(&cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a')))
        .unwrap();

    // The zero never becomes a number anywhere the report could be summed.
    assert_eq!(report.promoted("cost_micros"), None);
    assert_eq!(report.totals.cost_micros, None);

    let cost = report
        .fields
        .iter()
        .find(|field| field.field == "cost_micros")
        .expect("cost field is reported");
    assert_eq!(cost.class, LegacyFieldClass::Cost);
    assert_eq!(cost.source, LegacyMeasureOrigin::LegacyUnsetZero);
    assert_eq!(
        cost.unknown_reason,
        Some(BillingUnknownReason::ProviderUnreported)
    );
    assert_eq!(cost.reason, "legacy_zero_is_absent_measurement");

    // `Some(0)` on a legacy cost is refused at construction, whatever the declared source is.
    for source in [
        LegacyMeasureOrigin::Reported,
        LegacyMeasureOrigin::Absent,
        LegacyMeasureOrigin::LegacyUnsetZero,
    ] {
        assert_eq!(
            LegacyUpcastField::new(
                "cost_micros",
                LegacyFieldClass::Cost,
                source,
                Some(0),
                None,
                "fixture"
            )
            .unwrap_err(),
            "legacy_upcast_explicit_zero_forbidden"
        );
    }
    // A usage field has the same hole: a legacy zero token count is not a measured zero either.
    let report = LegacyUpcastReport::evaluate_cost_ledger(
        &cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a')),
        &cost_ledger(0, Some(0), Some(4)),
    )
    .unwrap();
    assert_eq!(report.promoted("input_tokens"), None);
    assert_eq!(
        report.reason_for("input_tokens"),
        Some("legacy_usage_zero_is_absent_measurement")
    );
    // And the reported sibling still promotes, so the fix is per-field and not blanket.
    assert_eq!(report.promoted("output_tokens"), Some(4));
}

/// A positive legacy cost is not a measurement either: there is no rate card and no provider
/// receipt behind an old number, so it stays unknown rather than being summed as a cost.
#[test]
fn rejects_a_positive_legacy_cost_measured_without_rate_card_or_receipt() {
    let report = cost_report(9_999, &digest('a'));
    assert_eq!(report.promoted("cost_micros"), None);
    assert_eq!(report.totals.cost_micros, None);
    assert_eq!(
        report.reason_for("cost_micros"),
        Some("legacy_cost_lacks_rate_card_and_receipt")
    );
    let cost = report
        .fields
        .iter()
        .find(|field| field.field == "cost_micros")
        .expect("cost field");
    // A reported legacy number still is not a `Cost` field with a value: the class forbids it.
    assert_eq!(cost.source, LegacyMeasureOrigin::Reported);
    assert_eq!(cost.promoted, None);
    assert_eq!(
        LegacyUpcastField::new(
            "cost_micros",
            LegacyFieldClass::Cost,
            LegacyMeasureOrigin::Reported,
            Some(9_999),
            None,
            "fixture"
        )
        .unwrap_err(),
        "legacy_upcast_measured_cost_forbidden"
    );
    // The totals object itself refuses a measured cost or a measured quota, whatever a caller
    // tries to hand it directly.
    let mut totals = LegacyUpcastTotals::from_fields(&report.fields).unwrap();
    totals.cost_micros = Some(0);
    totals.totals_digest = totals.digest();
    assert_eq!(
        totals.validate_against(&report.fields).unwrap_err(),
        "legacy_upcast_measured_cost_forbidden"
    );
}

/// Card rejection 3: re-running a migration must not overwrite history.
///
/// A replayed report is a no-op, a second different report for the same source is a refusal, and
/// neither of them consumes a new sequence number.
#[test]
fn rejects_repeated_migration_overwriting_recorded_history() {
    let mut history = LegacyUpcastHistory::new();
    let report = cost_report(0, &digest('a'));

    assert_eq!(
        history.append(&report).unwrap(),
        LegacyUpcastOutcome::Appended
    );
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].sequence, 1);

    // Re-running the same migration over the same bytes changes nothing.
    assert_eq!(
        history.append(&report).unwrap(),
        LegacyUpcastOutcome::AlreadyRecorded
    );
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].sequence, 1);

    // A different outcome for the same source is a conflict, not an overwrite. The second run
    // reads the same bytes but translates them differently, which is exactly the case a naive
    // "migrations may be repeated" would silently rewrite.
    let mut conflicting_ledger = cost_ledger(0, Some(12), Some(4));
    conflicting_ledger.input_tokens = Some(99);
    let conflicting = LegacyUpcastReport::evaluate_cost_ledger(
        &cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a')),
        &conflicting_ledger,
    )
    .unwrap();
    assert_ne!(conflicting.report_digest, report.report_digest);
    assert_eq!(
        history.append(&conflicting).unwrap(),
        LegacyUpcastOutcome::Conflict
    );
    assert_eq!(history.records.len(), 1);
    assert_eq!(history.records[0].report_digest, report.report_digest);

    // A second, genuinely different source still appends, and the sequence keeps counting up.
    let other = cost_report(0, &digest('b'));
    assert_eq!(
        history.append(&other).unwrap(),
        LegacyUpcastOutcome::Appended
    );
    assert_eq!(history.records.len(), 2);
    assert_eq!(history.records[1].sequence, 2);
    history.validate().unwrap();
    assert!(history
        .recorded(LegacyPayloadKind::CostLedger, &digest('a'))
        .is_some());
    assert!(history
        .recorded(LegacyPayloadKind::Quota, &digest('a'))
        .is_none());

    // A hand-built history that reuses one source for two entries is refused, so history cannot
    // be rewritten by editing a record.
    let mut forged = LegacyUpcastHistory::new();
    forged
        .records
        .push(LegacyUpcastRecord::new(1, &report).expect("first record"));
    let mut duplicate_source = LegacyUpcastRecord::new(2, &other).expect("second record");
    duplicate_source.source_digest = digest('a');
    duplicate_source.record_digest = duplicate_source.digest();
    forged.records.push(duplicate_source);
    assert_eq!(
        forged.validate().unwrap_err(),
        "legacy_upcast_history_source_duplicate"
    );
}

/// The card's success condition: old data upgrades without lying about what it was.
///
/// Reported usage and limits carry; the cost and the consumption counters do not appear as
/// numbers; the report names its source, destination, version and digest, and says what rolling it
/// back would need without ever performing a write.
#[test]
fn upgrades_old_ledger_quota_and_cassette_without_claiming_more_than_they_said() {
    let request = cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a'));
    let report =
        LegacyUpcastReport::evaluate_cost_ledger(&request, &cost_ledger(0, Some(12), Some(4)))
            .unwrap();
    assert_eq!(report.status, LegacyUpcastStatus::Upcast);
    assert_eq!(report.kind, LegacyPayloadKind::CostLedger);
    assert_eq!(report.source_schema, LEGACY_COST_LEDGER_SCHEMA);
    assert_eq!(report.destination_schema, NORMALIZED_USAGE_SCHEMA);
    assert_eq!(report.reason, "legacy_payload_upcast");
    // Usage carried; the cost did not.
    assert_eq!(report.promoted("input_tokens"), Some(12));
    assert_eq!(report.promoted("output_tokens"), Some(4));
    assert_eq!(report.promoted("cost_micros"), None);
    assert_eq!(report.totals.promoted_fields, 2);
    assert_eq!(report.totals.unknown_fields, 1);
    // Version and digest travel with it.
    assert_eq!(report.version, LEGACY_UPCAST_VERSION);
    assert_eq!(report.report_digest, report.digest());

    // Quota: limits travel, the counters the current budget needs stay absent.
    let quota_request = LegacyUpcastRequest::new(
        LegacyPayloadKind::Quota,
        LEGACY_QUOTA_SCHEMA,
        digest('b'),
        None,
    )
    .unwrap();
    let quota = LegacyQuota {
        scope: "provider:test".to_owned(),
        model_calls: 10,
        tokens: 4_096,
        concurrency: 2,
    };
    let quota_report = LegacyUpcastReport::evaluate_quota(&quota_request, &quota).unwrap();
    assert_eq!(quota_report.destination_schema, QUOTA_WINDOW_BUDGET_SCHEMA);
    assert_eq!(quota_report.promoted("model_calls"), Some(10));
    assert_eq!(quota_report.promoted("tokens"), Some(4_096));
    assert_eq!(quota_report.promoted("concurrency"), Some(2));
    assert_eq!(quota_report.promoted("used_requests"), None);
    assert_eq!(quota_report.promoted("used_tokens"), None);
    assert_eq!(quota_report.promoted("active_concurrency"), None);
    assert_eq!(quota_report.totals.used_units, None);
    assert_eq!(
        quota_report.reason_for("used_tokens"),
        Some("legacy_quota_has_no_consumption_counter")
    );

    // Cassette: token counts fold only when every output reported one, and a partial fold
    // collapses to unknown instead of reporting a smaller total than the file contains.
    let cassette_request = LegacyUpcastRequest::new(
        LegacyPayloadKind::Cassette,
        LEGACY_CASSETTE_SCHEMA,
        digest('c'),
        None,
    )
    .unwrap();
    let complete = LegacyCassette {
        outputs: vec![
            LegacyCassetteOutput {
                text: "first".to_owned(),
                model_id: Some("model:test".to_owned()),
                stop_reason: None,
                usage: Some(LegacyCassetteUsage {
                    input_tokens: Some(10),
                    output_tokens: Some(2),
                }),
            },
            LegacyCassetteOutput {
                text: "second".to_owned(),
                model_id: None,
                stop_reason: None,
                usage: Some(LegacyCassetteUsage {
                    input_tokens: Some(5),
                    output_tokens: Some(1),
                }),
            },
        ],
    };
    let complete_report =
        LegacyUpcastReport::evaluate_cassette(&cassette_request, &complete).unwrap();
    assert_eq!(complete_report.destination_schema, PROVIDER_CASSETTE_SCHEMA);
    assert_eq!(complete_report.promoted("input_tokens"), Some(15));
    assert_eq!(complete_report.promoted("output_tokens"), Some(3));
    assert_eq!(complete_report.promoted("cost_micros"), None);

    let mut partial = complete.clone();
    partial.outputs[1].usage = None;
    let partial_report =
        LegacyUpcastReport::evaluate_cassette(&cassette_request, &partial).unwrap();
    assert_eq!(partial_report.promoted("input_tokens"), None);
    assert_eq!(
        partial_report.reason_for("input_tokens"),
        Some("legacy_usage_partial_report")
    );

    // Config: no number is invented. The profile version the old file never carried is recorded
    // as absent, and the selection mode must actually be one this build knows.
    let config_request = LegacyUpcastRequest::new(
        LegacyPayloadKind::ProviderConfig,
        LEGACY_PROVIDER_CONFIG_SCHEMA,
        digest('d'),
        None,
    )
    .unwrap();
    let config = LegacyProviderConfig {
        selection_mode: "cassette".to_owned(),
        profiles: vec![LegacyConfigProfile {
            profile: "default".to_owned(),
            provider_id: "provider:test".to_owned(),
            protocol: "anthropic_messages".to_owned(),
            credential_ref: Some("sha256-key-reference".to_owned()),
        }],
    };
    let config_report = LegacyUpcastReport::evaluate_provider_config(&config_request, &config)
        .expect("legacy config upcasts");
    assert_eq!(
        config.selection_mode().unwrap(),
        ProviderSelectionMode::Cassette
    );
    assert_eq!(config.config_source(), ProviderConfigSource::LegacyCassette);
    assert_eq!(
        config_report.destination_schema,
        PROVIDER_CONFIG_SNAPSHOT_SCHEMA
    );
    assert_eq!(config_report.promoted("profile_version"), None);
    assert_eq!(
        config_report.reason_for("profile_version"),
        Some("legacy_provider_version_assigned_at_upcast")
    );
    assert_eq!(config_report.totals.promoted_fields, 0);
    assert_eq!(config_report.totals.unknown_fields, 1);

    let mut bad_mode = config.clone();
    bad_mode.selection_mode = "auto".to_owned();
    assert_eq!(
        LegacyUpcastReport::evaluate_provider_config(&config_request, &bad_mode).unwrap_err(),
        "legacy_upcast_selection_mode_unknown"
    );
    let mut bad_protocol = config;
    bad_protocol.profiles[0].protocol = "carrier_pigeon".to_owned();
    assert_eq!(
        LegacyUpcastReport::evaluate_provider_config(&config_request, &bad_protocol).unwrap_err(),
        "legacy_upcast_protocol_unknown"
    );
}

/// Re-running a migration over already-upcast material takes the idempotent path: it re-reads
/// nothing, invents no field, and still produces a valid, digest-bound report.
#[test]
fn an_already_current_payload_is_idempotent_and_invents_nothing() {
    let request = LegacyUpcastRequest::new(
        LegacyPayloadKind::CostLedger,
        NORMALIZED_USAGE_SCHEMA,
        digest('a'),
        None,
    )
    .unwrap();
    let report = LegacyUpcastReport::already_current(&request).unwrap();
    assert_eq!(report.status, LegacyUpcastStatus::AlreadyCurrent);
    assert_eq!(report.reason, "legacy_payload_already_current");
    assert!(report.fields.is_empty());
    assert_eq!(report.totals.promoted_fields, 0);
    assert_eq!(report.totals.unknown_fields, 0);
    report.validate_against(&request).unwrap();

    // A hand-made report that claims to be already current while carrying a field is refused.
    let mut forged = report.clone();
    forged.fields.push(
        LegacyUpcastField::new(
            "input_tokens",
            LegacyFieldClass::Usage,
            LegacyMeasureOrigin::Reported,
            Some(1),
            None,
            "legacy_usage_reported",
        )
        .unwrap(),
    );
    forged.totals = LegacyUpcastTotals::from_fields(&forged.fields).unwrap();
    forged.report_digest = forged.digest();
    assert_eq!(
        forged.validate_against(&request).unwrap_err(),
        "legacy_upcast_already_current_carries_fields"
    );

    // And the upcaster refuses to run the upcasting path over already-current material.
    let legacy_request = cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a'));
    assert_eq!(
        LegacyUpcastReport::evaluate_cost_ledger(
            &LegacyUpcastRequest::new(
                LegacyPayloadKind::CostLedger,
                NORMALIZED_USAGE_SCHEMA,
                digest('a'),
                None,
            )
            .unwrap(),
            &cost_ledger(0, Some(12), Some(4))
        )
        .unwrap_err(),
        "legacy_upcast_already_current_use_idempotent_path"
    );
    assert!(legacy_request.validate().is_ok());
}

/// The rollback and read-only statement the card asks for travels with every report, and a
/// report claiming it wrote something is rejected.
#[test]
fn every_report_is_read_only_and_names_its_rollback_inputs() {
    let request = cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a'));
    let report =
        LegacyUpcastReport::evaluate_cost_ledger(&request, &cost_ledger(0, Some(12), Some(4)))
            .unwrap();
    assert!(report.rollback.read_only);
    assert_eq!(report.rollback.legacy_source_digest, digest('a'));
    assert_eq!(
        report.rollback.verified_backup_ref.as_deref(),
        Some("backup:bq27-fixture")
    );
    report.rollback.validate().unwrap();

    // A report edited to claim it wrote back is refused, both by its own rollback record and by
    // the re-derivation in `validate_against`.
    let mut edited = report.clone();
    edited.rollback.read_only = false;
    edited.rollback.rollback_digest = edited.rollback.digest();
    edited.report_digest = edited.digest();
    assert_eq!(
        edited.validate_against(&request).unwrap_err(),
        "legacy_upcast_read_only_forbidden"
    );
    assert_eq!(
        edited.rollback.validate().unwrap_err(),
        "legacy_upcast_read_only_forbidden"
    );
}

/// An unknown field in a legacy payload is a typed refusal, not a dropped field. A dropped field
/// is a quiet downgrade of what the file said, and this module must not invent a translation.
#[test]
fn rejects_an_unknown_legacy_field_rather_than_dropping_it() {
    let payload = json!({
        "schema": LEGACY_COST_LEDGER_SCHEMA,
        "records": [],
        "input_tokens": Some(4),
        "output_tokens": serde_json::Value::Null,
        "cost_micros": 0,
        "settlement_state": "settled"
    });
    assert_eq!(
        parse_legacy::<LegacyCostLedger>(&payload).unwrap_err(),
        "legacy_upcast_non_migratable_field"
    );
    // A malformed field type is a different, equally typed refusal.
    let malformed = json!({
        "schema": LEGACY_COST_LEDGER_SCHEMA,
        "records": [],
        "input_tokens": "four",
        "output_tokens": null,
        "cost_micros": 0
    });
    assert_eq!(
        parse_legacy::<LegacyCostLedger>(&malformed).unwrap_err(),
        "legacy_upcast_payload_malformed"
    );
}

/// A tampered report is rejected on re-validation: the whole decision is re-derived from the
/// request, so an edited status, reason, promoted value or field list cannot pass.
#[test]
fn a_tampered_report_is_rejected_by_re_derivation() {
    let request = cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a'));
    let report =
        LegacyUpcastReport::evaluate_cost_ledger(&request, &cost_ledger(0, Some(12), Some(4)))
            .unwrap();

    // The reason is edited and the report digest is left stale, so both the re-derivation and
    // the digest would refuse it.
    let mut broken = report.clone();
    broken.reason = "edited".to_owned();
    assert_eq!(
        broken.validate_against(&request).unwrap_err(),
        "legacy_upcast_report_binding_invalid"
    );
    assert_eq!(
        broken.report_digest, report.report_digest,
        "the fixture must really have left the digest stale"
    );

    // Digest recomputed after the edit, so only the re-derivation can catch it.
    let mut retargeted = report.clone();
    retargeted.reason = "edited".to_owned();
    retargeted.report_digest = retargeted.digest();
    assert_eq!(
        retargeted.validate_against(&request).unwrap_err(),
        "legacy_upcast_report_binding_invalid"
    );

    // A promoted value edited onto a field the upcast refused.
    let mut inflated = report.clone();
    inflated.fields[0].promoted = Some(0);
    inflated.report_digest = inflated.digest();
    assert_eq!(
        inflated.validate_against(&request).unwrap_err(),
        "legacy_upcast_explicit_zero_forbidden"
    );

    // Totals edited away from the field list.
    let mut miscounted = report;
    miscounted.totals.promoted_fields = 3;
    miscounted.totals.totals_digest = miscounted.totals.digest();
    miscounted.report_digest = miscounted.digest();
    assert_eq!(
        miscounted.validate_against(&request).unwrap_err(),
        "legacy_upcast_field_totals_mismatch"
    );
}

/// Text that came out of a legacy file is bounded, redacted and sentinel-scanned before it can be
/// carried in a report, and the request itself is bound by a digest.
#[test]
fn legacy_text_is_bounded_redacted_and_secret_scanned() {
    let quota_request = LegacyUpcastRequest::new(
        LegacyPayloadKind::Quota,
        LEGACY_QUOTA_SCHEMA,
        digest('b'),
        None,
    )
    .unwrap();
    let leaky = LegacyQuota {
        scope: "access_token=BQ27_SENTINEL".to_owned(),
        model_calls: 1,
        tokens: 1,
        concurrency: 1,
    };
    assert_eq!(
        LegacyUpcastReport::evaluate_quota(&quota_request, &leaky).unwrap_err(),
        "legacy_upcast_quota_scope_not_redacted"
    );
    // A value redact_text leaves alone but the sentinel scanner still rejects: the two checks
    // are separate, and the scanner is the second line of defence.
    let scanned = LegacyQuota {
        scope: "Bearer BQ27_SENTINEL".to_owned(),
        model_calls: 1,
        tokens: 1,
        concurrency: 1,
    };
    assert_eq!(
        LegacyUpcastReport::evaluate_quota(&quota_request, &scanned).unwrap_err(),
        "legacy_upcast_quota_scope_not_redacted"
    );
    let long = LegacyQuota {
        scope: "s".repeat(MAX_LEGACY_TEXT + 1),
        model_calls: 1,
        tokens: 1,
        concurrency: 1,
    };
    assert_eq!(
        LegacyUpcastReport::evaluate_quota(&quota_request, &long).unwrap_err(),
        "legacy_upcast_quota_scope_invalid"
    );

    // A cassette body is model-visible text, so it is scanned as a transcript.
    let cassette_request = LegacyUpcastRequest::new(
        LegacyPayloadKind::Cassette,
        LEGACY_CASSETTE_SCHEMA,
        digest('c'),
        None,
    )
    .unwrap();
    let leaky_cassette = LegacyCassette {
        outputs: vec![LegacyCassetteOutput {
            text: "Authorization: Bearer BQ27_CASSETTE".to_owned(),
            model_id: None,
            stop_reason: None,
            usage: None,
        }],
    };
    assert_eq!(
        LegacyUpcastReport::evaluate_cassette(&cassette_request, &leaky_cassette).unwrap_err(),
        "legacy_upcast_cassette_text_secret_detected"
    );
    let empty_cassette = LegacyCassette {
        outputs: Vec::new(),
    };
    assert_eq!(
        LegacyUpcastReport::evaluate_cassette(&cassette_request, &empty_cassette).unwrap_err(),
        "legacy_upcast_cassette_outputs_invalid"
    );

    // A request whose digest does not cover its own fields is refused.
    let mut unbound = cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a'));
    unbound.payload_digest = digest('f');
    assert_eq!(
        unbound.validate().unwrap_err(),
        "legacy_upcast_request_digest_mismatch"
    );
    let malformed_digest = LegacyUpcastRequest::new(
        LegacyPayloadKind::CostLedger,
        LEGACY_COST_LEDGER_SCHEMA,
        "not-a-digest",
        None,
    );
    assert_eq!(
        malformed_digest.unwrap_err(),
        "legacy_upcast_payload_digest_invalid"
    );
}

/// The classifier that the whole card turns on: a legacy `0` is never reported, and a missing key
/// and an explicit `null` are the same absence. This is the rule a future upcaster for another
/// legacy family has to reuse rather than restate.
#[test]
fn the_measure_origin_classifier_never_calls_a_zero_reported() {
    assert_eq!(
        LegacyMeasureOrigin::of(None),
        LegacyMeasureOrigin::Absent,
        "a missing key is an absent measurement"
    );
    assert_eq!(
        LegacyMeasureOrigin::of(Some(&Value::Null)),
        LegacyMeasureOrigin::Absent,
        "an explicit null is the same absence as a missing key"
    );
    assert_eq!(
        LegacyMeasureOrigin::of(Some(&Value::from(0))),
        LegacyMeasureOrigin::LegacyUnsetZero
    );
    assert_eq!(
        LegacyMeasureOrigin::of(Some(&Value::from(4))),
        LegacyMeasureOrigin::Reported
    );
    assert_eq!(
        LegacyMeasureOrigin::of(Some(&Value::from(9_999))),
        LegacyMeasureOrigin::Reported
    );
    // The wire names are the contract an adapter would read.
    assert_eq!(LegacyMeasureOrigin::Reported.as_str(), "reported");
    assert_eq!(LegacyMeasureOrigin::Absent.as_str(), "absent");
    assert_eq!(
        LegacyMeasureOrigin::LegacyUnsetZero.as_str(),
        "legacy_unset_zero"
    );
    assert_eq!(LegacyFieldClass::Limit.as_str(), "limit");
    assert_eq!(LegacyFieldClass::Usage.as_str(), "usage");
    assert_eq!(LegacyFieldClass::Cost.as_str(), "cost");
    assert_eq!(LegacyPayloadKind::CostLedger.as_str(), "cost_ledger");
    assert_eq!(LegacyPayloadKind::Quota.as_str(), "quota");
    assert_eq!(LegacyPayloadKind::Cassette.as_str(), "cassette");
    assert_eq!(
        LegacyPayloadKind::ProviderConfig.as_str(),
        "provider_config"
    );
    assert_eq!(LegacyUpcastStatus::Upcast.as_str(), "upcast");
    assert_eq!(
        LegacyUpcastStatus::AlreadyCurrent.as_str(),
        "already_current"
    );
}

/// The registry names every upcaster, its source and its destination, and it is digest-bound so a
/// report cannot be produced against a different upcaster set.
#[test]
fn the_upcast_registry_is_named_versioned_and_digest_bound() {
    assert_eq!(LEGACY_UPCAST_SOURCES.len(), LegacyPayloadKind::ALL.len());
    for kind in LegacyPayloadKind::ALL {
        let source = legacy_upcast_source_schema(kind);
        let destination = legacy_upcast_destination_schema(kind);
        assert!(
            source.ends_with(".v0"),
            "{:?} legacy source must be a v0",
            kind
        );
        assert!(
            !destination.ends_with(".v0"),
            "{:?} destination must be current",
            kind
        );
        assert_eq!(
            classify_legacy_schema(source).unwrap(),
            LegacySchemaClass::LegacySource(kind)
        );
        assert_eq!(
            classify_legacy_schema(destination).unwrap(),
            LegacySchemaClass::CurrentDestination(kind)
        );
    }
    let registry = legacy_upcast_registry_digest();
    assert!(registry.starts_with("sha256:"));
    assert_eq!(registry, legacy_upcast_registry_digest());
    assert_eq!(
        cost_request(LEGACY_COST_LEDGER_SCHEMA, &digest('a')).registry_digest,
        legacy_upcast_registry_digest()
    );
}
