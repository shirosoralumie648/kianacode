//! CAP-31/CAP-32 backend selection: a fail-closed decision over host target and backend support.
//!
//! The cards for the macOS and Windows backends ask for something this repository cannot
//! honestly produce from a Linux build: a Seatbelt implementation, a Job Object/handle/ACL
//! implementation, and receipts from those machines. What *can* be written down, and what
//! matters more, is the rule for what happens when a caller selects one of those backends on a
//! host where it does not exist.
//!
//! The rule is a refusal, never a substitution. `select_backend` returns
//! [`BackendSelection::Unavailable`] for a target-only backend on the wrong host, and it never
//! returns a different, weaker backend in its place. A macOS backend selected on Linux that
//! "succeeds" would be a lie about the sandbox: the caller would believe Seatbelt confinement
//! was in force when nothing was confined. Returning `Unavailable` keeps the caller's belief
//! and the host's actual behaviour in agreement.
//!
//! `Implemented` is reachable only through [`PlatformBackendReport`] validation, which requires
//! `behavior_verified = true`. This module never constructs one; it can only forward a report
//! that already passed that gate. That is deliberate: the only way to reach `Implemented` from
//! here is to have already produced target-machine evidence.
//!
//! This is a source contract. It does not inspect the host, run Seatbelt, create a Job Object,
//! enumerate a sandbox, spawn a process or call a port. `host_is` is a caller-supplied fact
//! precisely because this module must not go looking.

use crate::{
    json_digest, PlatformBackendDisposition, PlatformBackendReport, PlatformTarget, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const BACKEND_SELECTION_SCHEMA: &str = "kiana.backend-selection.v1";
pub const BACKEND_SELECTION_DECISION_SCHEMA: &str = "kiana.backend-selection-decision.v1";
pub const BACKEND_SELECTION_VERSION: SchemaVersion = SchemaVersion::new(1, 0);
pub const MAX_BACKEND_SELECTION_LIMITATIONS: usize = 32;

/// The name of the backend that runs for the current product path on Linux.
///
/// This is the only backend that may be selected as a host execution environment in this
/// checkout. It is declared here rather than read from an env var so that the selection
/// contract and the sandbox implementation cannot drift apart silently: if the constant below
/// ever stops matching `harness_sandbox::SANDBOX_BACKEND`, the guard for this module fails.
pub const HOST_EXECUTION_BACKEND: &str = "bwrap";

/// A backend the caller has asked for, together with the server's own claim about it.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSelectionRequest {
    pub schema: String,
    pub version: SchemaVersion,
    /// The target the caller believes it is running on. It is checked, not trusted.
    pub requested_target: PlatformTarget,
    /// The backend name the caller asked for.
    pub requested_backend: String,
    /// The target this build actually runs on, as observed by the composition root.
    pub host_target: PlatformTarget,
    /// The backend report the server holds for `requested_backend`, if any. `None` means the
    /// server knows of no such backend, which is an absence of support, not a licence to pick
    /// another one.
    pub report: Option<PlatformBackendReport>,
    pub request_digest: String,
}

impl BackendSelectionRequest {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        requested_target: PlatformTarget,
        requested_backend: impl Into<String>,
        host_target: PlatformTarget,
        report: Option<PlatformBackendReport>,
    ) -> Result<Self, String> {
        let mut request = Self {
            schema: BACKEND_SELECTION_SCHEMA.to_owned(),
            version: BACKEND_SELECTION_VERSION,
            requested_target,
            requested_backend: requested_backend.into(),
            host_target,
            report,
            request_digest: String::new(),
        };
        request.request_digest = request.digest();
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != BACKEND_SELECTION_SCHEMA
            || !self.version.is_compatible_with(&BACKEND_SELECTION_VERSION)
            || self.requested_backend.trim().is_empty()
            || self.requested_backend.len() > 128
            || self.requested_backend.contains(['\0', '\n', '\r'])
        {
            return Err("backend_selection_request_invalid".to_owned());
        }
        if let Some(report) = &self.report {
            report.validate()?;
            if report.backend != self.requested_backend {
                return Err("backend_selection_report_backend_mismatch".to_owned());
            }
        }
        if self.request_digest != self.digest() {
            return Err("backend_selection_request_digest_mismatch".to_owned());
        }
        Ok(())
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "requested_target": self.requested_target,
            "requested_backend": self.requested_backend,
            "host_target": self.host_target,
            "report": self.report,
        }))
    }
}

