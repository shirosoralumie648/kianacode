//! UI-09 cross-language schema round-trip and compatibility gate.
//!
//! The checked-in JSON examples under `docs/schemas/ui/` are the canonical wire shapes recorded in
//! `schema-lock.v1.json`.  This target decodes them with the hand-written Rust DTOs, re-encodes and
//! compares, so a schema change that drifts away from the Rust type fails here before any client
//! grows a second parser.  It is a typed value test: no daemon, no broker, no model loop, no
//! filesystem, process or network access beyond the embedded fixtures.
//!
//! Examples carry synthetic `sha256:000…` digests.  Where the runtime validator recomputes a digest
//! with a public API the test repairs it and then asserts the payload is semantically acceptable;
//! where the digest is computed privately the test asserts the documented refusal instead, so a
//! synthetic digest can never be mistaken for a verified one.

use kiana_client::{
    UiEntityStoreSnapshot, UiStoreError, UI_ACTION_CANCEL_OPERATION, UI_ACTION_CONTINUE_OPERATION,
    UI_ACTION_STATUS_OPERATION, UI_ACTION_SUBMIT_OPERATION, UI_ARTIFACT_PAGE_OPERATION,
    UI_ENTITY_STORE_SNAPSHOT_SCHEMA, UI_FEED_RESUME_OPERATION, UI_FEED_SUBSCRIBE_OPERATION,
    UI_INITIALIZE_OPERATION, UI_QUERY_COMMAND_STATUS_OPERATION, UI_QUERY_HISTORY_OPERATION,
    UI_QUERY_SNAPSHOT_OPERATION,
};
use kiana_domain::{
    check_schema_compatibility, json_digest, schema_contract as registry_contract,
    CompatibilityPolicy, SchemaVersion,
};
use kiana_protocol::{
    UiActionResult, UiActionV1, UiCursor, UiErrorCode, UiFeedCursorV1, UiFeedFrameV1,
    UiHandshakeRequest, UiHandshakeResponse, UiSnapshotV1, UI_ACTION_RESULT_SCHEMA,
    UI_ACTION_SCHEMA, UI_FEED_FRAME_SCHEMA, UI_HANDSHAKE_REQUEST_SCHEMA,
    UI_HANDSHAKE_RESPONSE_SCHEMA, UI_SNAPSHOT_SCHEMA,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

const LOCK: &str = include_str!("../../docs/schemas/ui/schema-lock.v1.json");
const MATRIX: &str = include_str!("../../docs/schemas/ui/compatibility-matrix.v1.json");

const ACTION_EXAMPLE: &str = include_str!("../../docs/schemas/ui/examples/kiana.ui-action.v1.json");
const ACTION_RESULT_EXAMPLE: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-action-result.v1.json");
const FEED_FRAME_EXAMPLE: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-feed-frame.v1.json");
const HANDSHAKE_REQUEST_EXAMPLE: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-handshake-request.v1.json");
const HANDSHAKE_RESPONSE_EXAMPLE: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-handshake-response.v1.json");
const SNAPSHOT_EXAMPLE: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-snapshot.v1.json");
const ENTITY_STORE_EXAMPLE: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-entity-store-snapshot.v1.json");

const ACTION_UNKNOWN_FIELD: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-action.v1.unknown-field.json");
const ACTION_RESULT_UNKNOWN_FIELD: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-action-result.v1.unknown-field.json");
const FEED_FRAME_UNKNOWN_FIELD: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-feed-frame.v1.unknown-field.json");
const HANDSHAKE_REQUEST_UNKNOWN_FIELD: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-handshake-request.v1.unknown-field.json");
const HANDSHAKE_RESPONSE_UNKNOWN_FIELD: &str = include_str!(
    "../../docs/schemas/ui/examples/kiana.ui-handshake-response.v1.unknown-field.json"
);
const SNAPSHOT_UNKNOWN_FIELD: &str =
    include_str!("../../docs/schemas/ui/examples/kiana.ui-snapshot.v1.unknown-field.json");
const ENTITY_STORE_UNKNOWN_FIELD: &str = include_str!(
    "../../docs/schemas/ui/examples/kiana.ui-entity-store-snapshot.v1.unknown-field.json"
);

const ACTION_REMOVE_REQUIRED: &str =
    include_str!("../../docs/schemas/ui/negative/kiana.ui-action.v1.remove-required-field.json");
const ACTION_CHANGE_TYPE: &str =
    include_str!("../../docs/schemas/ui/negative/kiana.ui-action.v1.change-field-type.json");
const ACTION_UNKNOWN_MAJOR: &str =
    include_str!("../../docs/schemas/ui/negative/kiana.ui-action.v1.unknown-major.json");

fn locked_max_bytes(id: &str) -> usize {
    let lock: Value = serde_json::from_str(LOCK).expect("schema lock is JSON");
    lock["contracts"]
        .as_array()
        .expect("lock contracts")
        .iter()
        .find(|contract| contract["id"].as_str() == Some(id))
        .map(|contract| contract["max_bytes"].as_u64().expect("max_bytes") as usize)
        .expect("locked contract")
}

fn compact_len(fixture: &str) -> usize {
    let value: Value = serde_json::from_str(fixture).expect("fixture is JSON");
    serde_json::to_vec(&value)
        .expect("fixture re-encodes")
        .len()
}

/// Decode, re-encode and decode again: the JSON example and the Rust wire type must agree both ways.
fn round_trip<T>(id: &str, fixture: &str) -> T
where
    T: DeserializeOwned + Serialize + PartialEq + std::fmt::Debug,
{
    let max_bytes = locked_max_bytes(id);
    assert!(
        compact_len(fixture) <= max_bytes,
        "{id} fixture exceeds the locked byte ceiling"
    );
    let decoded: T = serde_json::from_str(fixture).unwrap_or_else(|error| {
        panic!("{id} example must decode with the Rust wire type: {error}")
    });
    let encoded = serde_json::to_string(&decoded).expect("decoded payload re-encodes");
    assert!(
        encoded.len() <= max_bytes,
        "{id} re-encoded payload exceeds the locked byte ceiling"
    );
    let again: T = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("{id} re-encoded payload must decode again: {error}"));
    assert_eq!(decoded, again, "{id} round-trip must be stable");
    decoded
}

