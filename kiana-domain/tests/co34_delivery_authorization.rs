use kiana_domain::*;

fn sha(seed: char) -> String {
    format!("sha256:{}", seed.to_string().repeat(64))
}

fn manifest() -> DeliveryManifest {
    let mut value = DeliveryManifest {
        schema: DELIVERY_MANIFEST_SCHEMA.to_owned(),
        manifest_id: "manifest-1".to_owned(),
        delivery_id: "delivery-1".to_owned(),
        project_id: "project-1".to_owned(),
        baseline_version: 4,
        acceptance_id: "acceptance-1".to_owned(),
        acceptance_status: AcceptanceStatus::Accepted,
        acceptance_digest: sha('a'),
        channel: "local_package".to_owned(),
        destination: "lessons/deliveries/delivery-1.json".to_owned(),
        recipient_ref: "principal:recipient-1".to_owned(),
        residual_obligations: Vec::new(),
        artifacts: vec![DeliveryManifestArtifact {
            artifact_ref: "artifact:report".to_owned(),
            project_id: "project-1".to_owned(),
            packet_id: Some("packet-1".to_owned()),
            artifact_version: 2,
            relative_path: "report/report.md".to_owned(),
            content_hash: sha('c'),
            content_size: 42,
            producer_run_refs: vec!["run:1".to_owned()],
        }],
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn package(manifest: &DeliveryManifest) -> LocalDeliveryPackage {
    let mut value = LocalDeliveryPackage {
        schema: LOCAL_DELIVERY_PACKAGE_SCHEMA.to_owned(),
        package_id: "package-1".to_owned(),
        manifest_id: manifest.manifest_id.clone(),
        manifest_digest: manifest.digest.clone(),
        package_root: "lessons/packages/package-1".to_owned(),
        entries: vec![LocalPackageEntry {
            artifact_ref: "artifact:report".to_owned(),
            relative_path: "report/report.md".to_owned(),
            content_hash: sha('c'),
            content_size: 42,
            symlink: false,
        }],
        package_digest: String::new(),
    };
    value.package_digest = value.canonical_digest();
    value
}

fn authorization(manifest: &DeliveryManifest) -> DeliveryAuthorization {
    let mut value = DeliveryAuthorization {
        schema: DELIVERY_AUTHORIZATION_SCHEMA.to_owned(),
        authorization_id: "authorization-1".to_owned(),
        delivery_id: manifest.delivery_id.clone(),
        manifest_id: manifest.manifest_id.clone(),
        manifest_digest: manifest.digest.clone(),
        project_id: manifest.project_id.clone(),
        baseline_version: manifest.baseline_version,
        destination: manifest.destination.clone(),
        recipient_ref: manifest.recipient_ref.clone(),
        approved_by: "sponsor-1".to_owned(),
        approved_at: 10,
        expires_at: 100,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn intent(manifest: &DeliveryManifest) -> DeliveryDispatchIntent {
    let mut value = DeliveryDispatchIntent {
        schema: DELIVERY_DISPATCH_SCHEMA.to_owned(),
        intent_id: "intent-1".to_owned(),
        authorization_id: "authorization-1".to_owned(),
        delivery_id: manifest.delivery_id.clone(),
        manifest_digest: manifest.digest.clone(),
        dispatch_request_id: "request-1".to_owned(),
        requested_by: "sender-1".to_owned(),
        requested_at: 20,
        state: DeliveryDispatchState::Requested,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn receipt(
    manifest: &DeliveryManifest,
    package: &LocalDeliveryPackage,
    intent: &DeliveryDispatchIntent,
    source: DeliveryReceiptSource,
    outcome: DeliveryEffectOutcome,
) -> DeliveryEffectReceipt {
    let mut value = DeliveryEffectReceipt {
        schema: DELIVERY_EFFECT_RECEIPT_SCHEMA.to_owned(),
        receipt_id: "receipt-1".to_owned(),
        intent_id: intent.intent_id.clone(),
        delivery_id: manifest.delivery_id.clone(),
        manifest_digest: manifest.digest.clone(),
        package_id: package.package_id.clone(),
        package_digest: package.package_digest.clone(),
        destination: manifest.destination.clone(),
        recipient_ref: manifest.recipient_ref.clone(),
        source,
        outcome,
        evidence_refs: vec!["event:broker-delivery-1".to_owned()],
        observed_at: 30,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

fn confirmation(receipt: &DeliveryEffectReceipt) -> DeliveryRecipientConfirmation {
    let mut value = DeliveryRecipientConfirmation {
        schema: DELIVERY_CONFIRMATION_SCHEMA.to_owned(),
        confirmation_id: "confirmation-1".to_owned(),
        receipt_id: receipt.receipt_id.clone(),
        delivery_id: receipt.delivery_id.clone(),
        manifest_digest: receipt.manifest_digest.clone(),
        package_digest: receipt.package_digest.clone(),
        recipient_ref: receipt.recipient_ref.clone(),
        confirmed_by: receipt.recipient_ref.clone(),
        confirmed_at: 40,
        human_confirmed: true,
        digest: String::new(),
    };
    value.digest = value.canonical_digest();
    value
}

#[test]
fn delivery_cannot_confirm_from_sender_claim_or_repeat_after_unknown_dispatch() {
    let manifest = manifest();
    let package = package(&manifest);
    let mut ledger = DeliveryAuthorizationLedger::default();
    ledger
        .authorize(authorization(&manifest), &manifest, 20)
        .expect("authorize");
    let dispatch = intent(&manifest);
    ledger
        .request_dispatch(dispatch.clone(), &manifest, 30)
        .expect("dispatch");

    let sender_claim = receipt(
        &manifest,
        &package,
        &dispatch,
        DeliveryReceiptSource::SenderClaim,
        DeliveryEffectOutcome::Delivered,
    );
    assert_eq!(
        ledger
            .record_effect(sender_claim, &manifest, &package)
            .unwrap_err(),
        "delivery_effect_receipt_binding_invalid"
    );

    let unknown = receipt(
        &manifest,
        &package,
        &dispatch,
        DeliveryReceiptSource::Broker,
        DeliveryEffectOutcome::Unknown,
    );
    ledger
        .record_effect(unknown.clone(), &manifest, &package)
        .expect("unknown receipt");
    assert_eq!(
        ledger
            .confirm_recipient(confirmation(&unknown))
            .unwrap_err(),
        "delivery_confirmation_binding_invalid"
    );
    let mut retry = intent(&manifest);
    retry.intent_id = "intent-2".to_owned();
    retry.dispatch_request_id = "request-2".to_owned();
    retry.digest = retry.canonical_digest();
    assert_eq!(
        ledger.request_dispatch(retry, &manifest, 40).unwrap_err(),
        "delivery_unknown_dispatch_fenced"
    );
}

#[test]
fn authorized_recipient_confirms_the_exact_delivery_once() {
    let manifest = manifest();
    let package = package(&manifest);
    let mut ledger = DeliveryAuthorizationLedger::default();
    ledger
        .authorize(authorization(&manifest), &manifest, 20)
        .expect("authorize");
    let dispatch = intent(&manifest);
    ledger
        .request_dispatch(dispatch.clone(), &manifest, 30)
        .expect("dispatch");
    let delivered = receipt(
        &manifest,
        &package,
        &dispatch,
        DeliveryReceiptSource::Broker,
        DeliveryEffectOutcome::Delivered,
    );
    ledger
        .record_effect(delivered.clone(), &manifest, &package)
        .expect("delivered");
    let confirmation = confirmation(&delivered);
    ledger
        .confirm_recipient(confirmation.clone())
        .expect("recipient confirmation");
    ledger
        .confirm_recipient(confirmation)
        .expect("idempotent confirmation");
    assert_eq!(
        ledger.dispatch_for_delivery("delivery-1").unwrap().state,
        DeliveryDispatchState::Confirmed
    );

    let mut wrong_recipient = DeliveryRecipientConfirmation {
        confirmation_id: "confirmation-2".to_owned(),
        ..DeliveryRecipientConfirmation {
            schema: DELIVERY_CONFIRMATION_SCHEMA.to_owned(),
            confirmation_id: "unused".to_owned(),
            receipt_id: delivered.receipt_id.clone(),
            delivery_id: delivered.delivery_id.clone(),
            manifest_digest: delivered.manifest_digest.clone(),
            package_digest: delivered.package_digest.clone(),
            recipient_ref: delivered.recipient_ref.clone(),
            confirmed_by: "principal:other".to_owned(),
            confirmed_at: 50,
            human_confirmed: true,
            digest: String::new(),
        }
    };
    wrong_recipient.digest = wrong_recipient.canonical_digest();
    assert_eq!(
        ledger.confirm_recipient(wrong_recipient).unwrap_err(),
        "delivery_confirmation_binding_invalid"
    );
}

#[test]
fn authorization_rejects_stale_manifest_destination_recipient_and_expiry() {
    let manifest = manifest();
    let mut ledger = DeliveryAuthorizationLedger::default();
    let mut stale = authorization(&manifest);
    stale.manifest_digest = sha('z');
    stale.digest = stale.canonical_digest();
    assert_eq!(
        ledger.authorize(stale, &manifest, 20).unwrap_err(),
        "delivery_authorization_manifest_or_expiry_invalid"
    );
    let mut expired = authorization(&manifest);
    expired.expires_at = 20;
    expired.digest = expired.canonical_digest();
    assert_eq!(
        ledger.authorize(expired, &manifest, 21).unwrap_err(),
        "delivery_authorization_manifest_or_expiry_invalid"
    );
    let mut wrong_recipient = authorization(&manifest);
    wrong_recipient.recipient_ref = "principal:other".to_owned();
    wrong_recipient.digest = wrong_recipient.canonical_digest();
    assert_eq!(
        ledger
            .authorize(wrong_recipient, &manifest, 20)
            .unwrap_err(),
        "delivery_authorization_manifest_or_expiry_invalid"
    );
}
