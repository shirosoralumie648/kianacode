use kiana_domain::{
    json_digest, EmbeddingDevice, EmbeddingIndexBinding, EmbeddingManifest, EmbeddingPooling,
    EmbeddingProvider, EmbeddingRotationPlan,
};
use serde_json::json;
use std::collections::BTreeMap;

fn digest(value: &str) -> String {
    json_digest(&json!({"value": value}))
}

fn manifest(suffix: &str) -> EmbeddingManifest {
    EmbeddingManifest::new(
        format!("local-onnx:{suffix}"),
        digest(&format!("package:{suffix}")),
        digest(&format!("weights:{suffix}")),
        digest(&format!("tokenizer:{suffix}")),
        digest(&format!("config:{suffix}")),
        384,
        EmbeddingPooling::Mean,
        EmbeddingProvider::Cpu,
        EmbeddingDevice::Cpu,
        true,
    )
    .unwrap()
}

#[test]
fn embedding_manifest_mismatch_fails_closed() {
    let good = manifest("v1");
    let rotated = manifest("v2");
    let binding = EmbeddingIndexBinding::new(
        8,
        digest("source-manifest"),
        &good,
        BTreeMap::from([("dense".to_owned(), digest("dense-component"))]),
    )
    .unwrap();
    assert_eq!(
        binding.validate_against(&rotated).unwrap_err(),
        "embedding_index_binding_invalid"
    );
    let mut network = good.clone();
    network.network_allowed = true;
    assert_eq!(
        network.validate().unwrap_err(),
        "embedding_manifest_header_invalid"
    );
}

#[test]
fn model_rotation_keeps_generation_consistent() {
    let old = manifest("v1");
    let new = manifest("v2");
    let plan = EmbeddingRotationPlan::new(8, 9, digest("source-manifest"), &old, &new).unwrap();
    plan.validate().unwrap();
    assert!(plan.reader_allowed(8, &old.manifest_digest));
    assert!(plan.reader_allowed(9, &new.manifest_digest));
    assert!(!plan.reader_allowed(8, &new.manifest_digest));
    assert!(!plan.reader_allowed(10, &new.manifest_digest));
}
