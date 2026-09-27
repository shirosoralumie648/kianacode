//! BQ-26 adapter-backed seam: the three cases whose seams are test dependencies of `kiana-core`.
//!
//! These live in a test target on purpose. `kiana-eventlog` and `kiana-provider` are
//! dev-dependencies of this crate, so a library module cannot link them; making them library
//! dependencies would pull reqwest/TLS into every `kiana-core` consumer for a fixture. The
//! contract stays in `src/bq26_fault_harness.rs`; only the *calls* live here.
//!
//! Everything below is a real call. `MemoryEventLog::commit_transition` runs the real
//! `plan_transition` read-set check, and `kiana_provider::replay_stream_fixture` runs the real
//! `Framer` and `Accumulator` over the supplied bytes. Nothing is simulated and no fault is
//! described rather than injected.

use kiana_core::{
    bq26_fixture_deadline, bq26_fixture_digest, bq26_fixture_now, Bq26FaultCase,
    Bq26FaultObservation, Bq26FaultRefusal, Bq26FaultSeam,
};
use kiana_domain::{
    AggregateVersion, AttemptId, CommitOutcome, ModelProtocol, ModelSideEffectState, RequestId,
    RunId, RuntimeEvent, TransitionBatch,
};
use kiana_eventlog::MemoryEventLog;
use kiana_ports::EventStorePort;
use kiana_provider::replay_stream_fixture;

/// Drives the three adapter-backed cases and delegates everything else to the library seam.
pub struct AdapterSeam {
    inner: LibrarySeam,
}

/// Re-enters the library's own seam implementation for the nine non-adapter cases.
pub struct LibrarySeam;

impl Default for LibrarySeam {
    fn default() -> Self {
        Self
    }
}

impl Bq26FaultSeam for LibrarySeam {
    fn inject(&mut self, case: Bq26FaultCase) -> Result<Bq26FaultObservation, String> {
        kiana_core::bq26_inject_library_case(case)
    }
}

impl Bq26FaultSeam for AdapterSeam {
    fn inject(&mut self, case: Bq26FaultCase) -> Result<Bq26FaultObservation, String> {
        match case {
            Bq26FaultCase::CasRace => drive_cas_race(),
            Bq26FaultCase::PartialFrame => drive_partial_frame(),
            Bq26FaultCase::NetworkEof => drive_network_eof(),
            other => self.inner.inject(other),
        }
    }
}

/// Build the full 12-case report by driving every case through the real seams.
pub fn run_all(
    seed: u64,
    source_cursor: u64,
    source_event_ids: Vec<String>,
) -> kiana_core::Bq26FaultHarnessRun {
    let mut seam = AdapterSeam { inner: LibrarySeam };
    kiana_core::Bq26FaultHarnessRun::evaluate_with_seam(
        seed,
        source_cursor,
        source_event_ids,
        &mut seam,
    )
    .expect("bq26 adapter seam run")
}

