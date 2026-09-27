//! SC-32 audit projector: a deletable, rebuildable cache of audit facts.
//!
//! The EventLog is the fact source. This module folds committed `RuntimeEvent`s into a bounded
//! `AuditProjectionSnapshot` and keeps one extra thing the domain snapshot does not carry: the
//! identity digest of the source events that produced it, which is what makes "rebuild from
//! scratch landed on the same answer" checkable rather than merely asserted.
//!
//! The three properties this slice exists to enforce are all about a cache not lying:
//!
//! - **It cannot cover a fact.** A tail may only add records and source identities. Dropping,
//!   rewriting or re-folding an already projected source event is refused, and the append path
//!   re-checks every previous record digest after folding instead of trusting the reducer.
//! - **It cannot skip a cursor.** A tail must begin at exactly `source_cursor + 1`. A gap, a
//!   regression and a replay are three distinct refusals, because they say three different things
//!   about what the projector believes it has seen.
//! - **It cannot present a stale view as fresh.** [`AuditProjectorFreshness`] binds a candidate
//!   view to the published generation, cursor and state digest, and only an exact match is fresh.
//!
//! This is a read-only fold over events a caller already committed. It appends nothing, writes no
//! file, opens no store and calls no port, and it cannot tell a truthful projector from a lying
//! one: it proves that the two replay paths agree about history, not that the history is right.

use kiana_domain::{
    json_digest, redact_text, scan_secret_sentinels, AuditProjectionSnapshot, AuditRecord,
    EventCursor, EventId, ProjectionLagStatus, ProjectionLagView, RuntimeEvent, SchemaVersion,
    SecretScanChannel, MAX_SOURCE_EVENT_IDS,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

use crate::data_boundary::{QueryDataBoundary, QueryDataDisposition};

pub const AUDIT_PROJECTOR_FOLD_SCHEMA: &str = "kiana.audit-projector-fold.v1";
pub const AUDIT_PROJECTOR_FRESHNESS_SCHEMA: &str = "kiana.audit-projector-freshness.v1";
pub const AUDIT_PROJECTOR_POSITION_SCHEMA: &str = "kiana.audit-projector-position.v1";
pub const AUDIT_PROJECTOR_QUERY_SCHEMA: &str = "kiana.audit-projector-query.v1";
pub const AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA: &str = "kiana.audit-projector-authorization.v1";
pub const AUDIT_PROJECTOR_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_AUDIT_PROJECTOR_TEXT: usize = 256;
/// One page is bounded so a read cannot be turned into an unbounded walk of the whole log.
pub const MAX_AUDIT_PROJECTOR_QUERY_PAGE: usize = 1_000;
/// The stable denial codes this module can produce, in decision order.
pub const AUDIT_PROJECTOR_DENY_CODES: [&str; 6] = [
    "audit_projector_query_project_mismatch",
    "audit_projector_query_scope_digest_mismatch",
    "audit_projector_query_authority_digest_mismatch",
    "audit_projector_query_boundary_denied",
    "audit_projector_query_after_cursor_ahead",
    "audit_projector_view_generation_stale",
];

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuditProjectorError {
    #[error("audit_projector_header_invalid")]
    HeaderInvalid,
    #[error("audit_projector_source_empty")]
    SourceEmpty,
    #[error("audit_projector_source_event_limit")]
    SourceEventLimit,
    #[error("audit_projector_source_event_duplicate")]
    SourceEventDuplicate,
    #[error("audit_projector_cursor_required")]
    CursorRequired,
    #[error("audit_projector_cursor_regression")]
    CursorRegression,
    #[error("audit_projector_cursor_gap")]
    CursorGap,
    #[error("audit_projector_projection_event_replay")]
    ProjectionEventReplay,
    #[error("audit_projector_projection_not_append_only")]
    ProjectionNotAppendOnly,
    #[error("audit_projector_projection_dropped_record")]
    ProjectionDroppedRecord,
    #[error("audit_projector_projection_rewrote_record")]
    ProjectionRewroteRecord,
    #[error("audit_projector_projection_source_rewritten")]
    ProjectionSourceRewritten,
    #[error("audit_projector_reduce_failed:{0}")]
    ReduceFailed(String),
    #[error("audit_projector_snapshot_invalid:{0}")]
    SnapshotInvalid(String),
    #[error("audit_projector_text_invalid:{0}")]
    TextInvalid(String),
    #[error("audit_projector_authorization_denied:{0}")]
    AuthorizationDenied(String),
}

/// Digest over the sorted, deduplicated source identities a projection consumed.
///
/// Both replay paths compute it the same way, so an incremental replay and a from-scratch rebuild
/// over the same committed facts produce the same value without either path trusting the other.
pub fn audit_fold_source_event_digest(source_event_ids: &[EventId]) -> String {
    let sorted = source_event_ids
        .iter()
        .map(ToString::to_string)
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect::<Vec<_>>();
    json_digest(&json!({
        "schema": AUDIT_PROJECTOR_FOLD_SCHEMA,
        "source_event_ids": sorted,
    }))
}

/// One folded audit projection plus the identity of the source facts it came from.
///
/// `generation` counts publishes of this projection, starting at 1 for the first fold. It is the
/// CAS token `kiana-core::audit_projection_commit` checks, so the two crates share one counter
/// rather than each keeping a private one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectorFold {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    pub generation: u64,
    pub first_source_cursor: EventCursor,
    pub snapshot: AuditProjectionSnapshot,
    pub source_event_digest: String,
    pub fold_digest: String,
}

