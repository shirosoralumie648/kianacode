//! BQ-21 read-only Core facade for billing restart/recovery facts.

use kiana_domain::{
    validate_billing_recovery as validate_domain_billing_recovery, BillingRecoveryFact,
};

pub fn validate_billing_recovery(fact: &BillingRecoveryFact) -> Result<(), &'static str> {
    validate_domain_billing_recovery(fact)
}
