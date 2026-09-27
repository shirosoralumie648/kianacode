//! PD-31 failure-first adapter conformance fixtures.
//!
//! The suite itself lives in `kiana_ports::adapter_conformance` so that Memory, JSONL, a future
//! durable-file adapter and a future SQLite adapter all run the *same* checks. These fixtures wire
//! the suite to the two adapters that exist, then pin the "先拒绝" (rejected-first) column: a
//! declaration that lies, a stub that succeeds on an unsupported capability, an undeclared refusal
//! code, a Memory row marked durable, and a reserved SQLite row pretending to be implemented.
//!
//! Nothing here opens a file, restarts a process, fsyncs a page or contacts a database. The suite
//! observes declarations and logical outcomes, so a green report here is `proof_level=source`.

use async_trait::async_trait;
use kiana_domain::{
    AggregateVersion, CommitOutcome, EventStoreCapabilities, RequestId, RuntimeEvent,
    TransitionBatch, JOURNAL_WRITER_VERSION, MAX_JOURNAL_FRAME_BYTES, MAX_TRANSITION_EVENTS,
};
use kiana_eventlog::{JsonlEventLog, MemoryEventLog};
use kiana_ports::{
    run_event_store_conformance, AdapterCapabilityMatrix, AdapterConformance, AdapterKind,
    ArtifactStorePort, BackupStorePort, ConformanceCheck, EventStorePort, MigrationRunnerPort,
    PortError, ProjectionStorePort, ProofCeiling, RetentionStorePort, UnsupportedProbe,
};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_path(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("kiana-pd31-{label}-{stamp}.jsonl"))
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = std::fs::remove_file(path.with_extension("jsonl.lock"));
}

/// The probe both shipped adapters share: neither implements a projection writer, so the
/// [`ProjectionStorePort`] default must refuse rather than apply nothing.
struct NoProjection;
#[async_trait]
impl ProjectionStorePort for NoProjection {}

async fn probe_projection() -> Result<String, PortError> {
    let store: &dyn ProjectionStorePort = &NoProjection;
    store
        .apply_events("pd31", Vec::new(), 0_u64)
        .await
        .map(|()| "projection_store_applied".to_owned())
}

/// A capability record with the journal's real bounds, for fixtures that only care about the
/// `durable_commits` bit. Derived from the same constants the adapters use.
fn capabilities_with(atomic_transitions: bool, durable_commits: bool) -> EventStoreCapabilities {
    EventStoreCapabilities {
        atomic_transitions,
        durable_commits,
        command_receipts: true,
        cursor_reads: true,
        writer_format_version: JOURNAL_WRITER_VERSION,
        max_frame_bytes: MAX_JOURNAL_FRAME_BYTES,
        max_batch_events: MAX_TRANSITION_EVENTS,
    }
}

/// The Memory row.
///
/// The numeric bounds come from `capabilities_with` rather than being written out here, so a change
/// to `MAX_JOURNAL_FRAME_BYTES` or the writer version cannot make this declaration silently wrong.
fn memory_declaration() -> AdapterConformance {
    AdapterConformance::new(
        AdapterKind::Memory,
        // The one bit the fixtures *do* assert is `durable_commits: false` — that is the property
        // the card is about, and `AdapterConformance::new` refuses to construct the row if it is
        // ever true.
        capabilities_with(true, false),
        ProofCeiling::LocalBehavior,
        UnsupportedProbe::ProjectionApply,
        vec![
            "projection_store_unsupported".to_owned(),
            "event_store_close_unsupported".to_owned(),
        ],
        vec![
            "process-local state is lost on exit; no fsync, no cross-process lock".to_owned(),
            "read_all is a full scan and is not a bounded replay path".to_owned(),
        ],
    )
    .expect("memory declaration is well formed")
}

