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
            Self::Event(_) => Ok(()),
            Self::Command(command_id) => {
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
        if self.session_id.is_empty() {
            return Err("correlation_session_required".to_owned());
        }
        Ok(())
    }
}

/// Immutable context attached to a committed signal or a derived span.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