impl AuditProjectorFold {
    fn seal(
        projector: &str,
        generation: u64,
        first_source_cursor: EventCursor,
        snapshot: AuditProjectionSnapshot,
    ) -> Result<Self, AuditProjectorError> {
        let mut fold = Self {
            schema: AUDIT_PROJECTOR_FOLD_SCHEMA.to_owned(),
            version: AUDIT_PROJECTOR_VERSION,
            projector: projector.to_owned(),
            generation,
            first_source_cursor,
            source_event_digest: audit_fold_source_event_digest(&snapshot.source_event_ids),
            snapshot,
            fold_digest: String::new(),
        };
        fold.fold_digest = fold.digest();
        fold.validate()?;
        Ok(fold)
    }

    pub fn validate(&self) -> Result<(), AuditProjectorError> {
        if self.schema != AUDIT_PROJECTOR_FOLD_SCHEMA
            || !self.version.is_compatible_with(&AUDIT_PROJECTOR_VERSION)
            || self.generation == 0
            || self.first_source_cursor == 0
        {
            return Err(AuditProjectorError::HeaderInvalid);
        }
        safe_text(&self.projector, "audit_projector_name")
            .map_err(AuditProjectorError::TextInvalid)?;
        self.snapshot
            .validate()
            .map_err(AuditProjectorError::SnapshotInvalid)?;
        if self.snapshot.source_cursor < self.first_source_cursor {
            return Err(AuditProjectorError::CursorRegression);
        }
        if self.source_event_digest
            != audit_fold_source_event_digest(&self.snapshot.source_event_ids)
        {
            return Err(AuditProjectorError::ProjectionSourceRewritten);
        }
        if self.fold_digest != self.digest() {
            return Err(AuditProjectorError::SnapshotInvalid(
                "audit_projector_fold_digest_mismatch".to_owned(),
            ));
        }
        Ok(())
    }

    /// The digest a compare-and-swap claim must carry as the new state.
    pub fn state_digest(&self) -> String {
        self.snapshot.projection_digest.clone()
    }

    pub fn source_cursor(&self) -> EventCursor {
        self.snapshot.source_cursor
    }

    pub fn records(&self) -> &[AuditRecord] {
        &self.snapshot.records
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "generation": self.generation,
            "first_source_cursor": self.first_source_cursor,
            "snapshot": self.snapshot,
            "source_event_digest": self.source_event_digest,
        }))
    }
}

