//! UI-16 route/auth/limit contract tests.
//!
//! These are source contracts: they bind the checked-in route fixture to the exported
//! `WEB_ROUTE_MATRIX`/`WEB_ERROR_MATRIX`/security-header constants, prove the Axum router
//! registration matches the exported inventory, prove every handler authorizes before it reads
//! any host/session state, and prove the serve path cannot log credentials or start a second
//! executor.  Live HTTP behavior is covered by the `ui16_*` fixtures inside `src/web.rs`, which
//! the same crate shard runs.

use kiana_entrypoints::web::{
    web_error_status_for_class, WebRouteClass, WEB_CSP_TEMPLATE, WEB_ERROR_MATRIX,
    WEB_ROUTE_MATRIX, WEB_SECURITY_HEADERS, WEB_TOKEN_HEADER, WEB_TOKEN_ROTATION_SCHEMA,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    schema: String,
    binding: Binding,
    limits: Limits,
    page_route: PageRoute,
    host_only_routes: Vec<String>,
    methods: Methods,
    token_rotation: TokenRotation,
    security_headers: Vec<(String, String)>,
    csp: Csp,
    error_codes: Vec<ErrorCode>,
    legacy_compat: Vec<LegacyRoute>,
    deny: Vec<String>,
    success: Vec<String>,
    routes: Vec<Route>,
    limitations: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Binding {
    listener: String,
    host: String,
    origin: String,
    token_header: String,
    sse_token_query: String,
    token_rotation_schema: String,
    method_rejection: String,
}

#[derive(Debug, Deserialize)]
struct Limits {
    body_bytes: usize,
    uri_bytes: usize,
    requests_per_window: u32,
    window_ms: u64,
}

#[derive(Debug, Deserialize)]
struct PageRoute {
    path: String,
    method: String,
    class: String,
    token_required: bool,
    host_only: bool,
}

#[derive(Debug, Deserialize)]
struct Methods {
    registered: Vec<String>,
    denied_without_handler: Vec<String>,
    wrong_method_status: u16,
    options_rejected_status: u16,
}

#[derive(Debug, Deserialize)]
struct TokenRotation {
    route: String,
    schema: String,
    echoes_credential: bool,
    generation_monotonic: bool,
    invalidates_previous_credential: bool,
    rehydrate_route: String,
    principal_changed: bool,
    lease_changed: bool,
}

#[derive(Debug, Deserialize)]
struct Csp {
    template: String,
    nonce_placeholder: String,
    report_uri: String,
    unsafe_inline: bool,
    eval: bool,
    header: String,
}

#[derive(Debug, Deserialize)]
struct ErrorCode {
    code: String,
    status: u16,
    class: String,
}

#[derive(Debug, Deserialize)]
struct LegacyRoute {
    legacy: String,
    current: String,
    method: String,
    class: String,
    disposition: String,
    writes_facts: bool,
}

#[derive(Debug, Deserialize)]
struct Route {
    path: String,
    method: String,
    class: String,
    token_required: bool,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/ui16-web-routes.json"))
        .expect("valid UI-16 route fixture")
}

fn route_class_name(class: WebRouteClass) -> &'static str {
    match class {
        WebRouteClass::Health => "health",
        WebRouteClass::Bootstrap => "bootstrap",
        WebRouteClass::State => "state",
        WebRouteClass::Sessions => "sessions",
        WebRouteClass::History => "history",
        WebRouteClass::Artifact => "artifact",
        WebRouteClass::ArtifactDetail => "artifact_detail",
        WebRouteClass::Diff => "diff",
        WebRouteClass::Events => "events",
        WebRouteClass::Notifications => "notifications",
        WebRouteClass::NotificationEvents => "notification_events",
        WebRouteClass::Run => "run",
        WebRouteClass::Cancel => "cancel",
        WebRouteClass::Trust => "trust",
        WebRouteClass::Sandbox => "sandbox",
        WebRouteClass::Session => "session",
        WebRouteClass::Receipt => "receipt",
        WebRouteClass::ReceiptDetail => "receipt_detail",
        WebRouteClass::Approval => "approval",
        WebRouteClass::Resume => "resume",
        WebRouteClass::Steer => "steer",
        WebRouteClass::Inject => "inject",
        WebRouteClass::Command => "command",
        WebRouteClass::Token => "token",
        WebRouteClass::Diagnostics => "diagnostics",
    }
}