#[test]
fn ui09_locked_examples_round_trip_through_the_rust_wire_types() {
    let mut action: UiActionV1 = round_trip("kiana.ui-action.v1", ACTION_EXAMPLE);
    assert_eq!(action.schema, UI_ACTION_SCHEMA);
    action.payload_digest = json_digest(&action.payload);
    action
        .validate()
        .expect("the locked action example is valid once its synthetic digest is recomputed");

    let result: UiActionResult = round_trip("kiana.ui-action-result.v1", ACTION_RESULT_EXAMPLE);
    assert_eq!(result.schema, UI_ACTION_RESULT_SCHEMA);
    result
        .validate()
        .expect("the locked action result example is valid");

    let frame: UiFeedFrameV1 = round_trip("kiana.ui-feed-frame.v1", FEED_FRAME_EXAMPLE);
    assert_eq!(frame.schema, UI_FEED_FRAME_SCHEMA);
    assert_eq!(
        frame
            .validate()
            .expect_err("a synthetic cursor digest is refused"),
        "ui_feed_cursor_digest_mismatch"
    );
    let mut repaired_frame = frame.clone();
    repaired_frame.cursor = UiFeedCursorV1::new(
        "instance-example",
        "epoch-example",
        4,
        UiCursor {
            epoch: "epoch-example".to_owned(),
            sequence: 2,
        },
    )
    .expect("the example cursor position is valid");
    repaired_frame
        .validate()
        .expect("the locked feed frame is valid with a recomputed cursor digest");

    let request: UiHandshakeRequest =
        round_trip("kiana.ui-handshake-request.v1", HANDSHAKE_REQUEST_EXAMPLE);
    assert_eq!(request.schema, UI_HANDSHAKE_REQUEST_SCHEMA);
    request
        .validate()
        .expect("the locked handshake request is valid");

    let response: UiHandshakeResponse =
        round_trip("kiana.ui-handshake-response.v1", HANDSHAKE_RESPONSE_EXAMPLE);
    assert_eq!(response.schema, UI_HANDSHAKE_RESPONSE_SCHEMA);
    response
        .validate()
        .expect("the locked handshake response is valid");

    let mut snapshot: UiSnapshotV1 = round_trip("kiana.ui-snapshot.v1", SNAPSHOT_EXAMPLE);
    assert_eq!(snapshot.schema, UI_SNAPSHOT_SCHEMA);
    let principal_digest = snapshot.principal.digest();
    snapshot.principal.principal_digest = principal_digest;
    snapshot.validate().expect(
        "the locked snapshot example is valid once its synthetic principal digest is bound",
    );

    let store: UiEntityStoreSnapshot =
        round_trip("kiana.ui-entity-store-snapshot.v1", ENTITY_STORE_EXAMPLE);
    assert_eq!(store.schema, UI_ENTITY_STORE_SNAPSHOT_SCHEMA);
    match store.validate() {
        Err(UiStoreError::Invalid(code)) => {
            assert_eq!(code, "ui_store_snapshot_digest_mismatch")
        }
        other => panic!("a synthetic store snapshot digest must be refused, got {other:?}"),
    }
}