/// Fold a committed event slice into a fresh audit projection at generation 1.
///
/// This is the from-scratch path, and it is deliberately the *only* way a projection comes into
/// being: a projector holding no checkpoint replays the log, and the incremental path below has
/// to land on the same digest as this one.
pub fn project_audit_from_scratch(
    projector: &str,
    events: &[RuntimeEvent],
    first_source_cursor: EventCursor,
) -> Result<AuditProjectorFold, AuditProjectorError> {
    safe_text(projector, "audit_projector_name").map_err(AuditProjectorError::TextInvalid)?;
    if first_source_cursor == 0 {
        return Err(AuditProjectorError::CursorRequired);
    }
    let source_event_ids = collect_source_event_ids(events)?;
    let records = kiana_domain::reduce_audit_records(events, first_source_cursor)
        .map_err(AuditProjectorError::ReduceFailed)?;
    let last_cursor = last_cursor_of(first_source_cursor, events.len())?;
    let snapshot =
        AuditProjectionSnapshot::new(1, last_cursor, source_event_ids, records, Vec::new())
            .map_err(AuditProjectorError::SnapshotInvalid)?;
    AuditProjectorFold::seal(projector, 1, first_source_cursor, snapshot)
}

/// Apply one contiguous tail to an existing projection.
///
/// The tail must begin at exactly `source_cursor + 1`, must not re-state a source event the
/// projection already consumed, and must leave every previously folded record byte-identical. A
/// caller that reaches for this with a stale checkpoint therefore gets a refusal rather than a
/// second history.
pub fn append_audit_tail(
    fold: &AuditProjectorFold,
    tail: &[RuntimeEvent],
    tail_first_cursor: EventCursor,
) -> Result<AuditProjectorFold, AuditProjectorError> {
    fold.validate()?;
    if fold.source_cursor() == u64::MAX {
        return Err(AuditProjectorError::CursorRequired);
    }
    if tail_first_cursor < fold.source_cursor() {
        return Err(AuditProjectorError::CursorRegression);
    }
    if tail_first_cursor != fold.source_cursor().saturating_add(1) {
        return Err(AuditProjectorError::CursorGap);
    }
    let tail_ids = collect_source_event_ids(tail)?;
    let already = fold
        .snapshot
        .source_event_ids
        .iter()
        .map(ToString::to_string)
        .collect::<BTreeSet<_>>();
    if tail_ids
        .iter()
        .any(|event_id| already.contains(&event_id.to_string()))
    {
        return Err(AuditProjectorError::ProjectionEventReplay);
    }
    let appended = kiana_domain::reduce_audit_records(tail, tail_first_cursor)
        .map_err(AuditProjectorError::ReduceFailed)?;
    let last_cursor = last_cursor_of(tail_first_cursor, tail.len())?;
    let mut source_event_ids = fold.snapshot.source_event_ids.clone();
    source_event_ids.extend(tail_ids);
    if source_event_ids.len() > MAX_SOURCE_EVENT_IDS {
        return Err(AuditProjectorError::SourceEventLimit);
    }
    let mut records = fold.snapshot.records.clone();
    records.extend(appended);
    // The append-only check runs after the reducer, not before: the reducer is the thing that
    // could have dropped or rewritten history, so its output is what gets compared.
    assert_append_only(fold, &records, &source_event_ids)?;
    let generation = fold
        .generation
        .checked_add(1)
        .ok_or(AuditProjectorError::CursorRequired)?;
    let snapshot = AuditProjectionSnapshot::new(
        generation,
        last_cursor,
        source_event_ids,
        records,
        fold.snapshot.limitations.clone(),
    )
    .map_err(AuditProjectorError::SnapshotInvalid)?;
    AuditProjectorFold::seal(
        &fold.projector,
        generation,
        fold.first_source_cursor,
        snapshot,
    )
}