/// One registration per router line: path, `get`/`post` method string and handler names.
fn registered_routes(source: &str) -> Vec<RegisteredRoute> {
    let block = source
        .split("fn router(app: WebApp) -> Router {")
        .nth(1)
        .and_then(|rest| rest.split(".layer(DefaultBodyLimit").next())
        .expect("router block");
    let mut routes = Vec::new();
    let mut rest = block;
    while let Some(index) = rest.find(".route(\"") {
        let after = &rest[index + ".route(\"".len()..];
        let end = after.find('"').expect("route path terminator");
        let path = after[..end].to_owned();
        let line_end = after.find('\n').unwrap_or(after.len());
        let spec = &after[end..line_end];
        let mut methods = Vec::new();
        let mut handlers = Vec::new();
        for method in ["get", "post"] {
            let needle = format!("{method}(");
            let mut search = spec;
            while let Some(position) = search.find(&needle) {
                let handler_tail = &search[position + needle.len()..];
                let handler_end = handler_tail.find(')').expect("handler terminator");
                methods.push(method.to_uppercase());
                handlers.push(handler_tail[..handler_end].to_owned());
                search = &handler_tail[handler_end..];
            }
        }
        assert!(!methods.is_empty(), "route {path} registers no handler");
        routes.push(RegisteredRoute {
            path,
            method: methods.join("/"),
            handlers,
        });
        // Advance past this registration: the scan is line-oriented, so leaving `rest` untouched
        // re-reads the same first route forever.
        rest = &after[line_end..];
    }
    routes
}

struct RegisteredRoute {
    path: String,
    method: String,
    handlers: Vec<String>,
}

fn handler_region<'a>(source: &'a str, handler: &str) -> &'a str {
    let marker = format!("async fn {handler}(");
    let start = source
        .find(&marker)
        .unwrap_or_else(|| panic!("handler {handler} is not defined in web.rs"));
    let rest = &source[start..];
    match rest.find("\n}\n") {
        Some(end) => &rest[..end],
        None => rest,
    }
}

