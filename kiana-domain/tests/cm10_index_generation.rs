use kiana_domain::*;
use std::collections::BTreeMap;

fn digest(value: char) -> String {
    format!("sha256:{}", value.to_string().repeat(64))
}

fn components(value: char) -> BTreeMap<String, String> {
    IndexComponentKind::ALL
        .into_iter()
        .map(|kind| (kind.as_str().to_owned(), digest(value)))
        .collect()
}

#[test]
fn readers_never_mix_index_generations() {
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    assert_eq!(
        state.reader().unwrap_err(),
        "index_generation_no_ready_manifest"
    );
    let ready = state.commit(building).unwrap();
    assert_eq!(state.reader().unwrap().generation, ready.generation);

    let next = state.begin(digest('c'), components('d')).unwrap();
    assert_eq!(state.reader().unwrap().source_manifest_digest, digest('a'));
    assert_eq!(next.source_manifest_digest, digest('c'));
    state.validate().unwrap();
}

#[test]
fn failed_rebuild_keeps_last_ready_generation() {
    let mut state = IndexGenerationState::new();
    let building = state.begin(digest('a'), components('b')).unwrap();
    let ready = state.commit(building).unwrap();
    state.begin(digest('c'), components('d')).unwrap();
    state.fail("dense component checksum mismatch").unwrap();
    let reader = state.reader().unwrap();
    assert_eq!(reader.generation, ready.generation);
    assert_eq!(reader.source_manifest_digest, digest('a'));
    assert_eq!(
        state.last_failure.as_deref(),
        Some("dense component checksum mismatch")
    );
}

#[test]
fn incomplete_component_manifest_is_rejected() {
    let mut partial = components('a');
    partial.remove("dense");
    assert_eq!(
        IndexManifest::building(1, digest('b'), partial).unwrap_err(),
        "index_manifest_header_invalid"
    );
}