/// The append-only rule behind [`append_audit_tail`], exposed so it can be stated and checked on
/// its own.
///
/// A projection is a cache, and a cache that can move backwards is worse than no cache: it
/// un-answers a question a caller already asked. Three shapes are refused, and they are distinct
/// because they mean different things. Fewer rows than the previous projection is a cache that
/// dropped an answer (`audit_projector_projection_not_append_only`). A source identity list that is
/// not the previous one plus more is a cache whose *history* was rewritten, which is how a deleted
/// or superseded fact comes back (`audit_projector_projection_source_rewritten`). An `audit_id`
/// that is gone, or one whose digest changed under the same identity, is a cache that edited an
/// answer already given (`..._dropped_record`, `..._rewrote_record`).
pub fn audit_projection_is_append_only(
    previous: &AuditProjectorFold,
    records: &[AuditRecord],
    source_event_ids: &[EventId],
) -> Result<(), AuditProjectorError> {
    previous.validate()?;
    for record in records {
        record
            .validate()
            .map_err(AuditProjectorError::SnapshotInvalid)?;
    }
    if records.len() < previous.snapshot.records.len() {
        return Err(AuditProjectorError::ProjectionNotAppendOnly);
    }
    if !source_event_ids.starts_with(&previous.snapshot.source_event_ids) {
        return Err(AuditProjectorError::ProjectionSourceRewritten);
    }
    let before = previous
        .snapshot
        .records
        .iter()
        .map(|record| (record.audit_id.clone(), record.record_digest.clone()))
        .collect::<BTreeMap<_, _>>();
    let after = records
        .iter()
        .map(|record| (record.audit_id.clone(), record.record_digest.clone()))
        .collect::<BTreeMap<_, _>>();
    for (audit_id, digest) in &before {
        match after.get(audit_id) {
            None => return Err(AuditProjectorError::ProjectionDroppedRecord),
            Some(current) if current != digest => {
                return Err(AuditProjectorError::ProjectionRewroteRecord)
            }
            Some(_) => {}
        }
    }
    Ok(())
}

fn assert_append_only(
    previous: &AuditProjectorFold,
    records: &[AuditRecord],
    source_event_ids: &[EventId],
) -> Result<(), AuditProjectorError> {
    audit_projection_is_append_only(previous, records, source_event_ids)
}

fn last_cursor_of(
    first_cursor: EventCursor,
    len: usize,
) -> Result<EventCursor, AuditProjectorError> {
    if len == 0 {
        return Err(AuditProjectorError::SourceEmpty);
    }
    first_cursor
        .checked_add(len as u64)
        .and_then(|cursor| cursor.checked_sub(1))
        .ok_or(AuditProjectorError::CursorRequired)
}

fn collect_source_event_ids(events: &[RuntimeEvent]) -> Result<Vec<EventId>, AuditProjectorError> {
    if events.is_empty() {
        return Err(AuditProjectorError::SourceEmpty);
    }
    if events.len() > MAX_SOURCE_EVENT_IDS {
        return Err(AuditProjectorError::SourceEventLimit);
    }
    let mut seen = BTreeSet::new();
    let mut ids = Vec::with_capacity(events.len());
    for event in events {
        if event.event_id.as_uuid().is_nil() {
            return Err(AuditProjectorError::HeaderInvalid);
        }
        if !seen.insert(event.event_id.to_string()) {
            return Err(AuditProjectorError::SourceEventDuplicate);
        }
        ids.push(event.event_id);
    }
    Ok(ids)
}

/// Where the published head of an audit projection currently stands.
///
/// Four scalars and one digest, not a second head contract: `kiana-core`'s
/// `AuditProjectionHead` carries exactly these fields, and a caller passes them across rather than
/// defining a competing shape. It is presented here because the freshness decision needs it and
/// `kiana-query` does not depend on `kiana-core`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectionPosition {
    pub schema: String,
    pub version: SchemaVersion,
    pub generation: u64,
    pub source_cursor: EventCursor,
    pub state_digest: String,
    pub data_epoch: u64,
    pub position_digest: String,
}