#[test]
fn ui16_route_fixture_covers_the_deny_first_matrix() {
    let fixture = fixture();
    assert_eq!(fixture.schema, "kiana.web-route-matrix.v1");
    assert_eq!(fixture.binding.listener, "loopback-only");
    assert_eq!(fixture.binding.host, "exact-bound-address");
    assert_eq!(fixture.binding.origin, "exact-http-origin-when-present");
    assert_eq!(fixture.binding.token_header, WEB_TOKEN_HEADER);
    assert_eq!(
        fixture.binding.sse_token_query,
        "explicit-eventsource-compatibility"
    );
    assert_eq!(
        fixture.binding.token_rotation_schema,
        WEB_TOKEN_ROTATION_SCHEMA
    );
    assert_eq!(
        fixture.binding.method_rejection,
        "axum-method-rejection-before-handler"
    );
    assert_eq!(fixture.limits.body_bytes, 128 * 1024);
    assert_eq!(fixture.limits.uri_bytes, 8 * 1024);
    assert_eq!(fixture.limits.requests_per_window, 120);
    assert_eq!(fixture.limits.window_ms, 1000);
    assert!(!fixture.limitations.is_empty());

    // The page is host-only: it issues the live token but never runtime state.
    assert_eq!(fixture.page_route.path, "/");
    assert_eq!(fixture.page_route.method, "GET");
    assert_eq!(fixture.page_route.class, "page");
    assert!(fixture.page_route.host_only);
    assert!(!fixture.page_route.token_required);
    assert_eq!(
        fixture.host_only_routes,
        ["/", "/api/health", "/api/csp-report"]
    );
    assert_eq!(fixture.methods.registered, ["GET", "POST"]);
    assert_eq!(fixture.methods.wrong_method_status, 405);
    assert_eq!(fixture.methods.options_rejected_status, 405);
    for denied in ["OPTIONS", "PUT", "DELETE", "PATCH", "TRACE"] {
        assert!(
            fixture
                .methods
                .denied_without_handler
                .iter()
                .any(|method| method == denied),
            "method {denied} must stay a method rejection"
        );
    }

    // Token rotation is a local exposure-control action, never a credential echo.
    assert_eq!(fixture.token_rotation.route, "/api/token/rotate");
    assert_eq!(fixture.token_rotation.schema, WEB_TOKEN_ROTATION_SCHEMA);
    assert!(!fixture.token_rotation.echoes_credential);
    assert!(fixture.token_rotation.generation_monotonic);
    assert!(fixture.token_rotation.invalidates_previous_credential);
    assert_eq!(fixture.token_rotation.rehydrate_route, "/");
    assert!(!fixture.token_rotation.principal_changed);
    assert!(!fixture.token_rotation.lease_changed);
    assert!(WEB_ROUTE_MATRIX
        .iter()
        .any(|contract| contract.path == fixture.token_rotation.route
            && contract.class == WebRouteClass::Token
            && contract.token_required));

    for expected in [
        "missing_token_state_events_action",
        "sse_header_query_conflict",
        "foreign_origin",
        "foreign_host",
        "path_traversal",
        "encoded_path_traversal",
        "cross_workspace_session",
        "health_absolute_path",
        "health_internal_error",
        "wrong_method_options",
        "wrong_method_get_on_post_route",
        "wrong_method_post_on_get_route",
        "oversized_body",
        "rate_limit_flood",
        "token_rotation_reuse_old_token",
        "duplicate_credential_header",
        "browser_side_executor",
    ] {
        assert!(
            fixture.deny.iter().any(|actual| actual == expected),
            "missing deny fixture {expected}"
        );
    }
    for expected in [
        "host_only_page_issues_the_live_token",
        "health_probe_is_low_detail_and_tokenless",
        "per_route_token_and_origin_check",
        "security_headers_on_every_response",
        "token_rotation_then_rehydrate",
        "concurrent_tabs_observe_one_server_snapshot",
        "rate_limit_window_recovers",
    ] {
        assert!(
            fixture.success.iter().any(|actual| actual == expected),
            "missing success fixture {expected}"
        );
    }

    // The card's route classification is covered by real entries.
    for class in [
        "health", "state", "sessions", "events", "run", "cancel", "trust", "sandbox", "session",
        "receipt", "approval", "resume", "command",
    ] {
        assert!(
            fixture.routes.iter().any(|route| route.class == class),
            "route class {class} is not covered"
        );
    }

    // Fixture routes and the exported matrix are the same contract in both directions.
    assert_eq!(fixture.routes.len(), WEB_ROUTE_MATRIX.len());
    for route in &fixture.routes {
        let contract = WEB_ROUTE_MATRIX
            .iter()
            .find(|contract| contract.path == route.path)
            .unwrap_or_else(|| panic!("route {} is missing from WEB_ROUTE_MATRIX", route.path));
        assert_eq!(contract.method, route.method, "method drift {}", route.path);
        assert_eq!(
            route_class_name(contract.class),
            route.class,
            "class drift {}",
            route.path
        );
        assert_eq!(
            contract.token_required, route.token_required,
            "token requirement drift {}",
            route.path
        );
    }
    for contract in WEB_ROUTE_MATRIX {
        assert!(
            fixture
                .routes
                .iter()
                .any(|route| route.path == contract.path),
            "exported route {} is missing from the fixture",
            contract.path
        );
    }

    let source = include_str!("../src/web.rs");
    for route in &fixture.routes {
        assert!(
            source.contains(&format!(".route(\"{}\"", route.path)),
            "route missing: {}",
            route.path
        );
    }
    assert!(source.contains("authorize_sse(&app, &headers, query.token.as_deref())?"));
    assert!(source.contains("DefaultBodyLimit::max(MAX_WEB_BODY_BYTES)"));
    assert!(source.contains("MAX_WEB_URI_BYTES"));
    assert!(source.contains("MAX_WEB_REQUESTS_PER_WINDOW"));
    assert!(source.contains("web_path_traversal_denied"));
    assert!(source.contains("web_rate_limit_exceeded"));
    let request_bounds = source
        .split("async fn enforce_request_bounds")
        .nth(1)
        .and_then(|rest| rest.split("async fn security_headers").next())
        .expect("request bounds middleware");
    assert!(request_bounds.contains("enforce_rate_limit"));
    let source_auth = source
        .split("fn authorize_web_request")
        .nth(1)
        .and_then(|rest| rest.split("fn authorize_host").next())
        .expect("source authorization");
    assert!(source_auth.contains("authorize_host(app, headers)?"));
    assert!(source_auth.contains("web_token"));
    assert!(!source_auth.contains("enforce_rate_limit"));
}