/// The JSONL row. `durable_commits` reflects the writer it actually is, while the proof ceiling
/// still stops at `local_behavior`: writing a file is not the same as surviving a power loss, and
/// nothing in this slice ran a power-loss, torn-tail or disk-full case.
fn jsonl_declaration() -> AdapterConformance {
    AdapterConformance::new(
        AdapterKind::Jsonl,
        capabilities_with(cfg!(unix), true),
        ProofCeiling::LocalBehavior,
        UnsupportedProbe::ProjectionApply,
        vec![
            "projection_store_unsupported".to_owned(),
            "event_store_read_all_unsupported".to_owned(),
        ],
        vec![
            "single writer lock is advisory; a second process is not fenced by this suite"
                .to_owned(),
            "no power-loss, torn-tail or disk-full run happened under this suite".to_owned(),
        ],
    )
    .expect("jsonl declaration is well formed")
}

/// Whether the suite ran `check` and it passed. A missing row is a fixture bug, not a pass.
fn passed(report: &kiana_ports::AdapterConformanceReport, check: ConformanceCheck) -> bool {
    report
        .rows
        .iter()
        .find(|row| row.check == check)
        .unwrap_or_else(|| panic!("PD-31 suite did not run {}", check.as_str()))
        .is_passed()
}

/// The checks the suite refused, as their stable ids.
fn refusals(report: &kiana_ports::AdapterConformanceReport) -> Vec<&'static str> {
    report
        .rows
        .iter()
        .filter(|row| !row.is_passed())
        .map(|row| row.check.as_str())
        .collect()
}

/// The checks the suite refused, paired with the code it observed.
fn refusal_codes(report: &kiana_ports::AdapterConformanceReport) -> Vec<(&'static str, String)> {
    report
        .rows
        .iter()
        .filter(|row| !row.is_passed())
        .map(|row| (row.check.as_str(), row.observed_code.clone()))
        .collect()
}

// ---------------------------------------------------------------------------------------
// 成功：相同 contract suite 通过；能力矩阵明确 durable/local_behavior/physical 边界
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn memory_and_jsonl_pass_the_same_contract_suite() {
    let memory = MemoryEventLog::new();
    let memory_report =
        run_event_store_conformance(&memory, &memory_declaration(), &probe_projection).await;
    memory_report
        .validate()
        .expect("memory report is structurally valid");
    assert!(
        memory_report.passed(),
        "memory suite refused: {:?}",
        memory_report
            .rows
            .iter()
            .find(|row| !row.is_passed())
            .map(|row| (row.check.as_str(), row.observed_code.clone()))
    );
    assert_eq!(memory_report.rows.len(), ConformanceCheck::all().len());
    // A non-durable adapter must not be able to say it is durable.
    assert!(passed(&memory_report, ConformanceCheck::MemoryIsNotDurable));

    let path = temp_path("jsonl");
    let jsonl = JsonlEventLog::open(&path).expect("jsonl opens");
    let jsonl_report =
        run_event_store_conformance(&jsonl, &jsonl_declaration(), &probe_projection).await;
    jsonl_report
        .validate()
        .expect("jsonl report is structurally valid");
    assert!(
        jsonl_report.passed(),
        "jsonl suite refused: {:?}",
        jsonl_report
            .rows
            .iter()
            .find(|row| !row.is_passed())
            .map(|row| (row.check.as_str(), row.observed_code.clone()))
    );
    drop(jsonl);
    cleanup(&path);
}

#[tokio::test]
async fn a_shared_suite_produces_the_same_check_order_for_every_adapter() {
    // Anti-drift: the two adapters run the identical ordered check list, so a check that is added
    // for one adapter is added for both.
    let memory = MemoryEventLog::new();
    let memory_report =
        run_event_store_conformance(&memory, &memory_declaration(), &probe_projection).await;

    let path = temp_path("order");
    let jsonl = JsonlEventLog::open(&path).expect("jsonl opens");
    let jsonl_report =
        run_event_store_conformance(&jsonl, &jsonl_declaration(), &probe_projection).await;
    drop(jsonl);
    cleanup(&path);

    let memory_checks: Vec<_> = memory_report
        .rows
        .iter()
        .map(|r| r.check.as_str())
        .collect();
    let jsonl_checks: Vec<_> = jsonl_report.rows.iter().map(|r| r.check.as_str()).collect();
    assert_eq!(memory_checks, jsonl_checks);
    // Both adapters refuse the projection probe with the same declared code.
    assert_eq!(
        memory_report.refusal_codes(),
        jsonl_report.refusal_codes(),
        "adapters drifted on the refusal code"
    );
}