impl AuditProjectionPosition {
    pub fn new(
        generation: u64,
        source_cursor: EventCursor,
        state_digest: &str,
        data_epoch: u64,
    ) -> Result<Self, String> {
        let mut position = Self {
            schema: AUDIT_PROJECTOR_POSITION_SCHEMA.to_owned(),
            version: AUDIT_PROJECTOR_VERSION,
            generation,
            source_cursor,
            state_digest: state_digest.to_owned(),
            data_epoch,
            position_digest: String::new(),
        };
        position.position_digest = position.digest();
        position.validate()?;
        Ok(position)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTOR_POSITION_SCHEMA
            || !self.version.is_compatible_with(&AUDIT_PROJECTOR_VERSION)
            || self.generation == 0
            || self.source_cursor == 0
            || self.data_epoch == 0
        {
            return Err("audit_projector_position_header_invalid".to_owned());
        }
        valid_digest(&self.state_digest, "audit_projector_position_state_digest")?;
        valid_digest(&self.position_digest, "audit_projector_position_digest")?;
        if self.position_digest != self.digest() {
            return Err("audit_projector_position_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "generation": self.generation,
            "source_cursor": self.source_cursor,
            "state_digest": self.state_digest,
            "data_epoch": self.data_epoch,
        }))
    }
}

/// Whether one candidate view may be presented as the current answer to an audit question.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectorFreshness {
    pub schema: String,
    pub version: SchemaVersion,
    pub projector: String,
    /// The domain lag view for the same facts, so a caller reads one shared vocabulary for "how far
    /// behind is this projection" instead of a SC-32-specific one.
    pub lag: ProjectionLagView,
    pub view_generation: u64,
    pub view_state_digest: String,
    pub fresh: bool,
    pub reason: String,
    pub remediation: String,
    pub view_digest: String,
}

impl AuditProjectorFreshness {
    /// Decide in a fixed order, so the reported reason is the first violated rule.
    ///
    /// Generation, then cursor, then state digest. A view that sits *ahead* of the published head
    /// is refused outright rather than labelled: an unpublished projection is not a stale one, and
    /// calling it stale would invite a caller to re-read a head that is behind it. Of the three
    /// comparable cases the order is most-specific-first - a view from an older publish needs a
    /// different fix than a view whose folded bytes drifted under an unchanged generation.
    pub fn evaluate(
        position: &AuditProjectionPosition,
        fold: &AuditProjectorFold,
    ) -> Result<Self, String> {
        let reason = derive_freshness(position, fold)?;
        let fresh = reason.is_empty();
        // The lag view carries a reason only while it is actually behind. A view that matches on
        // cursor but is stale on generation is not a lag, and mislabelling it as one would hide
        // the real problem behind a message about cursors.
        let lag = ProjectionLagView::new(
            position.source_cursor,
            Some(fold.source_cursor()),
            position.generation,
            position.data_epoch,
            (!fresh).then_some(reason.to_owned()),
        )?;
        let mut view = Self {
            schema: AUDIT_PROJECTOR_FRESHNESS_SCHEMA.to_owned(),
            version: AUDIT_PROJECTOR_VERSION,
            projector: fold.projector.clone(),
            lag,
            view_generation: fold.generation,
            view_state_digest: fold.state_digest(),
            fresh,
            reason: reason.to_owned(),
            remediation: if fresh {
                String::new()
            } else {
                "re-read the published head and rebuild from the committed source before answering"
                    .to_owned()
            },
            view_digest: String::new(),
        };
        view.view_digest = view.digest();
        view.validate_against(position, fold)?;
        Ok(view)
    }