#[test]
fn ui16_error_catalogue_matches_the_fixture_directionally() {
    let fixture = fixture();
    assert_eq!(fixture.error_codes.len(), WEB_ERROR_MATRIX.len());
    for entry in &fixture.error_codes {
        let contract = WEB_ERROR_MATRIX
            .iter()
            .find(|contract| contract.code == entry.code)
            .unwrap_or_else(|| {
                panic!("error code {} is missing from WEB_ERROR_MATRIX", entry.code)
            });
        assert_eq!(contract.status, entry.status, "status drift {}", entry.code);
        assert_eq!(contract.class, entry.class, "class drift {}", entry.code);
        assert_eq!(
            Some(entry.status),
            web_error_status_for_class(&entry.class),
            "unmapped error class for {}",
            entry.code
        );
    }
    for contract in WEB_ERROR_MATRIX {
        assert!(
            fixture
                .error_codes
                .iter()
                .any(|entry| entry.code == contract.code),
            "exported error code {} is missing from the fixture",
            contract.code
        );
    }
    // The codes the live HTTP fixtures assert first.
    for code in [
        "web_auth_required",
        "web_rate_limit_exceeded",
        "web_request_target_too_large",
        "web_path_traversal_denied",
        "session_unknown",
        "session_invalid",
        "web_tab_required",
        "web_shutting_down",
    ] {
        let contract = WEB_ERROR_MATRIX
            .iter()
            .find(|contract| contract.code == code)
            .unwrap_or_else(|| panic!("deny-first code {code} is missing"));
        assert_eq!(
            Some(contract.status),
            web_error_status_for_class(contract.class),
            "code {code} has no status mapping"
        );
    }
}

#[test]
fn ui16_router_registration_matches_the_exported_matrix() {
    let source = include_str!("../src/web.rs");
    let registered = registered_routes(source);
    assert_eq!(
        registered.len(),
        WEB_ROUTE_MATRIX.len() + 1,
        "the router must register the exported matrix plus the host-only page"
    );
    for route in &registered {
        if route.path == "/" {
            assert_eq!(route.method, "GET", "the page is a GET route");
            assert_eq!(route.handlers, ["index"]);
            continue;
        }
        let contract = WEB_ROUTE_MATRIX
            .iter()
            .find(|contract| contract.path == route.path)
            .unwrap_or_else(|| panic!("router route {} is not in WEB_ROUTE_MATRIX", route.path));
        assert_eq!(
            contract.method, route.method,
            "router/matrix method drift {}",
            route.path
        );
        assert!(
            !route.handlers.is_empty(),
            "route {} registers handlers",
            route.path
        );
    }
    let router_block = source
        .split("fn router(app: WebApp) -> Router {")
        .nth(1)
        .and_then(|rest| rest.split(".layer(DefaultBodyLimit").next())
        .expect("router block");
    assert!(
        !router_block.contains("options("),
        "OPTIONS must stay an unmatched-method rejection"
    );
}

#[test]
fn ui16_per_route_authorization_precedes_any_host_access() {
    let source = include_str!("../src/web.rs");
    let registered = registered_routes(source);
    let mut checked = 0;
    for route in &registered {
        let token_required = if route.path == "/" {
            false
        } else {
            WEB_ROUTE_MATRIX
                .iter()
                .find(|contract| contract.path == route.path)
                .unwrap_or_else(|| panic!("router route {} is not in WEB_ROUTE_MATRIX", route.path))
                .token_required
        };
        for handler in &route.handlers {
            let region = handler_region(source, handler);
            let auth = region
                .find("authorize_")
                .unwrap_or_else(|| panic!("{handler} never authorizes"));
            if token_required {
                assert!(
                    region.contains("authorize_mutation(") || region.contains("authorize_sse("),
                    "{handler} must require the process token before it runs"
                );
            } else {
                assert!(
                    region.contains("authorize_host("),
                    "{handler} is host-only and must still check the bound Host"
                );
            }
            // NM-15: a handler that emits an SSE stream awaits the feed subscription for the
            // lifetime of the connection, so the helper call itself is the sensitive read.
            let sensitive = [
                "app.host.",
                "app.snapshot(",
                "app.sessions",
                "app.options()",
                "app.read_session_ledger(",
                "app.history_sessions(",
                "stream_attach_state(",
            ]
            .iter()
            .filter_map(|marker| region.find(*marker))
            .min();
            if let Some(sensitive) = sensitive {
                assert!(
                    auth < sensitive,
                    "{handler} reads host/session state before authorization"
                );
            }
            checked += 1;
        }
    }
    assert!(checked >= WEB_ROUTE_MATRIX.len(), "handler order coverage");
}