/// The outcome of asking for a backend.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendSelection {
    /// The requested backend is the backend that will be used.
    Selected,
    /// The backend exists only on another target. Using it here is refused, and the name of the
    /// target that could run it is reported.
    TargetOnly,
    /// No backend of that name is known to the server. Refused.
    NotSupported,
    /// The backend is known and rejected on this host, for example because a required
    /// enforcement dimension is missing. Refused.
    Blocked,
}

impl BackendSelection {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Selected => "selected",
            Self::TargetOnly => "target_only",
            Self::NotSupported => "not_supported",
            Self::Blocked => "blocked",
        }
    }

    /// Whether execution may proceed on this host with the requested backend.
    ///
    /// Only `Selected` may. Every other value is a refusal, and this is the single place that
    /// fact is expressed, so a caller cannot accidentally treat a non-`Selected` answer as a
    /// usable environment.
    pub const fn is_usable(self) -> bool {
        matches!(self, Self::Selected)
    }
}

/// The ordered decision for one backend selection.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackendSelectionDecision {
    pub schema: String,
    pub version: SchemaVersion,
    pub requested_target: PlatformTarget,
    pub requested_backend: String,
    pub host_target: PlatformTarget,
    pub selection: BackendSelection,
    /// Always true. A refused selection must never name a substitute backend, and this field
    /// makes that claim checkable rather than a promise in a comment.
    pub no_host_fallback: bool,
    /// Always false in this slice. This module never grants a capability or an executor; it
    /// only decides whether a backend may be selected.
    pub grants_execution: bool,
    pub reason: String,
    pub remediation: String,
    /// The backend that will actually be used. Equal to `requested_backend` only when
    /// `selection` is `Selected`; otherwise empty, because there is no substitute.
    pub effective_backend: String,
    pub request_digest: String,
    pub decision_digest: String,
}

impl BackendSelectionDecision {
    pub fn evaluate(request: &BackendSelectionRequest) -> Result<Self, String> {
        let (selection, reason, remediation) = derive(request);
        let mut decision = Self {
            schema: BACKEND_SELECTION_DECISION_SCHEMA.to_owned(),
            version: BACKEND_SELECTION_VERSION,
            requested_target: request.requested_target,
            requested_backend: request.requested_backend.clone(),
            host_target: request.host_target,
            selection,
            no_host_fallback: true,
            grants_execution: false,
            reason: reason.to_owned(),
            remediation: remediation.to_owned(),
            effective_backend: if selection == BackendSelection::Selected {
                request.requested_backend.clone()
            } else {
                String::new()
            },
            request_digest: request.request_digest.clone(),
            decision_digest: String::new(),
        };
        decision.decision_digest = decision.digest();
        decision.validate_against(request)?;
        Ok(decision)
    }

    pub fn validate_against(&self, request: &BackendSelectionRequest) -> Result<(), String> {
        request.validate()?;
        let (selection, reason, remediation) = derive(request);
        if self.schema != BACKEND_SELECTION_DECISION_SCHEMA
            || !self.version.is_compatible_with(&BACKEND_SELECTION_VERSION)
            || self.requested_target != request.requested_target
            || self.requested_backend != request.requested_backend
            || self.host_target != request.host_target
            || self.selection != selection
            || self.reason != reason
            || self.remediation != remediation
            || self.request_digest != request.request_digest
        {
            return Err("backend_selection_decision_binding_invalid".to_owned());
        }
        if !self.no_host_fallback {
            return Err("backend_selection_host_fallback_forbidden".to_owned());
        }
        if self.grants_execution {
            return Err("backend_selection_grants_execution".to_owned());
        }
        // A refused selection must not carry a backend. This is the check that makes "no host
        // fallback" a property of the value rather than of the prose around it.
        if !selection.is_usable() && !self.effective_backend.is_empty() {
            return Err("backend_selection_substitute_backend_named".to_owned());
        }
        if selection.is_usable() && self.effective_backend != request.requested_backend {
            return Err("backend_selection_effective_backend_mismatch".to_owned());
        }
        if self.decision_digest != self.digest() {
            return Err("backend_selection_decision_digest_mismatch".to_owned());
        }
        Ok(())
    }