    pub fn validate_against(
        &self,
        position: &AuditProjectionPosition,
        fold: &AuditProjectorFold,
    ) -> Result<(), String> {
        let reason = derive_freshness(position, fold)?;
        self.lag
            .validate()
            .map_err(|error| format!("audit_projector_view_lag_invalid:{error}"))?;
        if self.schema != AUDIT_PROJECTOR_FRESHNESS_SCHEMA
            || !self.version.is_compatible_with(&AUDIT_PROJECTOR_VERSION)
            || self.projector != fold.projector
            || self.view_generation != fold.generation
            || self.view_state_digest != fold.state_digest()
        {
            return Err("audit_projector_view_binding_invalid".to_owned());
        }
        // Fresh is written as a conjunction over "nothing fired" so an edited `fresh` cannot stand
        // on its own, and a caught-up lag may not carry a staleness reason.
        if self.fresh != reason.is_empty()
            || self.reason != reason
            || (self.lag.status == ProjectionLagStatus::CaughtUp && !self.reason.is_empty())
        {
            return Err("audit_projector_view_freshness_mismatch".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "audit_projector_view_reason")?;
            safe_text(&self.remediation, "audit_projector_view_remediation")?;
        } else if !self.remediation.is_empty() {
            return Err("audit_projector_view_remediation_without_reason".to_owned());
        }
        valid_digest(&self.view_state_digest, "audit_projector_view_state_digest")?;
        valid_digest(&self.view_digest, "audit_projector_view_digest")?;
        if self.view_digest != self.digest() {
            return Err("audit_projector_view_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "projector": self.projector,
            "lag": self.lag,
            "view_generation": self.view_generation,
            "view_state_digest": self.view_state_digest,
            "fresh": self.fresh,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn derive_freshness<'a>(
    position: &AuditProjectionPosition,
    fold: &'a AuditProjectorFold,
) -> Result<&'a str, String> {
    position.validate()?;
    fold.validate().map_err(|error| error.to_string())?;
    if fold.source_cursor() > position.source_cursor {
        return Err("audit_projector_view_cursor_ahead".to_owned());
    }
    if fold.generation != position.generation {
        return Ok("audit_projector_view_generation_stale");
    }
    if fold.source_cursor() != position.source_cursor {
        return Ok("audit_projector_view_cursor_mismatch");
    }
    if fold.state_digest() != position.state_digest {
        return Ok("audit_projector_view_state_drift");
    }
    Ok("")
}

/// One bounded audit read, addressed by the authority that granted the scope.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectorQuery {
    pub schema: String,
    pub version: SchemaVersion,
    pub subject_ref: String,
    pub project_root: String,
    pub scope_digest: String,
    /// Digest of the `QueryDataBoundary` that granted this scope. A caller re-asserting a scope it
    /// computed itself does not match the server-derived decision digest.
    pub authority_digest: String,
    pub after_cursor: EventCursor,
    pub limit: usize,
    pub query_digest: String,
}

impl AuditProjectorQuery {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject_ref: &str,
        project_root: &str,
        scope_digest: &str,
        authority_digest: &str,
        after_cursor: EventCursor,
        limit: usize,
    ) -> Result<Self, String> {
        let mut query = Self {
            schema: AUDIT_PROJECTOR_QUERY_SCHEMA.to_owned(),
            version: AUDIT_PROJECTOR_VERSION,
            subject_ref: subject_ref.to_owned(),
            project_root: project_root.to_owned(),
            scope_digest: scope_digest.to_owned(),
            authority_digest: authority_digest.to_owned(),
            after_cursor,
            limit,
            query_digest: String::new(),
        };
        query.query_digest = query.digest();
        query.validate()?;
        Ok(query)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != AUDIT_PROJECTOR_QUERY_SCHEMA
            || !self.version.is_compatible_with(&AUDIT_PROJECTOR_VERSION)
            || self.limit == 0
            || self.limit > MAX_AUDIT_PROJECTOR_QUERY_PAGE
        {
            return Err("audit_projector_query_header_invalid".to_owned());
        }
        safe_text(&self.subject_ref, "audit_projector_query_subject")?;
        safe_text(&self.project_root, "audit_projector_query_project")?;
        valid_digest(&self.scope_digest, "audit_projector_query_scope_digest")?;
        valid_digest(
            &self.authority_digest,
            "audit_projector_query_authority_digest",
        )?;
        valid_digest(&self.query_digest, "audit_projector_query_digest")?;
        if self.query_digest != self.digest() {
            return Err("audit_projector_query_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "subject_ref": self.subject_ref,
            "project_root": self.project_root,
            "scope_digest": self.scope_digest,
            "authority_digest": self.authority_digest,
            "after_cursor": self.after_cursor,
            "limit": self.limit,
        }))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditProjectorDisposition {
    Allowed,
    Denied,
}

impl AuditProjectorDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Denied => "denied",
        }
    }
}

