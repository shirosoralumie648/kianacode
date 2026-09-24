use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Fixture {
    schema: String,
    binding: Binding,
    limits: Limits,
    deny: Vec<String>,
    routes: Vec<Route>,
}

#[derive(Debug, Deserialize)]
struct Binding {
    listener: String,
    host: String,
    origin: String,
    token_header: String,
    sse_token_query: String,
}

#[derive(Debug, Deserialize)]
struct Limits {
    body_bytes: usize,
    uri_bytes: usize,
    requests_per_window: u32,
    window_ms: u64,
}

#[derive(Debug, Deserialize)]
struct Route {
    path: String,
    method: String,
    class: String,
    token_required: bool,
}

#[test]
fn ui16_route_fixture_covers_the_deny_first_matrix() {
    let fixture: Fixture = serde_json::from_str(include_str!("fixtures/ui16-web-routes.json"))
        .expect("valid UI-16 route fixture");
    assert_eq!(fixture.schema, "kiana.web-route-matrix.v1");
    assert_eq!(fixture.binding.listener, "loopback-only");
    assert_eq!(fixture.binding.host, "exact-bound-address");
    assert_eq!(fixture.binding.origin, "exact-http-origin-when-present");
    assert_eq!(fixture.binding.token_header, "x-kiana-web-token");
    assert_eq!(
        fixture.binding.sse_token_query,
        "explicit-eventsource-compatibility"
    );
    assert_eq!(fixture.limits.body_bytes, 128 * 1024);
    assert_eq!(fixture.limits.uri_bytes, 8 * 1024);
    assert_eq!(fixture.limits.requests_per_window, 120);
    assert_eq!(fixture.limits.window_ms, 1000);

    for expected in [
        "missing_token_state_events_action",
        "foreign_origin",
        "path_traversal",
        "cross_workspace_session",
        "health_absolute_path",
        "health_internal_error",
    ] {
        assert!(
            fixture.deny.iter().any(|actual| actual == expected),
            "missing deny fixture {expected}"
        );
    }

    let source = include_str!("../src/web.rs");
    for route in &fixture.routes {
        assert!(
            !route.method.is_empty(),
            "route method missing: {}",
            route.path
        );
        assert!(
            source.contains(&format!(".route(\"{}\"", route.path)),
            "route missing: {}",
            route.path
        );
        assert!(
            source.contains(&format!(
                "class: WebRouteClass::{}",
                pascal_case(&route.class)
            )),
            "route class missing: {}",
            route.class
        );
        if route.token_required {
            assert!(
                source.contains("authorize_mutation(&app, &headers)?") || route.class == "events",
                "token route lacks auth: {}",
                route.path
            );
        }
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
        .and_then(|rest| rest.split("fn single_header").next())
        .expect("source authorization");
    assert!(source_auth.contains("authorize_host(app, headers)?"));
    assert!(source_auth.contains("web_token"));
    assert!(!source_auth.contains("enforce_rate_limit"));
}

fn pascal_case(value: &str) -> String {
    value
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}
