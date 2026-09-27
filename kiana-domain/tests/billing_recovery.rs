use kiana_domain::{BillingRecoveryFact, BillingRecoveryState, BILLING_RECOVERY_SCHEMA};
use serde_json::json;

fn digest(fact: &BillingRecoveryFact) -> String {
    kiana_domain::json_digest(&json!({
        "schema": fact.schema,
        "attempt_id": fact.attempt_id,
        "reservation_digest": fact.reservation_digest,
        "source_cursor": fact.source_cursor,
        "authority_epoch": fact.authority_epoch,
        "lease_epoch": fact.lease_epoch,
        "state": fact.state,
        "usage_unknown": fact.usage_unknown,
        "continue_resets_usage": fact.continue_resets_usage,
        "settlement_count": fact.settlement_count,
        "reconciliation_ref": fact.reconciliation_ref,
    }))
}

fn fact(state: BillingRecoveryState) -> BillingRecoveryFact {
    let mut fact = BillingRecoveryFact {
        schema: BILLING_RECOVERY_SCHEMA.to_owned(),
        attempt_id: "attempt:billing-1".to_owned(),
        reservation_digest:
            "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        source_cursor: 42,
        authority_epoch: 7,
        lease_epoch: 3,
        state,
        usage_unknown: matches!(state, BillingRecoveryState::NeedsReconciliation),
        continue_resets_usage: false,
        settlement_count: u8::from(matches!(state, BillingRecoveryState::Recovered)),
        reconciliation_ref: match state {
            BillingRecoveryState::NeedsReconciliation | BillingRecoveryState::Recovered => {
                Some("reconcile:billing-1".to_owned())
            }
            BillingRecoveryState::Paused => None,
        },
        recovery_digest: String::new(),
    };
    fact.recovery_digest = digest(&fact);
    fact
}

#[test]
fn recovery_states_preserve_unknown_until_explicit_reconciliation() {
    assert!(fact(BillingRecoveryState::Paused).validate().is_ok());
    assert!(fact(BillingRecoveryState::NeedsReconciliation)
        .validate()
        .is_ok());
    assert!(fact(BillingRecoveryState::Recovered).validate().is_ok());
}

#[test]
fn continue_never_resets_usage_and_unknown_cannot_be_success() {
    let mut invalid = fact(BillingRecoveryState::NeedsReconciliation);
    invalid.continue_resets_usage = true;
    assert_eq!(invalid.validate(), Err("billing_recovery_fact_invalid"));

    let mut invalid = fact(BillingRecoveryState::Recovered);
    invalid.usage_unknown = true;
    invalid.recovery_digest = digest(&invalid);
    assert_eq!(invalid.validate(), Err("billing_recovery_fact_invalid"));
}

#[test]
fn old_epoch_or_digest_tampering_fails_closed_and_unknown_fields_are_denied() {
    let mut invalid = fact(BillingRecoveryState::NeedsReconciliation);
    invalid.lease_epoch = 0;
    assert_eq!(invalid.validate(), Err("billing_recovery_fact_invalid"));

    let mut value = serde_json::to_value(fact(BillingRecoveryState::Paused)).unwrap();
    value["unexpected"] = json!(true);
    assert!(serde_json::from_value::<BillingRecoveryFact>(value).is_err());

    let mut repeated = fact(BillingRecoveryState::Recovered);
    repeated.settlement_count = 2;
    repeated.recovery_digest = digest(&repeated);
    assert_eq!(repeated.validate(), Err("billing_recovery_fact_invalid"));
}
