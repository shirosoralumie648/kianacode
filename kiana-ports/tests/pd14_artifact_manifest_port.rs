//! PD-14 `ArtifactManifest` and `ContentAddressedArtifactStorePort` contract fixtures.
//!
//! The manifest is the value a reader re-derives after a reopen, so these fixtures perturb exactly
//! the fields the seal covers: a blob digest that names different bytes, a schema tag from another
//! version, a version that no longer validates, and a hand-edited seal. Denials come first; the
//! round-trip and normalized-spelling cases follow. The port defaults are also pinned here: a store
//! that has not opted in must deny every method with one stable reason instead of looking empty.

use kiana_domain::{
    journal_sha256, ArtifactId, ArtifactProvenance, ArtifactVersion, MAX_ARTIFACT_BYTES,
};
use kiana_ports::{
    ArtifactManifest, ContentAddressedArtifactStorePort, PortError, ARTIFACT_MANIFEST_SCHEMA,
};

const SCOPE: &str = "sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const OTHER_DIGEST: &str =
    "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";

fn provenance() -> ArtifactProvenance {
    ArtifactProvenance {
        producer_kind: "fixture".to_owned(),
        producer_id: "pd14".to_owned(),
        source_event_id: None,
        source_run_id: None,
        recorded_by: "ci".to_owned(),
    }
}

fn version_for(content: &[u8]) -> ArtifactVersion {
    ArtifactVersion::new(
        ArtifactId::new(),
        1,
        "text/plain",
        content,
        SCOPE,
        provenance(),
        100,
    )
    .expect("valid version")
}

#[test]
fn manifest_rejects_blob_digest_schema_version_and_seal_drift() {
    let content = b"pd14 manifest bytes";
    let version = version_for(content);

    // A blob digest that names other bytes is a contradiction, not a hint.
    assert_eq!(
        ArtifactManifest::new(version.clone(), OTHER_DIGEST).unwrap_err(),
        "artifact_manifest_blob_digest_invalid"
    );
    assert_eq!(
        ArtifactManifest::new(version.clone(), "not-a-digest").unwrap_err(),
        "artifact_manifest_blob_digest_invalid"
    );

    let manifest =
        ArtifactManifest::new(version.clone(), journal_sha256(content)).expect("manifest");

    let mut foreign_schema = manifest.clone();
    foreign_schema.schema = "kiana.artifact-manifest.v0".to_owned();
    assert_eq!(
        foreign_schema.validate().unwrap_err(),
        "artifact_manifest_schema_invalid"
    );

    let mut invalid_version = manifest.clone();
    invalid_version.version.immutable = false;
    assert_eq!(
        invalid_version.validate().unwrap_err(),
        "artifact_version_invalid"
    );
    let mut zero_version = manifest.clone();
    zero_version.version.version = 0;
    assert_eq!(
        zero_version.validate().unwrap_err(),
        "artifact_version_invalid"
    );

    let mut edited_seal = manifest.clone();
    edited_seal.manifest_digest = OTHER_DIGEST.to_owned();
    assert_eq!(
        edited_seal.validate().unwrap_err(),
        "artifact_manifest_digest_mismatch"
    );
    let mut truncated_seal = manifest.clone();
    truncated_seal.manifest_digest = "sha256:".to_owned();
    assert_eq!(
        truncated_seal.validate().unwrap_err(),
        "artifact_manifest_digest_mismatch"
    );

    // The construction itself refuses the invalid combinations: nothing above ever produced a
    // manifest that claimed to be valid while pointing at different bytes.
    assert!(manifest.blob_names_version_content());
    assert_eq!(manifest.validate(), Ok(()));
}

#[test]
fn manifest_accepts_both_digest_spellings_and_round_trips_through_json() {
    let content = b"pd14 manifest bytes";
    let version = version_for(content);
    let bare = ArtifactManifest::new(version.clone(), journal_sha256(content)).expect("bare hex");
    let prefixed =
        ArtifactManifest::new(version, format!("sha256:{}", journal_sha256(content))).expect("hex");
    assert!(bare.blob_names_version_content());
    assert!(prefixed.blob_names_version_content());
    let seal_hex = bare
        .manifest_digest
        .strip_prefix("sha256:")
        .expect("the seal is a json_digest");
    assert_eq!(
        seal_hex.len(),
        64,
        "the seal carries a full sha256 hex body"
    );

    let encoded = serde_json::to_string(&prefixed).expect("encode");
    let decoded: ArtifactManifest = serde_json::from_str(&encoded).expect("decode");
    assert_eq!(decoded, prefixed);
    assert_eq!(decoded.validate(), Ok(()));

    // The wire object has exactly the three sealed fields; a fourth field is not silently ignored.
    let mut object: serde_json::Value = serde_json::from_str(&encoded).expect("json");
    object["extra"] = serde_json::json!("smuggled");
    assert!(serde_json::from_value::<ArtifactManifest>(object).is_err());

    assert_eq!(ARTIFACT_MANIFEST_SCHEMA, "kiana.artifact-manifest.v1");
}

/// A store that implements only the required boundary and inherits every fail-closed default.
struct UnsupportedStore;

impl ContentAddressedArtifactStorePort for UnsupportedStore {
    fn owner_scope_digest(&self) -> String {
        SCOPE.to_owned()
    }

    fn max_artifact_bytes(&self) -> usize {
        MAX_ARTIFACT_BYTES
    }
}

#[tokio::test]
async fn port_defaults_deny_with_one_stable_reason() {
    let store = UnsupportedStore;
    let version = version_for(b"denied");
    let reference = version.as_ref();
    assert_eq!(store.owner_scope_digest(), SCOPE);
    assert_eq!(store.max_artifact_bytes(), MAX_ARTIFACT_BYTES);

    let denials = [
        store
            .stage_content(version, b"denied".to_vec())
            .await
            .unwrap_err(),
        store.commit_content(&reference).await.unwrap_err(),
        store.read_manifest(&reference).await.unwrap_err(),
        store.read_content(&reference).await.unwrap_err(),
        store.verify_content(&reference).await.unwrap_err(),
        store
            .blob_present(&journal_sha256(b"denied"))
            .await
            .unwrap_err(),
    ];
    for denial in denials {
        assert_eq!(
            denial,
            PortError::Unavailable("content_addressed_artifact_store_unsupported".to_owned()),
            "every default must deny with the same reason"
        );
    }
}