#[test]
fn ui09_unknown_field_examples_are_rejected_by_strict_rust_decode() {
    let rejections: [(&str, &str, &str); 7] = [
        ("kiana.ui-action.v1", ACTION_UNKNOWN_FIELD, "unknown field"),
        (
            "kiana.ui-action-result.v1",
            ACTION_RESULT_UNKNOWN_FIELD,
            "unknown field",
        ),
        (
            "kiana.ui-feed-frame.v1",
            FEED_FRAME_UNKNOWN_FIELD,
            "unknown field",
        ),
        (
            "kiana.ui-handshake-request.v1",
            HANDSHAKE_REQUEST_UNKNOWN_FIELD,
            "unknown field",
        ),
        (
            "kiana.ui-handshake-response.v1",
            HANDSHAKE_RESPONSE_UNKNOWN_FIELD,
            "unknown field",
        ),
        (
            "kiana.ui-snapshot.v1",
            SNAPSHOT_UNKNOWN_FIELD,
            "unknown field",
        ),
        (
            "kiana.ui-entity-store-snapshot.v1",
            ENTITY_STORE_UNKNOWN_FIELD,
            "unknown field",
        ),
    ];
    for (id, fixture, marker) in rejections {
        let error = decode_error(id, fixture);
        assert!(
            error.contains(marker),
            "{id} must be rejected for the unknown field, got: {error}"
        );
    }
}

fn decode_error(id: &str, fixture: &str) -> String {
    let outcome: Result<(), serde_json::Error> = match id {
        "kiana.ui-action.v1" => serde_json::from_str::<UiActionV1>(fixture).map(|_| ()),
        "kiana.ui-action-result.v1" => serde_json::from_str::<UiActionResult>(fixture).map(|_| ()),
        "kiana.ui-feed-frame.v1" => serde_json::from_str::<UiFeedFrameV1>(fixture).map(|_| ()),
        "kiana.ui-handshake-request.v1" => {
            serde_json::from_str::<UiHandshakeRequest>(fixture).map(|_| ())
        }
        "kiana.ui-handshake-response.v1" => {
            serde_json::from_str::<UiHandshakeResponse>(fixture).map(|_| ())
        }
        "kiana.ui-snapshot.v1" => serde_json::from_str::<UiSnapshotV1>(fixture).map(|_| ()),
        "kiana.ui-entity-store-snapshot.v1" => {
            serde_json::from_str::<UiEntityStoreSnapshot>(fixture).map(|_| ())
        }
        other => panic!("unbound schema id {other}"),
    };
    match outcome {
        Err(error) => error.to_string(),
        Ok(()) => panic!("{id} unknown-field fixture must not decode"),
    }
}

