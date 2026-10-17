//! Server-derived correlation, trace and causation references.
//!
//! A correlation context is an observation aid, not an authority token.  The control plane must
//! construct it only after authenticating a [`RequestContext`] and resolving the current scope and
//! epochs.  External W3C trace context is parsed for linking only; it never supplies actor,
//! project, session, grant or authority data.

use crate::{
    check_schema_compatibility, EventId, ExecutionId, InvocationId, OrganizationId, ProjectId,
    RequestContext, RequestId, RunId, SchemaVersion, SessionId, TurnId,
};
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

pub const CORRELATION_CONTEXT_SCHEMA: &str = "kiana.correlation-context.v1";
pub const CORRELATION_CONTEXT_SCHEMA_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_SPAN_LINKS: usize = 16;
pub const MAX_TRACEPARENT_BYTES: usize = 512;

fn hex_from_uuid(uuid: Uuid) -> String {
    uuid.as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn validate_hex(value: &str, expected_len: usize, field: &str) -> Result<(), String> {
    if value.len() != expected_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || value.bytes().all(|byte| byte == b'0')
    {
        return Err(format!("{field}_invalid"));
    }
    Ok(())
}

/// W3C-compatible 16-byte trace identifier.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct TraceId(String);

impl TraceId {
    pub fn new() -> Self {
        Self(hex_from_uuid(Uuid::new_v4()))
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        validate_hex(value, 32, "trace_id")?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for TraceId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for TraceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("TraceId").field(&self.0).finish()
    }
}

impl fmt::Display for TraceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for TraceId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for TraceId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

/// W3C-compatible 8-byte span identifier.
#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct SpanId(String);

impl SpanId {
    pub fn new() -> Self {
        let uuid = Uuid::new_v4();
        let mut value = String::with_capacity(16);
        for byte in &uuid.as_bytes()[..8] {
            value.push_str(&format!("{byte:02x}"));
        }
        Self(value)
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        validate_hex(value, 16, "span_id")?;
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SpanId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for SpanId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("SpanId").field(&self.0).finish()
    }
}

impl fmt::Display for SpanId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for SpanId {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for SpanId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceRef {
    pub trace_id: TraceId,
}

impl TraceRef {
    pub fn new() -> Self {
        Self {
            trace_id: TraceId::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        TraceId::parse(self.trace_id.as_str()).map(|_| ())
    }
}

impl Default for TraceRef {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanRef {
    pub trace_id: TraceId,
    pub span_id: SpanId,
}

impl SpanRef {
    pub fn new(trace_id: TraceId) -> Self {
        Self {
            trace_id,
            span_id: SpanId::new(),
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        TraceId::parse(self.trace_id.as_str())?;
        SpanId::parse(self.span_id.as_str()).map(|_| ())
    }
}

/// A validated W3C `traceparent` supplied by an external boundary.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TraceParent {
    pub version: u8,
    pub trace_id: TraceId,
    pub parent_span_id: SpanId,
    pub trace_flags: u8,
}

impl TraceParent {
    pub fn parse(value: &str) -> Result<Self, String> {
        if value.len() > MAX_TRACEPARENT_BYTES || value.trim() != value {
            return Err("traceparent_invalid".to_owned());
        }
        if value != value.to_ascii_lowercase() {
            return Err("traceparent_non_canonical".to_owned());
        }
        let parts = value.split('-').collect::<Vec<_>>();
        if parts.len() < 4 || (parts[0].len() != 2) || (parts[3].len() != 2) {
            return Err("traceparent_invalid".to_owned());
        }
        if parts[0].bytes().any(|byte| !byte.is_ascii_hexdigit())
            || parts[3].bytes().any(|byte| !byte.is_ascii_hexdigit())
        {
            return Err("traceparent_invalid".to_owned());
        }
        let version = u8::from_str_radix(parts[0], 16).map_err(|_| "traceparent_invalid")?;
        if version == 0xff || (version == 0 && parts.len() != 4) {
            return Err("traceparent_invalid".to_owned());
        }
        let trace_id = TraceId::parse(parts[1])?;
        let parent_span_id = SpanId::parse(parts[2])?;
        let trace_flags = u8::from_str_radix(parts[3], 16).map_err(|_| "traceparent_invalid")?;
        if parts[1].bytes().any(|byte| byte.is_ascii_uppercase())
            || parts[2].bytes().any(|byte| byte.is_ascii_uppercase())
        {
            return Err("traceparent_non_canonical".to_owned());
        }
        if parts[4..].iter().any(|extension| {
            extension.is_empty() || extension.bytes().any(|byte| !byte.is_ascii_hexdigit())
        }) {
            return Err("traceparent_extension_invalid".to_owned());
        }
        Ok(Self {
            version,
            trace_id,
            parent_span_id,
            trace_flags,
        })
    }

    pub fn to_header(&self) -> String {
        format!(
            "{:02x}-{}-{}-{:02x}",
            self.version, self.trace_id, self.parent_span_id, self.trace_flags
        )
    }

    pub fn parent_ref(&self) -> SpanRef {
        SpanRef {
            trace_id: self.trace_id.clone(),
            span_id: self.parent_span_id.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanLinkKind {
    Parent,
    Causal,
    FollowsFrom,
    ForeignParent,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpanLink {
    pub span: SpanRef,
    pub relationship: SpanLinkKind,
}

impl SpanLink {
    pub fn new(span: SpanRef, relationship: SpanLinkKind) -> Self {
        Self { span, relationship }
    }

    pub fn validate(&self) -> Result<(), String> {
        self.span.validate()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptRef {
    pub run_id: RunId,
    pub turn_id: TurnId,
    pub invocation_id: InvocationId,
    pub execution_id: ExecutionId,
    pub command_id: RequestId,
    pub attempt: u32,
}

impl AttemptRef {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        run_id: RunId,
        turn_id: TurnId,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
        command_id: RequestId,
        attempt: u32,
    ) -> Result<Self, String> {
        let reference = Self {
            run_id,
            turn_id,
            invocation_id,
            execution_id,
            command_id,
            attempt,
        };
        reference.validate()?;
        Ok(reference)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.run_id.as_uuid().is_nil()
            || self.turn_id.as_uuid().is_nil()
            || self.invocation_id.as_uuid().is_nil()
            || self.execution_id.as_uuid().is_nil()
            || self.command_id.as_uuid().is_nil()
        {
            return Err("correlation_attempt_id_invalid".to_owned());
        }
        if self.attempt == 0 {
            return Err("correlation_attempt_required".to_owned());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum CausationRef {
    Event(EventId),
    Command(RequestId),
    Attempt(AttemptRef),
}

impl CausationRef {
    pub fn validate_against(&self, context: &CorrelationContext) -> Result<(), String> {
        match self {
            Self::Event(event_id) => {
                if event_id.as_uuid().is_nil() {
                    Err("correlation_causation_event_id_invalid".to_owned())
                } else {
                    Ok(())
                }
            }
            Self::Command(command_id) => {
                if command_id.as_uuid().is_nil() {
                    return Err("correlation_causation_command_id_invalid".to_owned());
                }
                if context.command_id == Some(*command_id) {
                    Ok(())
                } else {
                    Err("correlation_causation_command_mismatch".to_owned())
                }
            }
            Self::Attempt(attempt) => context.validate_attempt(attempt),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrelationScope {
    pub organization_id: Option<OrganizationId>,
    pub project_id: Option<ProjectId>,
    pub session_id: SessionId,
}

impl CorrelationScope {
    pub fn new(
        session_id: SessionId,
        organization_id: Option<OrganizationId>,
        project_id: Option<ProjectId>,
    ) -> Self {
        Self {
            organization_id,
            project_id,
            session_id,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self
            .organization_id
            .is_some_and(|organization_id| organization_id.as_uuid().is_nil())
        {
            return Err("correlation_organization_id_invalid".to_owned());
        }
        if self
            .project_id
            .is_some_and(|project_id| project_id.as_uuid().is_nil())
        {
            return Err("correlation_project_id_invalid".to_owned());
        }
        if self.session_id.is_empty() {
            return Err("correlation_session_required".to_owned());
        }
        Ok(())
    }
}

/// Immutable context attached to a committed signal or a derived span.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CorrelationContext {
    pub schema: String,
    pub version: SchemaVersion,
    pub trace_id: TraceId,
    pub span_id: SpanId,
    #[serde(default)]
    pub parent_span_id: Option<SpanId>,
    #[serde(default)]
    pub span_links: Vec<SpanLink>,
    pub correlation_id: RequestId,
    #[serde(default)]
    pub causation: Option<CausationRef>,
    #[serde(default)]
    pub command_id: Option<RequestId>,
    pub request_id: RequestId,
    #[serde(default)]
    pub organization_id: Option<OrganizationId>,
    #[serde(default)]
    pub project_id: Option<ProjectId>,
    pub session_id: SessionId,
    #[serde(default)]
    pub run_id: Option<RunId>,
    #[serde(default)]
    pub turn_id: Option<TurnId>,
    #[serde(default)]
    pub invocation_id: Option<InvocationId>,
    #[serde(default)]
    pub execution_id: Option<ExecutionId>,
    #[serde(default)]
    pub attempt: Option<AttemptRef>,
    pub actor_ref: String,
    pub authority_epoch: u64,
    pub data_epoch: u64,
    #[serde(default)]
    pub source_cursor: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CorrelationContextWire {
    schema: String,
    version: SchemaVersion,
    trace_id: TraceId,
    span_id: SpanId,
    #[serde(default)]
    parent_span_id: Option<SpanId>,
    #[serde(default)]
    span_links: Vec<SpanLink>,
    correlation_id: RequestId,
    #[serde(default)]
    causation: Option<CausationRef>,
    #[serde(default)]
    command_id: Option<RequestId>,
    request_id: RequestId,
    #[serde(default)]
    organization_id: Option<OrganizationId>,
    #[serde(default)]
    project_id: Option<ProjectId>,
    session_id: SessionId,
    #[serde(default)]
    run_id: Option<RunId>,
    #[serde(default)]
    turn_id: Option<TurnId>,
    #[serde(default)]
    invocation_id: Option<InvocationId>,
    #[serde(default)]
    execution_id: Option<ExecutionId>,
    #[serde(default)]
    attempt: Option<AttemptRef>,
    actor_ref: String,
    authority_epoch: u64,
    data_epoch: u64,
    #[serde(default)]
    source_cursor: Option<u64>,
}

impl<'de> Deserialize<'de> for CorrelationContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = CorrelationContextWire::deserialize(deserializer)?;
        let context = Self {
            schema: wire.schema,
            version: wire.version,
            trace_id: wire.trace_id,
            span_id: wire.span_id,
            parent_span_id: wire.parent_span_id,
            span_links: wire.span_links,
            correlation_id: wire.correlation_id,
            causation: wire.causation,
            command_id: wire.command_id,
            request_id: wire.request_id,
            organization_id: wire.organization_id,
            project_id: wire.project_id,
            session_id: wire.session_id,
            run_id: wire.run_id,
            turn_id: wire.turn_id,
            invocation_id: wire.invocation_id,
            execution_id: wire.execution_id,
            attempt: wire.attempt,
            actor_ref: wire.actor_ref,
            authority_epoch: wire.authority_epoch,
            data_epoch: wire.data_epoch,
            source_cursor: wire.source_cursor,
        };
        context.validate().map_err(D::Error::custom)?;
        Ok(context)
    }
}

impl CorrelationContext {
    /// Construct the root context after the caller has authenticated and scoped the request.
    pub fn from_request(
        request: &RequestContext,
        scope: CorrelationScope,
        authority_epoch: u64,
        data_epoch: u64,
        traceparent: Option<&str>,
    ) -> Result<Self, String> {
        scope.validate()?;
        if request.session_id != scope.session_id {
            return Err("correlation_session_mismatch".to_owned());
        }
        let actor_ref = request
            .actor_id
            .as_deref()
            .filter(|actor| !actor.trim().is_empty())
            .ok_or_else(|| "correlation_actor_required".to_owned())?;
        if authority_epoch == 0 || data_epoch == 0 {
            return Err("correlation_epoch_required".to_owned());
        }
        let foreign_parent = traceparent
            .map(TraceParent::parse)
            .transpose()?
            .map(|parent| SpanLink::new(parent.parent_ref(), SpanLinkKind::ForeignParent));
        let trace_id = TraceId::new();
        let context = Self {
            schema: CORRELATION_CONTEXT_SCHEMA.to_owned(),
            version: CORRELATION_CONTEXT_SCHEMA_VERSION,
            trace_id,
            span_id: SpanId::new(),
            parent_span_id: None,
            span_links: foreign_parent.into_iter().collect(),
            correlation_id: request.request_id,
            causation: None,
            command_id: Some(request.request_id),
            request_id: request.request_id,
            organization_id: scope.organization_id,
            project_id: scope.project_id,
            session_id: scope.session_id.clone(),
            run_id: None,
            turn_id: None,
            invocation_id: None,
            execution_id: None,
            attempt: None,
            actor_ref: actor_ref.to_owned(),
            authority_epoch,
            data_epoch,
            source_cursor: None,
        };
        context.validate()?;
        context.validate_for_request(request, &scope, authority_epoch, data_epoch)?;
        Ok(context)
    }

    pub fn root(
        request: &RequestContext,
        scope: CorrelationScope,
        authority_epoch: u64,
        data_epoch: u64,
        traceparent: Option<&str>,
    ) -> Result<Self, String> {
        Self::from_request(request, scope, authority_epoch, data_epoch, traceparent)
    }

    pub fn scope(&self) -> CorrelationScope {
        CorrelationScope {
            organization_id: self.organization_id,
            project_id: self.project_id,
            session_id: self.session_id.clone(),
        }
    }

    pub fn current_span(&self) -> SpanRef {
        SpanRef {
            trace_id: self.trace_id.clone(),
            span_id: self.span_id.clone(),
        }
    }

    /// Validate the context against the authenticated request and its current authority snapshot.
    pub fn validate_for_request(
        &self,
        request: &RequestContext,
        scope: &CorrelationScope,
        authority_epoch: u64,
        data_epoch: u64,
    ) -> Result<(), String> {
        self.validate()?;
        scope.validate()?;
        if self.request_id != request.request_id || self.correlation_id != request.request_id {
            return Err("correlation_request_mismatch".to_owned());
        }
        if self.command_id != Some(request.request_id) {
            return Err("correlation_command_mismatch".to_owned());
        }
        if self.session_id != request.session_id || self.scope() != *scope {
            return Err("correlation_scope_mismatch".to_owned());
        }
        let actor = request
            .actor_id
            .as_deref()
            .filter(|actor| !actor.trim().is_empty())
            .ok_or_else(|| "correlation_actor_required".to_owned())?;
        if self.actor_ref != actor {
            return Err("correlation_actor_mismatch".to_owned());
        }
        if self.authority_epoch != authority_epoch {
            return Err("correlation_authority_epoch_mismatch".to_owned());
        }
        if self.data_epoch != data_epoch {
            return Err("correlation_data_epoch_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate_scope(&self, expected: &CorrelationScope) -> Result<(), String> {
        if self.scope() == *expected {
            Ok(())
        } else {
            Err("correlation_scope_mismatch".to_owned())
        }
    }

    pub fn child_span(&self) -> Result<Self, String> {
        self.validate()?;
        let mut child = self.clone();
        child.parent_span_id = Some(self.span_id.clone());
        child.span_id = SpanId::new();
        child.span_links.clear();
        child.validate()?;
        Ok(child)
    }

    /// Create an asynchronous/recovery child. It has no parent owner and links the upstream span.
    pub fn linked_child(&self, relationship: SpanLinkKind) -> Result<Self, String> {
        if relationship == SpanLinkKind::Parent {
            return Err("correlation_async_parent_link_forbidden".to_owned());
        }
        self.validate()?;
        let mut child = self.clone();
        child.parent_span_id = None;
        child.span_id = SpanId::new();
        child.span_links = vec![SpanLink::new(self.current_span(), relationship)];
        child.validate()?;
        Ok(child)
    }

    pub fn with_run(&self, run_id: RunId) -> Result<Self, String> {
        let mut next = self.clone();
        if next.run_id.is_some_and(|existing| existing != run_id) {
            return Err("correlation_run_mismatch".to_owned());
        }
        next.run_id = Some(run_id);
        next.validate()?;
        Ok(next)
    }

    pub fn with_turn(&self, turn_id: TurnId) -> Result<Self, String> {
        if self.run_id.is_none() {
            return Err("correlation_turn_requires_run".to_owned());
        }
        let mut next = self.clone();
        if next.turn_id.is_some_and(|existing| existing != turn_id) {
            return Err("correlation_turn_mismatch".to_owned());
        }
        next.turn_id = Some(turn_id);
        next.validate()?;
        Ok(next)
    }

    pub fn with_invocation(
        &self,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
    ) -> Result<Self, String> {
        if self.run_id.is_none() || self.turn_id.is_none() {
            return Err("correlation_invocation_requires_turn".to_owned());
        }
        let mut next = self.clone();
        if next
            .invocation_id
            .is_some_and(|existing| existing != invocation_id)
            || next
                .execution_id
                .is_some_and(|existing| existing != execution_id)
        {
            return Err("correlation_invocation_mismatch".to_owned());
        }
        next.invocation_id = Some(invocation_id);
        next.execution_id = Some(execution_id);
        next.validate()?;
        Ok(next)
    }

    pub fn with_attempt(&self, attempt: AttemptRef) -> Result<Self, String> {
        self.validate_attempt(&attempt)?;
        let mut next = self.clone();
        next.attempt = Some(attempt);
        next.validate()?;
        Ok(next)
    }

    pub fn bind_attempt(&self, attempt: AttemptRef) -> Result<Self, String> {
        self.with_attempt(attempt)
    }

    pub fn with_causation(&self, causation: CausationRef) -> Result<Self, String> {
        causation.validate_against(self)?;
        let mut next = self.clone();
        next.causation = Some(causation);
        next.validate()?;
        Ok(next)
    }

    pub fn with_source_cursor(&self, source_cursor: u64) -> Result<Self, String> {
        if source_cursor == 0 {
            return Err("correlation_source_cursor_required".to_owned());
        }
        let mut next = self.clone();
        next.source_cursor = Some(source_cursor);
        next.validate()?;
        Ok(next)
    }

    fn validate_attempt(&self, attempt: &AttemptRef) -> Result<(), String> {
        attempt.validate()?;
        if self.run_id != Some(attempt.run_id)
            || self.turn_id != Some(attempt.turn_id)
            || self.invocation_id != Some(attempt.invocation_id)
            || self.execution_id != Some(attempt.execution_id)
        {
            return Err("correlation_attempt_scope_mismatch".to_owned());
        }
        if self.command_id != Some(attempt.command_id) {
            return Err("correlation_attempt_command_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        check_schema_compatibility(&self.schema, &self.version)
            .map_err(|_| "correlation_schema_incompatible".to_owned())?;
        if self.schema != CORRELATION_CONTEXT_SCHEMA {
            return Err("correlation_schema_mismatch".to_owned());
        }
        self.scope().validate()?;
        if self.correlation_id.as_uuid().is_nil() || self.request_id.as_uuid().is_nil() {
            return Err("correlation_request_id_invalid".to_owned());
        }
        if self.correlation_id != self.request_id {
            return Err("correlation_request_mismatch".to_owned());
        }
        if self
            .command_id
            .is_some_and(|command_id| command_id.as_uuid().is_nil())
        {
            return Err("correlation_command_id_invalid".to_owned());
        }
        if self.run_id.is_some_and(|run_id| run_id.as_uuid().is_nil()) {
            return Err("correlation_run_id_invalid".to_owned());
        }
        if self
            .turn_id
            .is_some_and(|turn_id| turn_id.as_uuid().is_nil())
        {
            return Err("correlation_turn_id_invalid".to_owned());
        }
        if self
            .invocation_id
            .is_some_and(|invocation_id| invocation_id.as_uuid().is_nil())
        {
            return Err("correlation_invocation_id_invalid".to_owned());
        }
        if self
            .execution_id
            .is_some_and(|execution_id| execution_id.as_uuid().is_nil())
        {
            return Err("correlation_execution_id_invalid".to_owned());
        }
        TraceId::parse(self.trace_id.as_str())?;
        SpanId::parse(self.span_id.as_str())?;
        if self.parent_span_id.as_ref() == Some(&self.span_id) {
            return Err("correlation_parent_self".to_owned());
        }
        if self.span_links.len() > MAX_SPAN_LINKS {
            return Err("correlation_span_link_limit".to_owned());
        }
        for link in &self.span_links {
            link.validate()?;
            if link.relationship == SpanLinkKind::Parent {
                return Err("correlation_parent_link_invalid".to_owned());
            }
            if link.span.trace_id == self.trace_id && link.span.span_id == self.span_id {
                return Err("correlation_span_link_self".to_owned());
            }
        }
        if self.session_id.is_empty() {
            return Err("correlation_session_required".to_owned());
        }
        if self.actor_ref.trim().is_empty() || self.actor_ref.len() > 256 {
            return Err("correlation_actor_invalid".to_owned());
        }
        if self.authority_epoch == 0 || self.data_epoch == 0 {
            return Err("correlation_epoch_required".to_owned());
        }
        if self.run_id.is_none() && (self.turn_id.is_some() || self.invocation_id.is_some()) {
            return Err("correlation_run_required".to_owned());
        }
        if self.turn_id.is_none() && (self.invocation_id.is_some() || self.execution_id.is_some()) {
            return Err("correlation_turn_required".to_owned());
        }
        if self.invocation_id.is_none() && self.execution_id.is_some() {
            return Err("correlation_invocation_required".to_owned());
        }
        if let Some(attempt) = &self.attempt {
            self.validate_attempt(attempt)?;
        }
        if let Some(causation) = &self.causation {
            causation.validate_against(self)?;
        }
        if self.source_cursor == Some(0) {
            return Err("correlation_source_cursor_required".to_owned());
        }
        Ok(())
    }
}

/// Ordered lifecycle stages a server-owned correlation context is threaded through.
///
/// The stage list mirrors the single execution spine (`ingress → run → turn → invocation →
/// provider → broker → eventlog`). It is a *description* of where a context currently is, never a
/// grant: entering a stage only derives a child span, it never authorizes an effect.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelationStage {
    Ingress,
    Run,
    Turn,
    Invocation,
    Provider,
    Broker,
    EventLog,
}

impl CorrelationStage {
    /// The full spine in order. Index equals [`CorrelationStage::position`].
    pub const ORDERED: [CorrelationStage; 7] = [
        CorrelationStage::Ingress,
        CorrelationStage::Run,
        CorrelationStage::Turn,
        CorrelationStage::Invocation,
        CorrelationStage::Provider,
        CorrelationStage::Broker,
        CorrelationStage::EventLog,
    ];

    pub fn position(self) -> usize {
        match self {
            CorrelationStage::Ingress => 0,
            CorrelationStage::Run => 1,
            CorrelationStage::Turn => 2,
            CorrelationStage::Invocation => 3,
            CorrelationStage::Provider => 4,
            CorrelationStage::Broker => 5,
            CorrelationStage::EventLog => 6,
        }
    }

    /// The only stage this one may advance to; `None` for the terminal stage.
    pub fn next(self) -> Option<CorrelationStage> {
        Self::ORDERED.get(self.position() + 1).copied()
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CorrelationStage::Ingress => "ingress",
            CorrelationStage::Run => "run",
            CorrelationStage::Turn => "turn",
            CorrelationStage::Invocation => "invocation",
            CorrelationStage::Provider => "provider",
            CorrelationStage::Broker => "broker",
            CorrelationStage::EventLog => "eventlog",
        }
    }
}

/// One stage of an authenticated correlation trail together with its server-owned context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelationStageEntry {
    pub stage: CorrelationStage,
    pub context: CorrelationContext,
}

/// Typed bridge that carries one authenticated correlation root through the execution spine.
///
/// The trail starts from the ingress context that `CorrelationContext::from_request` produced
/// after authentication and scope resolution. Every advance derives a child span from the previous
/// stage, so the whole spine shares one trace and one identity/scope/command binding. A trail is a
/// process-local description, not persisted authority: the persisted artifact is the per-stage
/// [`CorrelationContext`], and staging never grants an effect.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelationTrail {
    entries: Vec<CorrelationStageEntry>,
}

impl CorrelationTrail {
    /// Start the trail from the authenticated ingress root.
    pub fn from_ingress(root: CorrelationContext) -> Result<Self, String> {
        Self::from_entries(vec![CorrelationStageEntry {
            stage: CorrelationStage::Ingress,
            context: root,
        }])
    }

    /// Rebuild a trail from explicit entries, re-validating every stage and link.
    ///
    /// This is the only constructor that accepts a caller-supplied stage list, so a forged,
    /// reordered or drifted trail fails closed before any caller can trust it.
    pub fn from_entries(entries: Vec<CorrelationStageEntry>) -> Result<Self, String> {
        let trail = Self { entries };
        trail.validate()?;
        Ok(trail)
    }

    pub fn entries(&self) -> &[CorrelationStageEntry] {
        &self.entries
    }

    pub fn current(&self) -> &CorrelationContext {
        &self
            .entries
            .last()
            .expect("validated trail is never empty")
            .context
    }

    pub fn current_stage(&self) -> CorrelationStage {
        self.entries
            .last()
            .expect("validated trail is never empty")
            .stage
    }

    pub fn context_at(&self, stage: CorrelationStage) -> Option<&CorrelationContext> {
        self.entries
            .iter()
            .find(|entry| entry.stage == stage)
            .map(|entry| &entry.context)
    }

    /// The terminal EventLog correlation, once the trail has reached the eventlog stage.
    pub fn event_correlation(&self) -> Result<&CorrelationContext, String> {
        self.context_at(CorrelationStage::EventLog)
            .ok_or_else(|| "correlation_trail_eventlog_missing".to_owned())
    }

    fn advanced_context(&self, stage: CorrelationStage) -> Result<CorrelationContext, String> {
        self.validate()?;
        let expected = self
            .current_stage()
            .next()
            .ok_or_else(|| "correlation_trail_terminal".to_owned())?;
        if stage != expected {
            return Err("correlation_trail_stage_order".to_owned());
        }
        self.current().child_span()
    }

    fn append(
        mut self,
        stage: CorrelationStage,
        context: CorrelationContext,
    ) -> Result<Self, String> {
        self.entries.push(CorrelationStageEntry { stage, context });
        self.validate()?;
        Ok(self)
    }

    pub fn with_run(self, run_id: RunId) -> Result<Self, String> {
        let context = self.advanced_context(CorrelationStage::Run)?.with_run(run_id)?;
        self.append(CorrelationStage::Run, context)
    }

    pub fn with_turn(self, turn_id: TurnId) -> Result<Self, String> {
        let context = self
            .advanced_context(CorrelationStage::Turn)?
            .with_turn(turn_id)?;
        self.append(CorrelationStage::Turn, context)
    }

    pub fn with_invocation(
        self,
        invocation_id: InvocationId,
        execution_id: ExecutionId,
    ) -> Result<Self, String> {
        let context = self
            .advanced_context(CorrelationStage::Invocation)?
            .with_invocation(invocation_id, execution_id)?;
        self.append(CorrelationStage::Invocation, context)
    }

    /// Bind the attempt to the current invocation stage without opening a new span.
    pub fn with_attempt(mut self, attempt: AttemptRef) -> Result<Self, String> {
        self.validate()?;
        if self.current_stage() != CorrelationStage::Invocation {
            return Err("correlation_attempt_requires_invocation".to_owned());
        }
        let bound = self.current().with_attempt(attempt)?;
        let last = self
            .entries
            .last_mut()
            .ok_or_else(|| "correlation_trail_empty".to_owned())?;
        last.context = bound;
        self.validate()?;
        Ok(self)
    }

    pub fn provider(self) -> Result<Self, String> {
        let context = self.advanced_context(CorrelationStage::Provider)?;
        self.append(CorrelationStage::Provider, context)
    }

    pub fn broker(self) -> Result<Self, String> {
        let context = self.advanced_context(CorrelationStage::Broker)?;
        self.append(CorrelationStage::Broker, context)
    }

    pub fn event_log(self, source_cursor: u64) -> Result<Self, String> {
        let context = self
            .advanced_context(CorrelationStage::EventLog)?
            .with_source_cursor(source_cursor)?;
        self.append(CorrelationStage::EventLog, context)
    }

    /// Asynchronous recovery: never reuses the upstream span. It records a link to the upstream
    /// span and opens a fresh span so recovery work cannot overwrite or inherit the original owner.
    pub fn async_recovery(
        &self,
        relationship: SpanLinkKind,
    ) -> Result<CorrelationRecovery, String> {
        self.validate()?;
        if relationship == SpanLinkKind::Parent {
            return Err("correlation_async_parent_link_forbidden".to_owned());
        }
        let upstream = self.current().clone();
        let mut context = upstream.clone();
        context.parent_span_id = None;
        context.span_id = SpanId::new();
        context.span_links = vec![SpanLink::new(upstream.current_span(), relationship)];
        context.causation = None;
        context.run_id = None;
        context.turn_id = None;
        context.invocation_id = None;
        context.execution_id = None;
        context.attempt = None;
        context.source_cursor = None;
        context.validate()?;
        if context.trace_id != upstream.trace_id {
            return Err("correlation_recovery_trace_drift".to_owned());
        }
        if context.span_id == upstream.span_id {
            return Err("correlation_recovery_span_reuse".to_owned());
        }
        Ok(CorrelationRecovery {
            upstream: upstream.current_span(),
            relationship,
            context,
        })
    }

    /// Validate stage order, parent chain and identity/scope stability for the whole spine.
    pub fn validate(&self) -> Result<(), String> {
        let root = self
            .entries
            .first()
            .ok_or_else(|| "correlation_trail_empty".to_owned())?;
        if root.stage != CorrelationStage::Ingress {
            return Err("correlation_trail_ingress_required".to_owned());
        }
        root.context.validate()?;
        if root.context.parent_span_id.is_some() {
            return Err("correlation_trail_ingress_parent_forbidden".to_owned());
        }
        if root.context.run_id.is_some()
            || root.context.turn_id.is_some()
            || root.context.invocation_id.is_some()
            || root.context.execution_id.is_some()
            || root.context.attempt.is_some()
        {
            return Err("correlation_trail_ingress_scope_forbidden".to_owned());
        }
        let mut previous: Option<&CorrelationStageEntry> = None;
        for (index, entry) in self.entries.iter().enumerate() {
            entry.context.validate()?;
            if entry.stage.position() != index {
                return Err("correlation_trail_stage_order".to_owned());
            }
            if entry.context.trace_id != root.context.trace_id {
                return Err("correlation_trail_trace_drift".to_owned());
            }
            if entry.context.request_id != root.context.request_id
                || entry.context.correlation_id != root.context.correlation_id
                || entry.context.command_id != root.context.command_id
                || entry.context.actor_ref != root.context.actor_ref
                || entry.context.authority_epoch != root.context.authority_epoch
                || entry.context.data_epoch != root.context.data_epoch
                || entry.context.scope() != root.context.scope()
            {
                return Err("correlation_trail_identity_drift".to_owned());
            }
            if let Some(previous) = previous {
                if entry.context.parent_span_id.as_ref() != Some(&previous.context.span_id) {
                    return Err("correlation_trail_parent_link".to_owned());
                }
            }
            previous = Some(entry);
        }
        Ok(())
    }
}

/// A correlated asynchronous recovery: a link to the upstream span plus a fresh span.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CorrelationRecovery {
    pub upstream: SpanRef,
    pub relationship: SpanLinkKind,
    pub context: CorrelationContext,
}

impl CorrelationRecovery {
    /// Start a new trail for the recovered asynchronous work.
    ///
    /// The recovered root keeps the upstream trace and scope, carries the link, and has no parent
    /// span, so the recovered run re-binds its own run/turn/invocation instead of inheriting the
    /// upstream owner.
    pub fn trail(&self) -> Result<CorrelationTrail, String> {
        CorrelationTrail::from_ingress(self.context.clone())
    }
}
