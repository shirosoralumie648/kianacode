//! UI-16 authority/health/limit source guard.
//!
//! Complements `ui16_web_routes.rs` with the boundary markers that must survive refactors:
//! the health projection stays redacted, authorization is checked before the Host/Origin pair,
//! the security headers/CSP draft and the error catalogue stay bound to their source literals,
//! and the Web surface keeps using the canonical `DaemonHost` execution path.

use kiana_entrypoints::web::{WEB_ERROR_MATRIX, WEB_TOKEN_HEADER};

#[test]
fn ui16_web_routes_keep_authority_and_health_redaction_boundaries() {
    let source = include_str!("../src/web.rs");
    for marker in [
        "WEB_ROUTE_MATRIX",
        "WebRouteClass::Health",
        "WebRouteClass::State",
        "WebRouteClass::Sessions",
        "WebRouteClass::Events",
        "WebRouteClass::Run",
        "WebRouteClass::Cancel",
        "WebRouteClass::Trust",
        "WebRouteClass::Sandbox",
        "WebRouteClass::Session",
        "WebRouteClass::Receipt",
        "WebRouteClass::Approval",
        "WebRouteClass::Resume",
        "WebRouteClass::Command",
        "WebRouteClass::Token",
        "WebRouteClass::Diagnostics",
        "WEB_TOKEN_HEADER",
        "WEB_SECURITY_HEADERS",
        "WEB_CSP_TEMPLATE",
        "WEB_TOKEN_ROTATION_SCHEMA",
        "WEB_ERROR_MATRIX",
        "rotate_web_token_route",
        "DefaultBodyLimit::max(MAX_WEB_BODY_BYTES)",
        "MAX_WEB_URI_BYTES",
        "MAX_WEB_REQUESTS_PER_WINDOW",
        "security_headers",
        "path_contains_traversal",
        "validate_web_session_id",
        "web_path_traversal_denied",
        "web_rate_limit_exceeded",
        "origin_matches_bound_addr",
        "header_token != query_token",
        "single_header",
        "x-kiana-web-token",
        "DaemonHost",
        "harness_run::run_envelope_on_host",
    ] {
        assert!(
            source.contains(marker),
            "UI-16 guard marker missing: {marker}"
        );
    }
    assert_eq!(WEB_TOKEN_HEADER, "x-kiana-web-token");

    let health = source
        .split("async fn health")
        .nth(1)
        .and_then(|rest| rest.split("async fn parity").next())
        .expect("health handler");
    assert!(
        !health.contains("folder"),
        "health must not leak an absolute folder"
    );
    assert!(
        !health.contains("{error}"),
        "health must not expose internal error text"
    );
    assert!(health.contains("health_projection_unavailable"));

    let auth = source
        .split("fn authorize_web_request")
        .nth(1)
        .and_then(|rest| rest.split("fn authorize_host").next())
        .expect("web authorization helper");
    let auth_code: String = auth.chars().filter(|ch| !ch.is_whitespace()).collect();
    assert!(auth_code.contains("app.web_token.lock()"));
    assert!(auth.contains("supplied != Some(current.as_str())"));
    assert!(auth.contains("authorize_host(app, headers)?"));
    let host = source
        .split("fn authorize_host")
        .nth(1)
        .and_then(|rest| rest.split("fn authority_matches_bound_addr").next())
        .expect("host authorization helper");
    assert!(host.contains("origin_matches_bound_addr"));
    assert!(host.contains("authority_matches_bound_addr"));
}

/// Extract every concrete error-code literal produced by the given constructor pattern.
fn scanned_codes(source: &str, pattern: &str) -> Vec<String> {
    let mut codes = Vec::new();
    let mut rest = source;
    while let Some(index) = rest.find(pattern) {
        let after = &rest[index + pattern.len()..];
        let end = after
            .find('"')
            .unwrap_or_else(|| panic!("unterminated error code after {pattern}"));
        assert!(end > 0, "empty error code after {pattern}");
        codes.push(after[..end].to_owned());
        rest = &after[end..];
    }
    codes.sort();
    codes.dedup();
    codes
}

#[test]
fn ui16_error_codes_stay_bound_to_their_constructors() {
    let source = include_str!("../src/web.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    // Every literal constructor maps to exactly one catalogue class, and every catalogue entry
    // of that class still has a literal.  A new/renamed code cannot drift out of the catalogue.
    for (pattern, class) in [
        ("ApiError::bad(\"", "request"),
        ("ApiError::conflict(\"", "conflict"),
        ("ApiError::fail(\"", "internal"),
    ] {
        let codes = scanned_codes(production, pattern);
        assert!(
            !codes.is_empty(),
            "no error codes found for constructor {pattern}"
        );
        for code in &codes {
            let contract = WEB_ERROR_MATRIX
                .iter()
                .find(|entry| entry.code == code)
                .unwrap_or_else(|| panic!("{code} is not in WEB_ERROR_MATRIX"));
            assert_eq!(contract.class, class, "{code} class drift");
        }
        for contract in WEB_ERROR_MATRIX.iter().filter(|entry| entry.class == class) {
            assert!(
                codes.iter().any(|code| code == contract.code),
                "{} is in the catalogue but has no {pattern} literal",
                contract.code
            );
        }
    }

    // Codes that are constructed with an explicit `error:` field and status.
    let explicit = scanned_codes(production, "error: \"");
    assert_eq!(
        explicit,
        [
            "web_auth_required",
            "web_rate_limit_exceeded",
            "web_shutting_down"
        ],
        "explicit-status error codes changed; update WEB_ERROR_MATRIX and the fixture"
    );
    for (code, class) in [
        ("web_auth_required", "deny"),
        ("web_rate_limit_exceeded", "limit"),
        ("web_shutting_down", "unavailable"),
    ] {
        let contract = WEB_ERROR_MATRIX
            .iter()
            .find(|entry| entry.code == code)
            .unwrap_or_else(|| panic!("{code} is not in WEB_ERROR_MATRIX"));
        assert_eq!(contract.class, class, "{code} class drift");
    }

    // Catalogue hygiene: unique codes, only the reviewed status classes.
    let mut codes: Vec<&str> = WEB_ERROR_MATRIX.iter().map(|entry| entry.code).collect();
    let total = codes.len();
    codes.sort();
    codes.dedup();
    assert_eq!(
        total,
        codes.len(),
        "duplicate error codes in WEB_ERROR_MATRIX"
    );
    for contract in WEB_ERROR_MATRIX {
        assert!(
            [400, 401, 409, 429, 500, 503].contains(&contract.status),
            "{} uses an unreviewed status {}",
            contract.code,
            contract.status
        );
    }
}

#[test]
fn ui16_token_rotation_route_never_echoes_the_credential() {
    let source = include_str!("../src/web.rs");
    let handler = source
        .split("async fn rotate_web_token_route(")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("token rotation handler");
    assert!(handler.contains("authorize_mutation(&app, &headers)?"));
    assert!(handler.contains("rotate_web_token(&app)?"));
    assert!(
        !handler.contains("app.web_token"),
        "the rotation response must not read or echo the credential"
    );
    assert!(!handler.contains("lock_string(&app.web_token)"));
    let rotate = source
        .split("fn rotate_web_token(app: &WebApp)")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("token rotation helper");
    assert!(rotate.contains("uuid::Uuid::new_v4()"));
    assert!(rotate.contains("fetch_add(1"));
}