/// The ordered decision for one audit read.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditProjectorAuthorization {
    pub schema: String,
    pub version: SchemaVersion,
    pub disposition: AuditProjectorDisposition,
    pub subject_ref: String,
    pub project_root: String,
    /// Digest of the projection the read was authorized against, so a granted page is pinned to
    /// the exact read model it was decided on.
    pub fold_state_digest: String,
    pub query_digest: String,
    pub authority_digest: String,
    pub reason: String,
    pub remediation: String,
    pub authorization_digest: String,
}

impl AuditProjectorAuthorization {
    /// Decide in a fixed order, so the reported reason is the first violated rule.
    ///
    /// Project, then scope digest, then the authority digest that proves the scope came from a
    /// server-derived boundary rather than from the caller, then the boundary's own disposition,
    /// then the page bound, then freshness, then the cursor. Identity comes first because a query
    /// for another project is not a stale view or an oversized page - it is a different question,
    /// and answering it out of this cache would be the scope crossing the card names. The page
    /// bound precedes freshness so an unbounded read is refused before anyone reasons about which
    /// view it would have read; freshness precedes the cursor because an answer drawn from a view
    /// that is not the published one is wrong whatever cursor it was asked for.
    pub fn evaluate(
        query: &AuditProjectorQuery,
        fold: &AuditProjectorFold,
        position: &AuditProjectionPosition,
        boundary: &QueryDataBoundary,
        freshness: &AuditProjectorFreshness,
    ) -> Result<Self, String> {
        let (denied, reason, remediation) =
            derive_access(query, fold, position, boundary, freshness)?;
        let mut authorization = Self {
            schema: AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA.to_owned(),
            version: AUDIT_PROJECTOR_VERSION,
            disposition: if denied {
                AuditProjectorDisposition::Denied
            } else {
                AuditProjectorDisposition::Allowed
            },
            subject_ref: query.subject_ref.clone(),
            project_root: query.project_root.clone(),
            fold_state_digest: fold.state_digest(),
            query_digest: query.query_digest.clone(),
            authority_digest: query.authority_digest.clone(),
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            authorization_digest: String::new(),
        };
        authorization.authorization_digest = authorization.digest();
        authorization.validate_against(query, fold, position, boundary, freshness)?;
        Ok(authorization)
    }

    pub fn validate_against(
        &self,
        query: &AuditProjectorQuery,
        fold: &AuditProjectorFold,
        position: &AuditProjectionPosition,
        boundary: &QueryDataBoundary,
        freshness: &AuditProjectorFreshness,
    ) -> Result<(), String> {
        let (denied, reason, remediation) =
            derive_access(query, fold, position, boundary, freshness)?;
        if self.schema != AUDIT_PROJECTOR_AUTHORIZATION_SCHEMA
            || !self.version.is_compatible_with(&AUDIT_PROJECTOR_VERSION)
            || self.disposition
                != if denied {
                    AuditProjectorDisposition::Denied
                } else {
                    AuditProjectorDisposition::Allowed
                }
            || self.subject_ref != query.subject_ref
            || self.project_root != query.project_root
            || self.fold_state_digest != fold.state_digest()
            || self.query_digest != query.query_digest
            || self.authority_digest != query.authority_digest
            || self.reason != reason
            || self.remediation != remediation
        {
            return Err("audit_projector_authorization_binding_invalid".to_owned());
        }
        if denied != !self.reason.is_empty() {
            return Err("audit_projector_authorization_reason_state_mismatch".to_owned());
        }
        if !self.reason.is_empty() {
            safe_text(&self.reason, "audit_projector_authorization_reason")?;
            safe_text(
                &self.remediation,
                "audit_projector_authorization_remediation",
            )?;
        } else if !self.remediation.is_empty() {
            return Err("audit_projector_authorization_remediation_without_reason".to_owned());
        }
        valid_digest(
            &self.fold_state_digest,
            "audit_projector_authorization_fold_state_digest",
        )?;
        valid_digest(
            &self.authorization_digest,
            "audit_projector_authorization_digest",
        )?;
        if self.authorization_digest != self.digest() {
            return Err("audit_projector_authorization_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn allowed(&self) -> bool {
        self.disposition == AuditProjectorDisposition::Allowed
    }

    /// Read the authorized page out of the projection.
    ///
    /// The decision is re-derived here rather than trusted, so a deserialized `Allowed`
    /// authorization cannot be used to read a projection it was never decided against.
    pub fn page(
        &self,
        query: &AuditProjectorQuery,
        fold: &AuditProjectorFold,
        position: &AuditProjectionPosition,
        boundary: &QueryDataBoundary,
        freshness: &AuditProjectorFreshness,
    ) -> Result<Vec<AuditRecord>, AuditProjectorError> {
        self.validate_against(query, fold, position, boundary, freshness)
            .map_err(AuditProjectorError::AuthorizationDenied)?;
        if !self.allowed() {
            return Err(AuditProjectorError::AuthorizationDenied(
                self.reason.clone(),
            ));
        }
        Ok(fold
            .records()
            .iter()
            .filter(|record| record.source_cursor > query.after_cursor)
            .take(query.limit)
            .cloned()
            .collect())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "disposition": self.disposition,
            "subject_ref": self.subject_ref,
            "project_root": self.project_root,
            "fold_state_digest": self.fold_state_digest,
            "query_digest": self.query_digest,
            "authority_digest": self.authority_digest,
            "reason": self.reason,
            "remediation": self.remediation,
        }))
    }
}

