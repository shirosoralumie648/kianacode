//! Core adapter for unified Company Human Inbox cards.
//!
//! Cards are read/decision projections; consuming one only records the bound decision. The
//! ControlPlane must reauthorize and execute the referenced command separately.

use kiana_domain::{CompanyInboxCard, CompanyInboxLedger};

pub(crate) fn publish_company_inbox_card(
    ledger: &mut CompanyInboxLedger,
    card: CompanyInboxCard,
) -> Result<(), &'static str> {
    ledger.publish(card)
}

pub(crate) fn consume_company_inbox_card(
    ledger: &mut CompanyInboxLedger,
    card_id: &str,
    decider_ref: &str,
    option: &str,
    decision_ref: &str,
    now: u64,
    target_revision: u64,
    target_digest: &str,
    scope_digest: &str,
) -> Result<(), &'static str> {
    ledger.consume(
        card_id,
        decider_ref,
        option,
        decision_ref,
        now,
        target_revision,
        target_digest,
        scope_digest,
    )
}