#[test]
fn the_capability_matrix_states_the_durable_local_behavior_physical_boundary() {
    let matrix = AdapterCapabilityMatrix::new(vec![memory_declaration(), jsonl_declaration()])
        .expect("matrix is well formed");
    matrix.validate().expect("matrix validates");
    assert!(matrix.boundaries_are_honest());
    // Only the JSONL row may be durable; the Memory row is the one the check refuses to lie about.
    assert_eq!(matrix.durable_adapters(), vec![AdapterKind::Jsonl]);
    // No row may claim durable or physical on a source-only suite.
    for declaration in &matrix.declarations {
        assert!(declaration.proof_ceiling <= ProofCeiling::LocalBehavior);
    }
}

// ---------------------------------------------------------------------------------------
// 先拒绝：adapter 宣称 unsupported 却被调用
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn a_stub_that_succeeds_on_an_unsupported_capability_is_refused() {
    /// A store whose `commit_transition` lies and whose `read_all` returns empty success.
    struct LyingStore {
        inner: MemoryEventLog,
    }
    #[async_trait]
    impl EventStorePort for LyingStore {
        fn supports_atomic_transitions(&self) -> bool {
            true
        }
        fn capabilities(&self) -> EventStoreCapabilities {
            self.inner.capabilities()
        }
        async fn commit_transition(
            &self,
            _batch: TransitionBatch,
        ) -> Result<CommitOutcome, PortError> {
            Ok(CommitOutcome::Committed {
                receipt: kiana_domain::CommandReceipt {
                    command_id: RequestId::new(),
                    command_digest: std::iter::repeat_n('a', 64).collect(),
                    commit_id: kiana_domain::EventId::new(),
                    first_cursor: 1,
                    cursor: 1,
                    event_ids: Vec::new(),
                    versions: vec![AggregateVersion::new("run", "pd31-lie", 1)],
                },
            })
        }
        async fn read_command(
            &self,
            _id: &RequestId,
        ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
            Ok(None)
        }
        async fn read_from(
            &self,
            _cursor: u64,
            _limit: usize,
        ) -> Result<kiana_domain::JournalPage, PortError> {
            Ok(kiana_domain::JournalPage {
                events: Vec::new(),
                cursor: 0,
                has_more: false,
            })
        }
        async fn append(&self, _event: RuntimeEvent) -> Result<(), PortError> {
            Ok(())
        }
        async fn read_request(&self, _id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
            Ok(Vec::new())
        }
        /// The failure this step exists for: an unsupported read reported as a clean empty page.
        async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
            Ok(Vec::new())
        }
    }

    let store = LyingStore {
        inner: MemoryEventLog::new(),
    };
    let report =
        run_event_store_conformance(&store, &memory_declaration(), &probe_projection).await;
    report.validate().expect("report is structurally valid");
    assert!(!report.passed(), "a lying store must not pass the suite");

    let first = report.first_refusal().expect("a refusal is recorded");
    assert_eq!(first.check, ConformanceCheck::FreshBatchCommits);

    // Every way of faking success is named individually, so the report does not stop at the first.
    let refused = refusals(&report);
    assert!(refused.contains(&"unsupported_read_all_is_not_empty_success"));
    assert!(refused.contains(&"cursor_page_preserves_commit_order"));
    assert!(refused.contains(&"same_command_replays"));
    assert!(refused.contains(&"stale_version_conflicts"));
    assert!(refused.contains(&"stream_read_agrees_with_commit_order"));
}

