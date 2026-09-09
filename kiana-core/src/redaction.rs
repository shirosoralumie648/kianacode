use super::*;

pub(crate) use kiana_domain::{
    redact_text as redact_event_text, redact_value as redact_event_value,
};

pub(crate) fn redact_capability_result(result: CapabilityResult) -> CapabilityResult {
    CapabilityResult {
        request_id: result.request_id,
        success: result.success,
        output: redact_event_value(&result.output),
        evidence_refs: result
            .evidence_refs
            .iter()
            .map(|reference| redact_event_text(reference))
            .collect(),
    }
}
#[cfg(test)]
mod event_redaction_tests {
    use super::{redact_event_text, redact_event_value};
    use crate::events::{capability_event_payload, direct_capability_event_payload};
    use kiana_domain::{CapabilityKind, CapabilityRequest, RequestContext, RequestId, RunId};
    use serde_json::json;

    #[test]
    fn capability_result_keeps_cell_scope_correlation() {
        let mut request = CapabilityRequest::new(
            RequestId::new(),
            CapabilityKind::Filesystem,
            "apply_patch",
            json!({ "patch": "*** Begin Patch" }),
        );
        request.cell_id = Some(kiana_domain::CellId::new());
        request.capability_grant_id = Some(kiana_domain::CapabilityGrantId::new());
        request.budget_lease_id = Some(kiana_domain::BudgetLeaseId::new());
        let context = RequestContext::local("session-1", "/repo");
        let run_id = RunId::new();
        let payload =
            capability_event_payload(&json!({ "changed": true }), &request, &context, run_id);

        assert_eq!(payload["run_id"], json!(run_id));
        assert_eq!(payload["session_id"], "session-1");
        assert_eq!(payload["cell_id"], json!(request.cell_id));
        assert_eq!(
            payload["capability_grant_id"],
            json!(request.capability_grant_id)
        );
        assert_eq!(payload["budget_lease_id"], json!(request.budget_lease_id));
        assert_eq!(payload["capability_request_id"], json!(request.request_id));
    }

    #[test]
    fn scalar_capability_result_is_wrapped_with_scope_correlation() {
        let request =
            CapabilityRequest::new(RequestId::new(), CapabilityKind::Query, "search", json!({}));
        let context = RequestContext::local("session-1", "/repo");
        let run_id = RunId::new();
        let payload = capability_event_payload(&json!(["one", "two"]), &request, &context, run_id);

        assert_eq!(payload["output"], json!(["one", "two"]));
        assert_eq!(payload["run_id"], json!(run_id));
        assert_eq!(payload["capability_request_id"], json!(request.request_id));
        assert_eq!(payload["operation"], "search");
    }

    #[test]
    fn direct_capability_result_keeps_request_scope_correlation() {
        let request =
            CapabilityRequest::new(RequestId::new(), CapabilityKind::Query, "search", json!({}));
        let payload = direct_capability_event_payload(&json!("found"), &request);

        assert_eq!(payload["output"], "found");
        assert_eq!(payload["capability_request_id"], json!(request.request_id));
        assert_eq!(payload["capability"], "query");
        assert_eq!(payload["operation"], "search");
    }

    #[test]
    fn event_error_redaction_preserves_codes_and_masks_secret_parameters() {
        let redacted = redact_event_text("provider_failed token=abc123, retryable=true");
        assert_eq!(redacted, "provider_failed token=[REDACTED], retryable=true");
    }

    #[test]
    fn event_redaction_masks_bearer_and_json_string_secrets() {
        let redacted = redact_event_value(&json!({
            "error": "Authorization: Bearer bearer-secret token=token-secret",
            "nested": ["{\"api_key\":\"json-secret\"}"],
            "secret_ref": "vault://capability",
        }));
        let text = redacted.to_string();
        assert!(!text.contains("bearer-secret"), "{text}");
        assert!(!text.contains("token-secret"), "{text}");
        assert!(!text.contains("json-secret"), "{text}");
        assert!(text.contains("[REDACTED]"), "{text}");
        assert_eq!(redacted["secret_ref"], "vault://capability");
    }

    #[test]
    fn event_redaction_masks_standard_header_and_spaced_json_secrets() {
        let redacted = redact_event_value(&json!({
            "basic": "Authorization: Basic basic-secret",
            "header": "X-Api-Key:  header-secret",
            "json": "{\"api_key\": \"json-secret\", \"secret_ref\": \"vault://kept\"}",
        }));
        let text = redacted.to_string();
        for sentinel in ["basic-secret", "header-secret", "json-secret"] {
            assert!(!text.contains(sentinel), "leaked {sentinel}: {text}");
        }
        assert!(text.contains("[REDACTED]"), "{text}");
        assert!(text.contains("vault://kept"), "{text}");
    }

    #[test]
    fn event_redaction_preserves_references_and_non_sensitive_results() {
        let redacted = redact_event_value(&json!({
            "secret_ref": "provider/anthropic",
            "api_key": "do-not-record",
            "numeric_api_key": 123456,
            "tokens_before": 4096,
            "token_budget": 1000,
            "estimated_tokens": 42,
            "token_overlap": 2,
            "nested": [{"access_token": "also-private", "path": "src/lib.rs"}],
        }));
        assert_eq!(redacted["secret_ref"], "provider/anthropic");
        assert_eq!(redacted["api_key"], "[REDACTED]");
        assert_eq!(redacted["numeric_api_key"], "[REDACTED]");
        assert_eq!(redacted["tokens_before"], 4096);
        assert_eq!(redacted["token_budget"], 1000);
        assert_eq!(redacted["estimated_tokens"], 42);
        assert_eq!(redacted["token_overlap"], 2);
        assert_eq!(redacted["nested"][0]["access_token"], "[REDACTED]");
        assert_eq!(redacted["nested"][0]["path"], "src/lib.rs");
    }
}