#[test]
fn ui16_security_headers_csp_and_legacy_compat_stay_bound() {
    let fixture = fixture();
    let source = include_str!("../src/web.rs");

    assert_eq!(fixture.security_headers.len(), WEB_SECURITY_HEADERS.len());
    for (name, value) in &fixture.security_headers {
        assert!(
            WEB_SECURITY_HEADERS
                .iter()
                .any(|(known_name, known_value)| known_name == name && known_value == value),
            "security header {name} is not part of the reviewed draft"
        );
    }
    let security = source
        .split("async fn security_headers")
        .nth(1)
        .and_then(|rest| rest.split("\n}\n").next())
        .expect("security header middleware");
    assert!(security.contains("WEB_SECURITY_HEADERS"));
    assert!(security.contains("WEB_CSP_TEMPLATE.replace(\"{nonce}\""));
    assert!(security.contains("content-security-policy"));

    assert_eq!(fixture.csp.template, WEB_CSP_TEMPLATE);
    assert_eq!(fixture.csp.header, "content-security-policy");
    assert_eq!(fixture.csp.nonce_placeholder, "{nonce}");
    assert!(WEB_CSP_TEMPLATE.contains("nonce-{nonce}"));
    assert!(WEB_CSP_TEMPLATE.contains("report-uri /api/csp-report"));
    assert!(WEB_CSP_TEMPLATE.contains("default-src 'self'"));
    assert!(WEB_CSP_TEMPLATE.contains("object-src 'none'"));
    assert!(WEB_CSP_TEMPLATE.contains("base-uri 'none'"));
    assert!(WEB_CSP_TEMPLATE.contains("frame-ancestors 'none'"));
    assert!(!WEB_CSP_TEMPLATE.contains("unsafe-inline"));
    assert!(!WEB_CSP_TEMPLATE.contains("unsafe-eval"));
    assert!(!fixture.csp.unsafe_inline);
    assert!(!fixture.csp.eval);
    assert_eq!(fixture.csp.report_uri, "/api/csp-report");

    // Legacy compatibility stays one-way: the original workbench routes are preserved 1:1 and no
    // alias/redirect machinery was added, so nothing can widen a legacy path into a capability.
    let registered = registered_routes(source);
    assert!(!fixture.legacy_compat.is_empty());
    for entry in &fixture.legacy_compat {
        assert_eq!(entry.disposition, "preserved");
        assert_eq!(entry.legacy, entry.current);
        let route = registered
            .iter()
            .find(|route| route.path == entry.current)
            .unwrap_or_else(|| panic!("legacy route {} disappeared", entry.legacy));
        assert_eq!(
            route.method, entry.method,
            "legacy method drift {}",
            entry.legacy
        );
        assert!(!entry.class.is_empty());
    }
    assert!(!source.contains("redirect"));
}

#[test]
fn ui16_serve_path_cannot_log_credentials_or_start_a_second_executor() {
    let source = include_str!("../src/web.rs");
    let production = source
        .split("#[cfg(test)]")
        .next()
        .expect("production source");

    let serve_region = production
        .split("fn router(app: WebApp) -> Router {")
        .nth(1)
        .and_then(|rest| rest.split("fn maybe_open(").next())
        .expect("serve region");
    for macro_name in [
        "println!(",
        "eprintln!(",
        "print!(",
        "dbg!(",
        "tracing::",
        "log::",
    ] {
        assert!(
            !serve_region.contains(macro_name),
            "the request/serve path must not log: {macro_name}"
        );
    }
    for forbidden in [
        "execute_tool_call",
        "KianaHarness",
        "kiana_runner",
        "tokio::process",
        "std::process::Command",
        "Command::new(",
    ] {
        assert!(
            !serve_region.contains(forbidden),
            "the Web serve path must not start a second executor: {forbidden}"
        );
    }
    assert!(production.contains("harness_run::run_envelope_on_host"));

    // Startup banners may print where the workspace lives, but never a credential or the nonce.
    for line in production
        .lines()
        .filter(|line| line.contains("println!(") || line.contains("eprintln!("))
    {
        for forbidden in ["web_token", "token", "csp_nonce", "authorization", "origin"] {
            assert!(
                !line.contains(forbidden),
                "log line leaks {forbidden}: {line}"
            );
        }
    }

    // The only process spawn left in this module is the operator's browser opener.
    let opener = production
        .split("fn maybe_open(")
        .nth(1)
        .expect("browser opener");
    assert_eq!(opener.matches("Command::new(").count(), 3);
    assert!(opener.contains("Command::new(\"open\")"));
    assert!(opener.contains("Command::new(\"cmd\")"));
    assert!(opener.contains("Command::new(\"xdg-open\")"));
    let before_opener = production.split("fn maybe_open(").next().unwrap();
    assert!(!before_opener.contains("Command::new("));

    // The fixture-denied capability cannot come back through a status change either.
    let fixture = fixture();
    assert!(fixture
        .deny
        .iter()
        .any(|entry| entry == "browser_side_executor"));
}