#[tokio::test]
async fn a_probe_that_succeeds_is_refused_as_an_unsupported_capability() {
    // The probe stands in for "the caller exercises a capability the adapter declared absent".
    // A probe that answers Ok is exactly the silent-success failure mode.
    async fn probe_succeeds() -> Result<String, PortError> {
        Ok("projection_store_applied".to_owned())
    }
    let memory = MemoryEventLog::new();
    let report = run_event_store_conformance(&memory, &memory_declaration(), &probe_succeeds).await;
    let refused = refusals(&report);
    assert!(refused.contains(&"unsupported_capability_refuses"));
    assert!(refused.contains(&"refusal_code_is_declared"));
}

// ---------------------------------------------------------------------------------------
// 先拒绝：Memory 被误标 durable
// ---------------------------------------------------------------------------------------

#[test]
fn a_memory_row_marked_durable_is_rejected_at_declaration_time() {
    let error = AdapterConformance::new(
        AdapterKind::Memory,
        capabilities_with(true, true),
        ProofCeiling::LocalBehavior,
        UnsupportedProbe::ProjectionApply,
        vec!["projection_store_unsupported".to_owned()],
        Vec::new(),
    )
    .expect_err("a memory row must not be constructible as durable");
    assert_eq!(error, "adapter_declaration_memory_cannot_be_durable");
}

#[test]
fn a_matrix_whose_memory_row_claims_durability_is_rejected() {
    let mut memory = memory_declaration();
    memory.capabilities.durable_commits = true;
    memory.declaration_digest = memory.digest();
    let error = AdapterCapabilityMatrix::new(vec![memory, jsonl_declaration()])
        .expect_err("the matrix must refuse a durable memory row");
    assert_eq!(error, "adapter_declaration_memory_cannot_be_durable");
}

#[test]
fn a_declaration_whose_ceiling_outranks_the_suite_is_rejected() {
    // The suite only observes declarations and logical outcomes. It cannot certify durable.
    let error = AdapterConformance::new(
        AdapterKind::Jsonl,
        capabilities_with(true, true),
        ProofCeiling::Durable,
        UnsupportedProbe::ProjectionApply,
        vec!["projection_store_unsupported".to_owned()],
        Vec::new(),
    )
    .expect_err("a source suite must not certify a durable ceiling");
    assert_eq!(error, "adapter_declaration_proof_ceiling_not_certifiable");
}

// ---------------------------------------------------------------------------------------
// 先拒绝：不同 adapter 事件顺序/错误码漂移
// ---------------------------------------------------------------------------------------

#[tokio::test]
async fn an_adapter_whose_event_order_differs_from_the_suite_is_refused() {
    /// Same capabilities, same declared codes, but a cursor page that reverses commit order.
    struct ReorderingStore {
        inner: MemoryEventLog,
    }
    #[async_trait]
    impl EventStorePort for ReorderingStore {
        fn supports_atomic_transitions(&self) -> bool {
            true
        }
        fn capabilities(&self) -> EventStoreCapabilities {
            self.inner.capabilities()
        }
        async fn commit_transition(
            &self,
            batch: TransitionBatch,
        ) -> Result<CommitOutcome, PortError> {
            self.inner.commit_transition(batch).await
        }
        async fn read_command(
            &self,
            id: &RequestId,
        ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
            self.inner.read_command(id).await
        }
        async fn read_from(
            &self,
            cursor: u64,
            limit: usize,
        ) -> Result<kiana_domain::JournalPage, PortError> {
            let mut page = self.inner.read_from(cursor, limit).await?;
            page.events.reverse();
            Ok(page)
        }
        async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
            self.inner.append(event).await
        }
        async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_request(id).await
        }
        async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_all().await
        }
        async fn read_stream(
            &self,
            aggregate_type: &str,
            aggregate_id: &str,
        ) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_stream(aggregate_type, aggregate_id).await
        }
    }

    let store = ReorderingStore {
        inner: MemoryEventLog::new(),
    };
    let report =
        run_event_store_conformance(&store, &memory_declaration(), &probe_projection).await;
    let refused = refusal_codes(&report);
    assert!(
        refused.iter().any(
            |(check, code)| *check == "cursor_page_preserves_commit_order"
                && code == "adapter_cursor_page_order_invalid"
        ),
        "an order drift must be named: {refused:?}"
    );
}