fn derive_access<'a>(
    query: &AuditProjectorQuery,
    fold: &'a AuditProjectorFold,
    position: &'a AuditProjectionPosition,
    boundary: &'a QueryDataBoundary,
    freshness: &'a AuditProjectorFreshness,
) -> Result<(bool, &'a str, &'a str), String> {
    query.validate()?;
    fold.validate().map_err(|error| error.to_string())?;
    boundary
        .validate()
        .map_err(|error| format!("audit_projector_boundary_invalid:{error}"))?;
    freshness
        .validate_against(position, fold)
        .map_err(|error| format!("audit_projector_view_invalid:{error}"))?;
    if boundary.project_root != query.project_root {
        return Ok((
            true,
            "audit_projector_query_project_mismatch",
            "query the projection that was folded for this project root",
        ));
    }
    if boundary.scope_digest != query.scope_digest {
        return Ok((
            true,
            "audit_projector_query_scope_digest_mismatch",
            "present the scope digest the server-derived boundary carries",
        ));
    }
    if query.authority_digest != boundary.decision_digest {
        return Ok((
            true,
            "audit_projector_query_authority_digest_mismatch",
            "a self-asserted scope is not an authority; bind the query to the boundary decision digest",
        ));
    }
    if boundary.cache != QueryDataDisposition::Allowed {
        return Ok((
            true,
            "audit_projector_query_boundary_denied",
            "the derived read model is withheld at this data epoch; no page may be read from it",
        ));
    }
    if !freshness.fresh {
        return Ok((
            true,
            freshness.reason.as_str(),
            freshness.remediation.as_str(),
        ));
    }
    if query.after_cursor > fold.source_cursor() {
        return Ok((
            true,
            "audit_projector_query_after_cursor_ahead",
            "the requested cursor is past the published projection; there is nothing after it",
        ));
    }
    Ok((false, "", ""))
}

fn safe_text(value: &str, field: &str) -> Result<(), String> {
    if value.trim().is_empty()
        || value.len() > MAX_AUDIT_PROJECTOR_TEXT
        || value.contains(['\0', '\r', '\n'])
    {
        return Err(format!("{field}_invalid"));
    }
    if redact_text(value) != value {
        return Err(format!("{field}_not_redacted"));
    }
    scan_secret_sentinels(SecretScanChannel::Receipt, value)
        .map_err(|_| format!("{field}_secret_detected"))
}

fn valid_digest(value: &str, field: &str) -> Result<(), String> {
    let Some(hex) = value.strip_prefix("sha256:") else {
        return Err(format!("{field}_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}
