//! UI-09 cross-language emit: print the canonical encodings of the locked UI DTO examples.
//!
//! The checked-in JSON examples under `docs/schemas/ui/examples/` are the canonical wire shapes
//! recorded in `schema-lock.v1.json`.  This binary decodes each example with the hand-written Rust
//! wire type, re-encodes it, and prints one JSON report to stdout.  `--emitted` in
//! `scripts/validate-ui09-schemas.py` then validates every emitted payload against the canonical
//! JSON Schema, which is the cross-language half of the round trip; the reverse half is the typed
//! round-trip target `kiana-client/tests/ui09_schema_round_trip.rs`.
//!
//! The binary is a pure value transform: no filesystem, process, network, daemon, broker, model loop
//! or capability path is touched, and the report only carries the already-redacted fixture values.

use kiana_client::UiEntityStoreSnapshot;
use kiana_protocol::{
    UiActionResult, UiActionV1, UiFeedFrameV1, UiHandshakeRequest, UiHandshakeResponse,
    UiSnapshotV1,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Map, Value};

const REPORT_SCHEMA: &str = "kiana.ui09-emitted.v1";

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

/// Decode one locked example with its Rust wire type and re-encode it as a JSON value.
fn emit<T>(id: &str, fixture: &str) -> (String, Value)
where
    T: DeserializeOwned + Serialize,
{
    let decoded: T = serde_json::from_str(fixture).unwrap_or_else(|error| {
        panic!("{id} example must decode with the Rust wire type: {error}")
    });
    let encoded = serde_json::to_value(&decoded)
        .unwrap_or_else(|error| panic!("{id} decoded payload must re-encode as JSON: {error}"));
    (id.to_owned(), encoded)
}

fn main() {
    let mut contracts = Map::new();
    for (id, value) in [
        emit::<UiActionV1>("kiana.ui-action.v1", ACTION_EXAMPLE),
        emit::<UiActionResult>("kiana.ui-action-result.v1", ACTION_RESULT_EXAMPLE),
        emit::<UiEntityStoreSnapshot>("kiana.ui-entity-store-snapshot.v1", ENTITY_STORE_EXAMPLE),
        emit::<UiFeedFrameV1>("kiana.ui-feed-frame.v1", FEED_FRAME_EXAMPLE),
        emit::<UiHandshakeRequest>("kiana.ui-handshake-request.v1", HANDSHAKE_REQUEST_EXAMPLE),
        emit::<UiHandshakeResponse>("kiana.ui-handshake-response.v1", HANDSHAKE_RESPONSE_EXAMPLE),
        emit::<UiSnapshotV1>("kiana.ui-snapshot.v1", SNAPSHOT_EXAMPLE),
    ] {
        contracts.insert(id, value);
    }

    let report = json!({
        "schema": REPORT_SCHEMA,
        "contracts": Value::Object(contracts),
    });
    let encoded = serde_json::to_string_pretty(&report)
        .expect("the emitted UI schema report is serializable JSON");
    println!("{encoded}");
}