#[tokio::test]
async fn an_adapter_whose_refusal_code_was_never_declared_is_refused() {
    // Two adapters can drift into ad-hoc strings. The suite compares the observed refusal code
    // against the adapter's own declared set, so an undeclared code is a failure even when the
    // call did refuse.
    async fn probe_undeclared_code() -> Result<String, PortError> {
        Err(PortError::Unavailable("projection_store_todo".to_owned()))
    }
    let memory = MemoryEventLog::new();
    let report =
        run_event_store_conformance(&memory, &memory_declaration(), &probe_undeclared_code).await;
    let refused = refusal_codes(&report);
    assert!(
        refused
            .iter()
            .any(|(check, code)| *check == "refusal_code_is_declared"
                && code == "adapter_refusal_code_undeclared_projection_store_todo"),
        "an undeclared refusal code must be named: {refused:?}"
    );
}

#[test]
fn a_declaration_whose_probe_code_is_missing_from_its_own_list_is_rejected() {
    let error = AdapterConformance::new(
        AdapterKind::Memory,
        capabilities_with(true, false),
        ProofCeiling::LocalBehavior,
        UnsupportedProbe::ProjectionApply,
        vec!["event_store_close_unsupported".to_owned()],
        Vec::new(),
    )
    .expect_err("a declaration must list the code it expects its own probe to return");
    assert_eq!(error, "adapter_declaration_probe_code_not_declared");
}

#[tokio::test]
async fn a_declaration_whose_capabilities_drift_from_the_adapter_is_refused() {
    // The declaration is the thing under test. An adapter that quietly adds a capability the
    // declaration never claimed is refused at the first check, before any behaviour is observed.
    struct UpgradedStore {
        inner: MemoryEventLog,
    }
    #[async_trait]
    impl EventStorePort for UpgradedStore {
        fn supports_atomic_transitions(&self) -> bool {
            true
        }
        fn capabilities(&self) -> EventStoreCapabilities {
            let mut caps = self.inner.capabilities();
            caps.durable_commits = true;
            caps
        }
        async fn commit_transition(
            &self,
            batch: TransitionBatch,
        ) -> Result<CommitOutcome, PortError> {
            self.inner.commit_transition(batch).await
        }
        async fn read_command(
            &self,
            id: &RequestId,
        ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
            self.inner.read_command(id).await
        }
        async fn read_from(
            &self,
            cursor: u64,
            limit: usize,
        ) -> Result<kiana_domain::JournalPage, PortError> {
            self.inner.read_from(cursor, limit).await
        }
        async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
            self.inner.append(event).await
        }
        async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_request(id).await
        }
        async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_all().await
        }
    }

    let store = UpgradedStore {
        inner: MemoryEventLog::new(),
    };
    let report =
        run_event_store_conformance(&store, &memory_declaration(), &probe_projection).await;
    let first = report.first_refusal().expect("a refusal is recorded");
    assert_eq!(first.check, ConformanceCheck::DeclarationMatchesAdapter);
    assert_eq!(
        first.observed_code,
        "adapter_declaration_capabilities_mismatch"
    );
}