#[test]
fn ui09_version_and_command_gates_fail_closed_before_decode() {
    let lock: Value = serde_json::from_str(LOCK).expect("schema lock is JSON");
    let ids = lock["contracts"]
        .as_array()
        .expect("lock contracts")
        .iter()
        .map(|contract| contract["id"].as_str().expect("contract id").to_owned())
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 7, "the lock covers the seven UI DTO boundaries");

    for id in &ids {
        check_schema_compatibility(id, &SchemaVersion::new(1, 0))
            .unwrap_or_else(|error| panic!("{id} locked version must be accepted: {error}"));
        let error = check_schema_compatibility(id, &SchemaVersion::new(2, 0))
            .expect_err("an unknown major must fail closed");
        assert!(
            error.contains("incompatible schema version"),
            "{id}: {error}"
        );
        let bumped = id.replace(".v1", ".v2");
        assert!(
            registry_contract(&bumped).is_none(),
            "{bumped} must stay unregistered"
        );
        assert!(
            check_schema_compatibility(&bumped, &SchemaVersion::new(2, 0)).is_err(),
            "{bumped} must be refused as an unknown schema"
        );
    }

    let catalog = [
        UI_INITIALIZE_OPERATION,
        UI_QUERY_SNAPSHOT_OPERATION,
        UI_QUERY_HISTORY_OPERATION,
        UI_QUERY_COMMAND_STATUS_OPERATION,
        UI_FEED_SUBSCRIBE_OPERATION,
        UI_FEED_RESUME_OPERATION,
        UI_ARTIFACT_PAGE_OPERATION,
        UI_ACTION_SUBMIT_OPERATION,
        UI_ACTION_CANCEL_OPERATION,
        UI_ACTION_CONTINUE_OPERATION,
        UI_ACTION_STATUS_OPERATION,
    ];
    assert_eq!(catalog.len(), 11, "the typed surface stays enumerable");
    let mut unique = catalog.to_vec();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), 11, "operation names must stay distinct");
    assert!(
        !catalog.contains(&"ui.action.explode"),
        "an unnegotiated command name is never part of the catalog"
    );
    assert!(catalog.iter().all(|operation| operation.starts_with("ui.")));
    assert_eq!(UiErrorCode::ALL.len(), 11);
    assert_eq!(
        serde_json::to_value(UiErrorCode::SchemaUnsupported).expect("error code encodes"),
        json!("schema_unsupported"),
        "an unintelligible command is reported with the schema-unsupported code"
    );
}

#[test]
fn ui09_lock_contracts_are_registered_and_negative_fixtures_stay_refused() {
    let matrix: Value = serde_json::from_str(MATRIX).expect("compatibility matrix is JSON");
    assert_eq!(
        matrix["same_version_policy"].as_str(),
        Some("frozen_shape"),
        "a same-version shape change requires a reviewed lock update"
    );
    assert_eq!(
        matrix["unknown_major_policy"].as_str(),
        Some("reject_before_decode")
    );
    assert_eq!(
        matrix["unknown_command_policy"].as_str(),
        Some("reject_with_schema_unsupported")
    );

    let lock: Value = serde_json::from_str(LOCK).expect("schema lock is JSON");
    for contract in lock["contracts"].as_array().expect("lock contracts") {
        let id = contract["id"].as_str().expect("contract id");
        let registered = registry_contract(id).expect("a locked schema must be registered");
        assert_eq!(
            registered.owner_crate,
            contract["owner_crate"].as_str().expect("owner crate"),
            "{id} owner crate must match the lock"
        );
        assert_eq!(registered.version, SchemaVersion::new(1, 0), "{id}");
        assert_eq!(
            registered.compatibility,
            CompatibilityPolicy::Breaking,
            "{id} must not accept a silent same-version shape change"
        );
        assert!(
            !registered.allow_unknown_fields,
            "{id} must deny unknown fields"
        );
        assert_eq!(contract["unknown_fields"].as_str(), Some("deny"), "{id}");
    }

    assert!(
        serde_json::from_str::<UiActionV1>(ACTION_REMOVE_REQUIRED).is_err(),
        "removing a required field without a major must be refused"
    );
    assert!(
        serde_json::from_str::<UiActionV1>(ACTION_CHANGE_TYPE).is_err(),
        "changing a field type within the same major must be refused"
    );
    let bumped: UiActionV1 = serde_json::from_str(ACTION_UNKNOWN_MAJOR)
        .expect("an unknown major still decodes as an opaque string field");
    assert_eq!(bumped.schema, "kiana.ui-action.v2");
    assert_eq!(
        bumped
            .validate()
            .expect_err("the schema identity check fires first"),
        "ui_action_schema_invalid"
    );
}
