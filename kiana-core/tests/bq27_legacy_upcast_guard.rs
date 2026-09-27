//! BQ-27 source guard: an upcast never invents a measurement, and this module never performs a
//! migration.
//!
//! Every marker below is grep-verified against the real file, and every forbidden string is
//! grep-verified absent. Read the source before changing either list.

#[test]
fn bq27_unknown_major_is_a_typed_refusal_and_never_a_best_effort_parse() {
    let domain = include_str!("../../kiana-domain/src/legacy_upcast.rs");
    for marker in [
        "LEGACY_UPCAST_SOURCES",
        "classify_legacy_schema",
        "legacy_upcast_unknown_major",
        "legacy_upcast_registry_drift",
        "parse_legacy",
        "legacy_upcast_non_migratable_field",
        "legacy_upcast_payload_malformed",
        "LegacySchemaClass",
        "LegacyUpcastSource",
        "legacy_upcast_registry_digest",
    ] {
        assert!(domain.contains(marker), "BQ-27 marker missing: {marker}");
    }
    // The refusal is the only thing `classify_legacy_schema` can return besides a class: there is
    // no `Ok` path that falls through to a parse of something unrecognised. It is emitted twice
    // on purpose, once by the classifier and once by the admission order, and both sites must
    // exist: dropping either one would leave a route where an unrecognised name is parsed.
    assert_eq!(
        domain
            .matches("Err(\"legacy_upcast_unknown_major\".to_owned())")
            .count(),
        2,
        "BQ-27 must refuse an unknown major from the classifier and from admission"
    );
    assert!(
        domain.contains("pub fn classify_legacy_schema(payload_schema: &str) -> Result<LegacySchemaClass, String> {"),
        "BQ-27 must classify the declared schema before anything is parsed"
    );
    assert!(
        domain.contains(
            "fn admit(request: &LegacyUpcastRequest) -> Result<LegacyAdmission, String> {"
        ),
        "BQ-27 must re-check the schema in the admission order"
    );
    // The decision order is fixed, so the reported reason is deterministic: request shape, then
    // registry drift, then already-current, then the named upcaster.
    let rules = [
        "request.validate()?;",
        "legacy_upcast_registry_drift",
        "legacy_payload_already_current",
        "legacy_upcast_unknown_major",
    ];
    let mut at = 0;
    for rule in rules {
        let found = domain[at..]
            .find(rule)
            .unwrap_or_else(|| panic!("BQ-27 decision order must reach {rule}"));
        at += found + rule.len();
    }
}

#[test]
fn bq27_a_legacy_zero_cost_can_never_become_a_measured_zero() {
    let domain = include_str!("../../kiana-domain/src/legacy_upcast.rs");
    for marker in [
        "LegacyMeasureOrigin",
        "LegacyUnsetZero",
        "LegacyFieldClass",
        "legacy_upcast_explicit_zero_forbidden",
        "legacy_upcast_measured_cost_forbidden",
        "legacy_upcast_measured_usage_forbidden",
        "legacy_zero_is_absent_measurement",
        "legacy_usage_zero_is_absent_measurement",
        "legacy_cost_lacks_rate_card_and_receipt",
        "legacy_upcast_field_totals_mismatch",
        "legacy_upcast_totals_digest",
    ] {
        assert!(domain.contains(marker), "BQ-27 marker missing: {marker}");
    }
    // A promoted zero is refused no matter which class the field belongs to. Assert it against
    // the exact line, because a file-wide search would also match the `cost_micros.is_some()`
    // checks and would pass even if this rule were removed.
    assert!(
        domain.contains("if self.promoted == Some(0) {"),
        "BQ-27 must refuse an explicitly promoted zero"
    );
    // The class rule is separate from the zero rule, and it is what stops a *positive* legacy
    // cost from being reported as measured.
    assert!(
        domain.contains("if self.class == LegacyFieldClass::Cost"),
        "BQ-27 must refuse any promotable legacy cost, not only a zero one"
    );
    // The two totals that could be mistaken for measurements are structurally absent and are
    // re-asserted in `validate_against`, not merely initialised to `None`.
    assert!(
        domain.contains("if self.cost_micros.is_some() {"),
        "BQ-27 must re-assert that no measured cost leaves an upcast"
    );
    assert!(
        domain.contains("if self.used_units.is_some() {"),
        "BQ-27 must re-assert that no consumed quota leaves an upcast"
    );
    // `Some(0)` is never classified as a reported measurement. This is the classification the
    // whole card turns on, so it is asserted on the classifier's own arm.
    assert!(
        domain.contains(
            "Some(Value::Number(number)) if number.as_u64() == Some(0) => Self::LegacyUnsetZero,"
        ),
        "BQ-27 must classify a legacy zero as an unset zero, never as reported"
    );
    // And the cost field builder never hands back a value, on either legacy path.
    let costs = domain
        .find("fn cost_field(")
        .expect("BQ-27 must classify legacy cost through one place");
    let costs = &domain[costs
        ..domain[costs..]
            .find("\n}\n")
            .map(|end| costs + end)
            .unwrap()];
    assert!(
        costs.matches("None,").count() >= 2,
        "BQ-27 must return no promoted value on both legacy cost paths"
    );
    assert!(
        !costs.contains("Some(reported)"),
        "BQ-27 must never promote a legacy cost"
    );
}