#[tokio::test]
async fn an_adapter_whose_atomic_flag_disagrees_with_its_capabilities_is_refused() {
    struct SplitFlagStore {
        inner: MemoryEventLog,
    }
    #[async_trait]
    impl EventStorePort for SplitFlagStore {
        // Advertises atomic transitions on the flag while the capability record says no.
        fn supports_atomic_transitions(&self) -> bool {
            true
        }
        fn capabilities(&self) -> EventStoreCapabilities {
            let mut caps = self.inner.capabilities();
            caps.atomic_transitions = false;
            caps
        }
        async fn commit_transition(
            &self,
            batch: TransitionBatch,
        ) -> Result<CommitOutcome, PortError> {
            self.inner.commit_transition(batch).await
        }
        async fn read_command(
            &self,
            id: &RequestId,
        ) -> Result<Option<kiana_domain::CommandReceipt>, PortError> {
            self.inner.read_command(id).await
        }
        async fn read_from(
            &self,
            cursor: u64,
            limit: usize,
        ) -> Result<kiana_domain::JournalPage, PortError> {
            self.inner.read_from(cursor, limit).await
        }
        async fn append(&self, event: RuntimeEvent) -> Result<(), PortError> {
            self.inner.append(event).await
        }
        async fn read_request(&self, id: &RequestId) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_request(id).await
        }
        async fn read_all(&self) -> Result<Vec<RuntimeEvent>, PortError> {
            self.inner.read_all().await
        }
    }

    let store = SplitFlagStore {
        inner: MemoryEventLog::new(),
    };
    let report =
        run_event_store_conformance(&store, &memory_declaration(), &probe_projection).await;
    let refused = refusals(&report);
    assert!(refused.contains(&"declaration_matches_adapter"));
    assert!(refused.contains(&"atomic_flag_agrees_with_capabilities"));
}

// ---------------------------------------------------------------------------------------
// 先拒绝：未来 SQLite/durable 行的实现声明
// ---------------------------------------------------------------------------------------

#[test]
fn a_reserved_sqlite_row_may_not_claim_to_be_implemented() {
    // There is no SQLite adapter in this checkout. A reserved row that claims a ceiling above
    // `source` is presenting a roadmap slot as a shipped adapter.
    let error = AdapterConformance::new(
        AdapterKind::Sqlite,
        capabilities_with(true, true),
        ProofCeiling::LocalBehavior,
        UnsupportedProbe::ProjectionApply,
        vec!["projection_store_unsupported".to_owned()],
        Vec::new(),
    )
    .expect_err("a reserved kind must not be constructible above source");
    assert_eq!(error, "adapter_declaration_proof_ceiling_not_certifiable");
}

#[test]
fn a_reserved_durable_file_row_must_stay_at_source() {
    let reserved = AdapterConformance::new(
        AdapterKind::DurableFile,
        capabilities_with(true, true),
        ProofCeiling::Source,
        UnsupportedProbe::ProjectionApply,
        vec!["projection_store_unsupported".to_owned()],
        vec!["reserved slot; no durable-file adapter ships in this checkout".to_owned()],
    )
    .expect("a reserved row at source is legitimate");
    assert_eq!(reserved.proof_ceiling, ProofCeiling::Source);

    // The matrix must still name it honestly rather than dropping it.
    let matrix = AdapterCapabilityMatrix::new(vec![memory_declaration(), reserved])
        .expect("matrix with a reserved row is well formed");
    matrix.validate().expect("matrix validates");
    assert!(matrix.boundaries_are_honest());
}

#[test]
fn the_matrix_requires_a_memory_row_and_rejects_duplicate_adapters() {
    let error = AdapterCapabilityMatrix::new(vec![jsonl_declaration()])
        .expect_err("a matrix without a memory row cannot state the durable boundary");
    assert_eq!(error, "adapter_capability_matrix_memory_row_missing");

    let error = AdapterCapabilityMatrix::new(vec![memory_declaration(), memory_declaration()])
        .expect_err("one row per adapter");
    assert_eq!(error, "adapter_capability_matrix_adapter_duplicate");
}

// ---------------------------------------------------------------------------------------
// The storage ports an event-store adapter does not implement must also refuse.
// ---------------------------------------------------------------------------------------