/// CAS race: two writers present the same expected aggregate version. The in-memory store's real
/// read-set check must let exactly one commit and hand the loser a `CommitOutcome::Conflict`.
fn drive_cas_race() -> Result<Bq26FaultObservation, String> {
    const AGGREGATE_TYPE: &str = "billing_attempt";

    let run_id = RunId::new();
    let attempt_id = AttemptId::new();
    let aggregate_id = format!("bq26-cas-race-{attempt_id}");

    let batch_for = |command_id: RequestId, sequence: u64| -> Result<TransitionBatch, String> {
        let event = RuntimeEvent::new(
            command_id,
            sequence,
            "usage.settled",
            serde_json::json!({
                "run_id": run_id.to_string(),
                "attempt_id": attempt_id.to_string(),
                "sequence": sequence,
            }),
        )
        .map_err(|error| error.to_string())?
        .with_stream_metadata(AGGREGATE_TYPE, &aggregate_id, sequence);
        Ok(TransitionBatch {
            command_id,
            command_digest: bq26_fixture_digest('c'),
            expected_versions: vec![AggregateVersion::new(AGGREGATE_TYPE, &aggregate_id, 0)],
            events: vec![event],
        })
    };

    // Build both batches before entering the runtime: `?` cannot cross an async block that
    // returns a tuple, and building them up front is also how the race actually happens — both
    // writers read version 0 before either one writes.
    let first_batch = batch_for(RequestId::new(), 1)?;
    let second_batch = batch_for(RequestId::new(), 1)?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let (first, second, all) = runtime.block_on(async {
        let store = MemoryEventLog::new();
        (
            store.commit_transition(first_batch).await,
            // The second writer was built from the same read (version 0) and is stale by the time
            // it lands. This is the race the CAS spine has to refuse.
            store.commit_transition(second_batch).await,
            store.read_all().await,
        )
    });

    if !matches!(
        first.map_err(|e| e.to_string())?,
        CommitOutcome::Committed { .. }
    ) {
        return Err("bq26_cas_race_first_writer_not_committed".to_owned());
    }
    let second = second.map_err(|e| e.to_string())?;
    let CommitOutcome::Conflict { changed } = second else {
        return Err("bq26_cas_race_stale_writer_not_conflicted".to_owned());
    };
    if !changed.iter().any(|v| v.aggregate_id == aggregate_id) {
        return Err("bq26_cas_race_conflict_missing_aggregate".to_owned());
    }
    let all = all.map_err(|error| error.to_string())?;
    let survivors = all
        .iter()
        .filter(|event| {
            event.aggregate_type.as_deref() == Some(AGGREGATE_TYPE)
                && event.aggregate_id.as_deref() == Some(aggregate_id.as_str())
        })
        .collect::<Vec<&RuntimeEvent>>();
    if survivors.len() != 1 {
        return Err("bq26_cas_race_duplicate_fact".to_owned());
    }
    // "故障后事实可重放": the surviving fact is a well-formed, contiguous, readable stream.
    if survivors[0].stream_version != Some(1) {
        return Err("bq26_cas_race_stream_version_not_contiguous".to_owned());
    }
    if kiana_core::project_model_attempt_lifecycle(&[]).is_ok() {
        return Err("bq26_cas_race_empty_projection_accepted".to_owned());
    }

    Bq26FaultObservation::new(
        Bq26FaultCase::CasRace,
        Bq26FaultRefusal::new(
            "MemoryEventLog::commit_transition",
            "event_store_transition_conflict",
            Some("CommitOutcome::Committed".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "re-read the winning aggregate version before the next settlement write",
    )
}

/// Partial frame: a provider stream that stops mid-frame. The provider's own `Framer` is driven
/// through the public `replay_stream_fixture` seam, so this is the production parser.
fn drive_partial_frame() -> Result<Bq26FaultObservation, String> {
    let prepared = fixture_prepared()?;
    // An SSE frame opened and never closed: `data:` with no terminating blank line.
    let chunks: Vec<Vec<u8>> = vec![b"data: {\"type\":\"content_block_delta\"".to_vec()];
    let error = replay_stream_fixture(prepared, &chunks, 4_096)
        .expect_err("a partial frame must not parse as a reply")
        .code;
    if error != "provider_frame_truncated" {
        return Err(format!("bq26_partial_frame_unexpected_code:{error}"));
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::PartialFrame,
        Bq26FaultRefusal::new(
            "kiana_provider::replay_stream_fixture",
            "provider_frame_truncated",
            Some("ModelReply".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "discard the partial frame and re-request within the bounded retry policy",
    )
}

/// Network EOF: a stream that ends on a frame boundary but never sends a terminal event.
/// `Framer::finish` succeeds, so the refusal has to come from the accumulator, not the framer.
fn drive_network_eof() -> Result<Bq26FaultObservation, String> {
    let prepared = fixture_prepared()?;
    let chunks: Vec<Vec<u8>> = vec![
        br#"data: {"id":"m1","model":"bq26-model","choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":null}]}
"#
        .to_vec(),
    ];
    let error = replay_stream_fixture(prepared, &chunks, 4_096)
        .expect_err("an EOF without a terminal event must not parse as a reply");
    if error.code != "provider_stream_incomplete" {
        return Err(format!("bq26_network_eof_unexpected_code:{}", error.code));
    }
    // A mid-stream EOF after the request was sent is an unknown side effect, not a clean failure.
    if error.side_effect_state != ModelSideEffectState::Unknown {
        return Err("bq26_network_eof_side_effect_not_unknown".to_owned());
    }
    Bq26FaultObservation::new(
        Bq26FaultCase::NetworkEof,
        Bq26FaultRefusal::new(
            "kiana_provider::replay_stream_fixture",
            "provider_stream_incomplete",
            Some("ModelReply".to_owned()),
            false,
        )?,
        0,
        0,
        0,
        true,
        false,
        "mark the attempt unknown and reconcile the reservation against the provider receipt",
    )
}

/// A minimal streaming `PreparedModelCall` for the provider framer seam. Only the
/// framer/accumulator path is exercised; no socket is involved.
fn fixture_prepared() -> Result<kiana_domain::PreparedModelCall, String> {
    use kiana_domain::{
        ModelCallSpec, ModelMessage, ModelPurpose, ModelRequest, ModelResponseFormat, ModelRoute,
        TokenBudget, MODEL_CALL_SCHEMA,
    };

    let request = ModelRequest {
        messages: vec![ModelMessage::user("bq26 offline fault fixture")],
        tools: Vec::new(),
        sandbox: "read-only".to_owned(),
    };
    let mut prepared = kiana_domain::PreparedModelCall {
        schema: MODEL_CALL_SCHEMA.to_owned(),
        spec: ModelCallSpec {
            call_id: RequestId::new(),
            attempt_id: RequestId::new(),
            model_attempt_id: None,
            step_id: None,
            step: 1,
            purpose: ModelPurpose::Task,
            assignment: None,
            response_format: ModelResponseFormat::Text,
            replay: Vec::new(),
            deadline_unix_ms: bq26_fixture_deadline(),
        },
        route: ModelRoute {
            provider_id: "bq26-provider".to_owned(),
            protocol: ModelProtocol::OpenAiChat,
            connection_id: "bq26-connection".to_owned(),
            model_id: "bq26-model".to_owned(),
            profile: "bq26".to_owned(),
            configuration_revision: "bq26.v1".to_owned(),
            streaming: true,
        },
        request: request.clone(),
        wire_body: serde_json::json!({"messages": []}),
        request_hash: String::new(),
        budget: TokenBudget::new(1, 0, 0, 16, 4_096),
        tool_catalog_hash: kiana_domain::tool_catalog_hash(&request.tools),
        provider_account: None,
        credential_revision: None,
    };
    prepared.seal();
    prepared.validate().map_err(|error| error.code.clone())?;
    Ok(prepared)
}

/// Keep the injected-now fixture referenced so a driver cannot silently drift onto a real clock.
#[allow(dead_code)]
fn _injected_now_is_used() -> u64 {
    bq26_fixture_now()
}

#[allow(dead_code)]
fn _attempt_id_is_used() -> AttemptId {
    AttemptId::new()
}
