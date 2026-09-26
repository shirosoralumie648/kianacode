//! Core adapter for manifest-bound delivery authorization and confirmation.
//!
//! Only the existing Broker/effect path can produce a delivery receipt. This adapter does not
//! write a package, contact a recipient or reinterpret an Unknown outcome.

use kiana_domain::{
    DeliveryAuthorization, DeliveryAuthorizationLedger, DeliveryDispatchIntent,
    DeliveryEffectReceipt, DeliveryManifest, DeliveryRecipientConfirmation, LocalDeliveryPackage,
};

pub(crate) fn authorize_delivery(
    ledger: &mut DeliveryAuthorizationLedger,
    authorization: DeliveryAuthorization,
    manifest: &DeliveryManifest,
    now: u64,
) -> Result<(), &'static str> {
    ledger.authorize(authorization, manifest, now)
}

pub(crate) fn request_delivery_dispatch(
    ledger: &mut DeliveryAuthorizationLedger,
    intent: DeliveryDispatchIntent,
    manifest: &DeliveryManifest,
    now: u64,
) -> Result<(), &'static str> {
    ledger.request_dispatch(intent, manifest, now)
}

pub(crate) fn record_delivery_effect(
    ledger: &mut DeliveryAuthorizationLedger,
    receipt: DeliveryEffectReceipt,
    manifest: &DeliveryManifest,
    package: &LocalDeliveryPackage,
) -> Result<(), &'static str> {
    ledger.record_effect(receipt, manifest, package)
}

pub(crate) fn confirm_delivery_recipient(
    ledger: &mut DeliveryAuthorizationLedger,
    confirmation: DeliveryRecipientConfirmation,
) -> Result<(), &'static str> {
    ledger.confirm_recipient(confirmation)
}
