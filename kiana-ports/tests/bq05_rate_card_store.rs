use kiana_domain::{BillingPriceDimension, RateCard, RateCardId, UsageVector};
use kiana_ports::{InMemoryRateCardStore, RateCardStore};

fn card(id: RateCardId, from: u64, to: Option<u64>, model: &str) -> RateCard {
    let mut card = RateCard::new(
        id,
        "provider:test",
        model,
        "USD",
        Some(2),
        Some(3),
        from,
        to,
        1,
        "fixture",
    )
    .unwrap();
    card.cache_read_price_per_unit = Some(1);
    card.audio_input_price_per_unit = Some(4);
    card.audio_output_price_per_unit = Some(5);
    card.effect_price = Some(6);
    card.rate_card_digest = card.digest();
    card.validate().unwrap();
    card
}

#[tokio::test]
async fn rate_card_store_resolves_pinned_model_provider_mapping() {
    let store = InMemoryRateCardStore::new();
    let id = RateCardId::new();
    let rate_card = card(id, 1_000, Some(10_000), "model:test");
    store.insert(rate_card.clone()).await.unwrap();
    let resolved = store
        .resolve_rate_card("provider:test", "model:test", 2_000)
        .await
        .unwrap();
    assert_eq!(resolved.rate_card_id, id);
    assert_eq!(resolved.card_version, 1);
    assert_eq!(
        resolved.price_for(BillingPriceDimension::AudioInputTokens),
        Some(4)
    );
    assert_eq!(store.read_rate_card(id).await.unwrap(), Some(rate_card));

    let mut usage = UsageVector::zero();
    usage.audio_input_tokens = Some(2);
    usage.effect_count = 3;
    let estimate = resolved.estimate(&usage, 0).unwrap();
    assert_eq!(estimate.rate_card_version, 1);
    assert_eq!(estimate.amount.unwrap().micros, 2 * 4 + 3 * 6);
}

#[tokio::test]
async fn rate_card_store_rejects_overlap_unknown_model_and_expiry() {
    let store = InMemoryRateCardStore::new();
    store
        .insert(card(RateCardId::new(), 1_000, Some(10_000), "model:test"))
        .await
        .unwrap();
    assert_eq!(
        store
            .insert(card(RateCardId::new(), 9_000, Some(12_000), "model:test"))
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:rate_card_effective_overlap"
    );
    assert_eq!(
        store
            .resolve_rate_card("provider:test", "unknown", 2_000)
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:rate_card_unknown_model"
    );
    assert_eq!(
        store
            .resolve_rate_card("provider:test", "model:test", 10_000)
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:rate_card_expired"
    );
}