#[test]
fn bq27_re_running_a_migration_is_idempotent_rather_than_an_overwrite() {
    let domain = include_str!("../../kiana-domain/src/legacy_upcast.rs");
    for marker in [
        "LegacyUpcastHistory",
        "LegacyUpcastOutcome",
        "AlreadyRecorded",
        "Conflict",
        "legacy_upcast_history_source_duplicate",
        "legacy_upcast_history_sequence_invalid",
        "legacy_upcast_history_exceeded",
        "already_current",
        "legacy_upcast_already_current_carries_fields",
        "legacy_upcast_already_current_use_idempotent_path",
    ] {
        assert!(domain.contains(marker), "BQ-27 marker missing: {marker}");
    }
    // A replay is a no-op and a divergent report is a conflict, not a rewrite. Both arms return
    // before anything is pushed, so assert the early return exists at all.
    assert!(
        domain.contains("return if existing.report_digest == report.report_digest {"),
        "BQ-27 must treat a replayed report as already recorded"
    );
    assert!(
        domain.contains("Ok(LegacyUpcastOutcome::Conflict)"),
        "BQ-27 must refuse a second different report for the same source"
    );
    // Nothing is ever removed from the history: an append-only fold has no removal path.
    assert!(
        !domain.contains("records.remove("),
        "BQ-27 history must be append-only"
    );
    assert!(
        !domain.contains("records.clear("),
        "BQ-27 history must not be resettable by a migration"
    );
    // The already-current path must not read or re-translate the legacy bytes.
    assert!(
        domain.contains("pub fn already_current(request: &LegacyUpcastRequest)"),
        "BQ-27 must expose the idempotent path for already-upcast material"
    );
    assert!(
        domain.contains("Self::assemble(request, admission, Vec::new())"),
        "BQ-27 must invent no field when there is nothing left to upcast"
    );
}

#[test]
fn bq27_migration_is_reported_not_performed() {
    let domain = include_str!("../../kiana-domain/src/legacy_upcast.rs");
    // This module decides what an upcast means. It must not read, write, lock, migrate or append.
    for forbidden in [
        "std::fs",
        "std::process",
        "File::",
        "tokio::",
        "EventStorePort",
        "ArtifactStorePort",
        "MigrationRecordPort",
        "write_all",
        "sync_all",
        "remove_file",
        "remove_dir",
        "rename(",
        "fn apply",
        "fn run",
    ] {
        assert!(
            !domain.contains(forbidden),
            "BQ-27 upcast crossed the effect boundary: {forbidden}"
        );
    }
    // The report is read-only by construction, and a report claiming otherwise is rejected.
    assert!(
        domain.contains("pub read_only: bool,"),
        "BQ-27 rollback must state that it is read-only"
    );
    assert!(
        domain.contains("if !self.read_only {"),
        "BQ-27 must reject a report claiming it wrote anything back"
    );
    // The bytes travel as a digest, never as a payload body.
    assert!(
        domain.contains("pub payload_digest: String,"),
        "BQ-27 must carry a digest of the legacy bytes"
    );
    for forbidden in [
        "pub payload: String",
        "pub bytes: Vec<u8>",
        "pub raw: String",
    ] {
        assert!(
            !domain.contains(forbidden),
            "BQ-27 must never carry the legacy payload body: {forbidden}"
        );
    }
    // Rollback and read-only are named, which is the card's success text, and they are digest
    // bound like everything else in the module.
    assert!(
        domain.contains("pub verified_backup_ref: Option<String>,"),
        "BQ-27 must name the backup a caller would need before writing back"
    );
    assert!(
        domain.contains("pub legacy_source_digest: String,"),
        "BQ-27 rollback must name the source to re-read"
    );
}

#[test]
fn bq27_reuses_the_existing_vocabulary_instead_of_a_second_one() {
    let domain = include_str!("../../kiana-domain/src/legacy_upcast.rs");
    let storage = include_str!("../../kiana-domain/src/storage_schema.rs");
    // The unknown-major rule is the storage upcaster's rule, restated for a different family, not
    // a new compatibility policy. Both use the same stable code and the same majors-only split.
    assert!(
        storage.contains("storage_schema_unknown_major"),
        "BQ-27 must sit alongside the existing storage upcaster rule"
    );
    assert!(
        domain.contains("legacy_upcast_unknown_major"),
        "BQ-27 must name the refusal the same way for its own family"
    );
    // Cost and quota reuse the billing contracts rather than re-deriving them.
    for reused in [
        "BillingUnknownReason",
        "crate::NORMALIZED_USAGE_SCHEMA",
        "crate::QUOTA_WINDOW_BUDGET_SCHEMA",
        "crate::PROVIDER_CASSETTE_SCHEMA",
        "crate::PROVIDER_CONFIG_SNAPSHOT_SCHEMA",
        "ProviderConfigSource::LegacyCassette",
        "ProviderSelectionMode",
    ] {
        assert!(
            domain.contains(reused),
            "BQ-27 must reuse the existing contract, not a second one: {reused}"
        );
    }
    // The upcast destinations are the current schemas, so a legacy name is never a live one.
    assert!(
        domain.contains("crate::MigrationRegistry"),
        "BQ-27 must not compete with the migration registry for the same step"
    );
    // Protocol parsing is derived from the current enum's own wire names rather than a second
    // list that could drift from it.
    assert!(
        domain.contains("pub fn wire_name(self) -> &'static str {"),
        "BQ-27 must derive protocol spellings from the current enum"
    );
    assert!(
        !domain.contains("fn parse_legacy(value: &str) -> Option<ModelProtocol>"),
        "BQ-27 must not define a second protocol vocabulary"
    );
}
