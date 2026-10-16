use kiana_domain::{BillingPriceDimension, RateCard, RateCardId, UsageVector};
use kiana_ports::{InMemoryRateCardStore, RateCardStore, MAX_RATE_CARD_STORE_CARDS};

fn card(id: RateCardId, from: u64, to: Option<u64>, model: &str, card_version: u64) -> RateCard {
    let mut card = RateCard::new(
        id,
        "provider:test",
        model,
        "USD",
        Some(2),
        Some(3),
        from,
        to,
        card_version,
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
    let rate_card = card(id, 1_000, Some(10_000), "model:test", 1);
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
async fn rate_card_store_uses_provider_wildcard_after_exact_model_lookup() {
    let store = InMemoryRateCardStore::new();
    let wildcard = card(RateCardId::new(), 1_000, Some(10_000), "*", 1);
    let wildcard_id = wildcard.rate_card_id;
    store.insert(wildcard).await.unwrap();

    let resolved = store
        .resolve_rate_card("provider:test", "model:unlisted", 2_000)
        .await
        .expect("provider wildcard fallback");
    assert_eq!(resolved.rate_card_id, wildcard_id);

    let exact = card(RateCardId::new(), 1_000, Some(10_000), "model:unlisted", 2);
    let exact_id = exact.rate_card_id;
    store.insert(exact).await.unwrap();
    let resolved = store
        .resolve_rate_card("provider:test", "model:unlisted", 2_000)
        .await
        .expect("exact model override");
    assert_eq!(resolved.rate_card_id, exact_id);
    assert_eq!(resolved.card_version, 2);
}

#[tokio::test]
async fn rate_card_store_rejects_overlap_unknown_model_and_expiry() {
    let store = InMemoryRateCardStore::new();
    store
        .insert(card(
            RateCardId::new(),
            1_000,
            Some(10_000),
            "model:test",
            1,
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .insert(card(
                RateCardId::new(),
                20_000,
                Some(30_000),
                "model:test",
                1,
            ))
            .await
            .unwrap_err()
            .to_string(),
        "port_conflict:rate_card_duplicate_version"
    );
    assert_eq!(
        store
            .insert(card(
                RateCardId::new(),
                9_000,
                Some(12_000),
                "model:test",
                2,
            ))
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

#[tokio::test]
async fn rate_card_store_rejects_nil_identity_before_lookup() {
    let store = InMemoryRateCardStore::new();
    let nil = RateCardId::parse_str("00000000-0000-0000-0000-000000000000")
        .expect("nil UUID parses as a typed id");
    assert_eq!(
        store.read_rate_card(nil).await.unwrap_err().to_string(),
        "port_failed:rate_card_lookup_invalid"
    );
}

#[tokio::test]
async fn rate_card_store_rejects_zero_lookup_timestamp_before_lookup() {
    let store = InMemoryRateCardStore::new();
    store
        .insert(card(
            RateCardId::new(),
            1_000,
            Some(10_000),
            "model:test",
            1,
        ))
        .await
        .unwrap();
    assert_eq!(
        store
            .resolve_rate_card("provider:test", "model:test", 0)
            .await
            .unwrap_err()
            .to_string(),
        "port_failed:rate_card_lookup_invalid"
    );
}

#[tokio::test]
async fn rate_card_store_rejects_capacity_overflow_without_growing() {
    let store = InMemoryRateCardStore::new();
    for index in 0..MAX_RATE_CARD_STORE_CARDS {
        store
            .insert(card(
                RateCardId::new(),
                1_000,
                Some(10_000),
                &format!("model:{index}"),
                1,
            ))
            .await
            .expect("card within bounded fixture capacity");
    }

    let overflow = card(RateCardId::new(), 1_000, Some(10_000), "model:overflow", 1);
    assert_eq!(
        store.insert(overflow).await.unwrap_err().to_string(),
        "port_unavailable:rate_card_capacity_exceeded"
    );
    let retained = store
        .resolve_rate_card("provider:test", "model:255", 2_000)
        .await
        .expect("capacity denial must retain existing cards");
    assert_eq!(retained.model_selector, "model:255");
}

#[tokio::test]
async fn rate_card_store_rejects_zero_card_version_at_insert() {
    let store = InMemoryRateCardStore::new();
    // `RateCard::new` refuses version zero, so build a digest-consistent card and mutate the
    // version to exercise the store-side deny boundary directly.
    let mut zero_version = card(
        RateCardId::new(),
        1_000,
        Some(10_000),
        "model:zero-version",
        1,
    );
    zero_version.card_version = 0;
    zero_version.rate_card_digest = zero_version.digest();
    assert_eq!(zero_version.validate().unwrap_err(), "rate_card_invalid");
    assert_eq!(
        store.insert(zero_version).await.unwrap_err().to_string(),
        "port_failed:rate_card_invalid:rate_card_invalid"
    );
    assert_eq!(
        store
            .resolve_rate_card("provider:test", "model:zero-version", 2_000)
            .await
            .unwrap_err()
            .to_string(),
        "port_unavailable:rate_card_unknown_model"
    );
}