/// A port set that implements nothing beyond the defaults. Every method must refuse.
struct NoStoragePorts;
impl ProjectionStorePort for NoStoragePorts {}
impl ArtifactStorePort for NoStoragePorts {}
impl BackupStorePort for NoStoragePorts {}
impl MigrationRunnerPort for NoStoragePorts {}
impl RetentionStorePort for NoStoragePorts {}

#[tokio::test]
async fn unimplemented_storage_ports_refuse_rather_than_return_empty_success() {
    use kiana_domain::{
        ArtifactId, ArtifactProvenance, ArtifactVersion, DeleteRequest, DeletionManifest,
        DeletionPropagation, DeletionTombstone, StoreIdentityId,
    };
    use std::collections::BTreeSet;

    const SCOPE: &str = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let store: &dyn ProjectionStorePort = &NoStoragePorts;
    let artifacts: &dyn ArtifactStorePort = &NoStoragePorts;
    let backups: &dyn BackupStorePort = &NoStoragePorts;
    let migrations: &dyn MigrationRunnerPort = &NoStoragePorts;
    let retention: &dyn RetentionStorePort = &NoStoragePorts;
    let store_id = StoreIdentityId::new();
    let cursor: u64 = 1;

    assert_eq!(
        store
            .apply_events("pd31", Vec::new(), cursor)
            .await
            .unwrap_err(),
        PortError::Unavailable("projection_store_unsupported".to_owned())
    );
    assert_eq!(
        artifacts
            .stage_artifact(
                ArtifactVersion::new(
                    ArtifactId::new(),
                    1,
                    "text/plain",
                    b"pd31",
                    SCOPE,
                    ArtifactProvenance {
                        producer_kind: "test".to_owned(),
                        producer_id: "pd31".to_owned(),
                        source_event_id: None,
                        source_run_id: None,
                        recorded_by: "ci".to_owned(),
                    },
                    1,
                )
                .expect("version is well formed"),
                b"pd31".to_vec(),
            )
            .await
            .unwrap_err(),
        PortError::Unavailable("artifact_store_unsupported".to_owned())
    );
    assert_eq!(
        backups.create_backup(store_id, cursor).await.unwrap_err(),
        PortError::Unavailable("backup_store_unsupported".to_owned())
    );
    assert_eq!(
        migrations.apply(1, 2, SCOPE).await.unwrap_err(),
        PortError::Unavailable("migration_runner_unsupported".to_owned())
    );
    assert_eq!(
        retention.purge_tombstoned(store_id, 1).await.unwrap_err(),
        PortError::Unavailable("retention_store_unsupported".to_owned())
    );

    // The typed deletion boundary refuses with its own code, not the legacy object/reason one,
    // so a caller cannot mistake "typed deletion unsupported" for "nothing to delete".
    let request = DeleteRequest::new(
        RequestId::new(),
        "/repo",
        BTreeSet::from(["src/input.txt".to_owned()]),
        "delete",
        "data_subject_request",
        "principal:operator",
        7,
        3,
        5,
    )
    .expect("request is well formed");
    let tombstone = DeletionTombstone::new(
        &request,
        "src/input.txt",
        format!("sha256:{}", "a".repeat(64)),
        4,
    )
    .expect("tombstone is well formed");
    let manifest = DeletionManifest::new(
        &request,
        4,
        std::slice::from_ref(&tombstone),
        vec![DeletionPropagation::unknown(
            "external",
            "adapter_receipt_required",
        )],
    )
    .expect("manifest is well formed");
    assert_eq!(
        retention
            .append_deletion_tombstone(store_id, tombstone)
            .await
            .unwrap_err(),
        PortError::Unavailable("typed_deletion_tombstone_unsupported".to_owned())
    );
    assert_eq!(
        retention
            .append_deletion_manifest(store_id, manifest)
            .await
            .unwrap_err(),
        PortError::Unavailable("deletion_manifest_unsupported".to_owned())
    );
}