    /// Whether the caller may proceed with the requested backend.
    pub fn usable(&self) -> bool {
        self.selection.is_usable()
    }

    pub fn digest(&self) -> String {
        json_digest(&json!({
            "schema": self.schema,
            "version": self.version,
            "requested_target": self.requested_target,
            "requested_backend": self.requested_backend,
            "host_target": self.host_target,
            "selection": self.selection,
            "no_host_fallback": self.no_host_fallback,
            "grants_execution": self.grants_execution,
            "reason": self.reason,
            "remediation": self.remediation,
            "effective_backend": self.effective_backend,
            "request_digest": self.request_digest,
        }))
    }
}

/// Decide in a fixed order. The order is: is the request's own claim about its target true, then
/// does the server know this backend at all, then is its disposition usable on this host.
///
/// The first rule is the one that matters most for the honesty requirement in the cards. A
/// caller that believes it is on macOS while the host is Linux is itself evidence of a
/// misconfigured or forged environment, and answering `Selected` there would convert a
/// configuration bug into a confinement lie. So a target mismatch is refused before the
/// backend's own disposition is even consulted.
fn derive(request: &BackendSelectionRequest) -> (BackendSelection, &'static str, &'static str) {
    if request.requested_target != request.host_target {
        return (
            BackendSelection::Blocked,
            "backend_selection_target_mismatch",
            "run on the target platform or select the host execution backend",
        );
    }
    let Some(report) = &request.report else {
        return (
            BackendSelection::NotSupported,
            "backend_selection_unknown_backend",
            "register a backend report before selecting it",
        );
    };
    if report.validate().is_err() {
        return (
            BackendSelection::Blocked,
            "backend_selection_report_invalid",
            "repair the backend report; an invalid report is never treated as support",
        );
    }
    if report.target != request.host_target {
        return (
            BackendSelection::TargetOnly,
            "backend_selection_target_only_backend",
            "run on the target platform; this backend has no implementation here",
        );
    }
    match report.disposition {
        PlatformBackendDisposition::Implemented => {
            if !report.behavior_verified {
                return (
                    BackendSelection::Blocked,
                    "backend_selection_behavior_unverified",
                    "produce target-machine evidence before selecting this backend",
                );
            }
            (BackendSelection::Selected, "backend_selection_accepted", "")
        }
        PlatformBackendDisposition::TargetOnly => (
            BackendSelection::TargetOnly,
            "backend_selection_target_only_backend",
            "run on the target platform; this backend has no implementation here",
        ),
        PlatformBackendDisposition::NotSupported => (
            BackendSelection::NotSupported,
            "backend_selection_not_supported",
            "select a backend implemented for this host",
        ),
        PlatformBackendDisposition::Blocked => (
            BackendSelection::Blocked,
            "backend_selection_blocked",
            "resolve the reported enforcement gap before selecting this backend",
        ),
    }
}

/// The limitations a report must carry for a target-only backend, so the caller sees why the
/// request was refused without having to read this module's source.
pub fn target_only_limitations(target: PlatformTarget, backend: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("behavior_verified".to_owned(), "false".to_owned()),
        (
            "evidence".to_owned(),
            format!("no {backend} receipt exists for {target:?}; see the CAP-3x baseline"),
        ),
    ])
}

/// Whether the current host can run `backend`, according to a caller-supplied report.
///
/// This is the convenience form used by adapters: it never constructs a report, so a caller
/// cannot use it to obtain an `Implemented` answer without holding target-machine evidence.
pub fn backend_is_selectable(
    host_target: PlatformTarget,
    backend: &str,
    report: Option<&PlatformBackendReport>,
) -> bool {
    let Ok(request) =
        BackendSelectionRequest::new(host_target, backend, host_target, report.cloned())
    else {
        return false;
    };
    BackendSelectionDecision::evaluate(&request)
        .map(|decision| decision.usable())
        .unwrap_or(false)
}
