pub mod analytics;
pub mod api;
pub mod auth;
pub mod compact;
pub mod errors;
pub mod lsp;
pub mod mcp;
pub mod network_policy;
pub mod oauth;

pub use errors::{ServiceError, ServiceResult};

#[cfg(test)]
pub(crate) fn env_test_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

#[cfg(test)]
mod network_policy_tests {
    use crate::network_policy::{validate_http_redirect, validate_http_url, HttpNetworkSurface};
    use url::Url;

    #[test]
    fn web_fetch_policy_denies_local_and_private_targets() {
        for url in [
            "file:///etc/passwd",
            "http://localhost/",
            "http://127.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
        ] {
            let error = validate_http_url(HttpNetworkSurface::WebFetch, url)
                .expect_err("target should be blocked");
            assert!(
                error.contains("network policy denied URL"),
                "{url} returned {error:?}"
            );
        }

        validate_http_url(HttpNetworkSurface::WebFetch, "https://example.com/").unwrap();
    }

    #[test]
    fn http_mcp_policy_allows_local_initial_url_but_blocks_cross_origin_redirects() {
        let original =
            validate_http_url(HttpNetworkSurface::HttpMcp, "http://127.0.0.1:12345/mcp").unwrap();
        let same_origin =
            validate_http_url(HttpNetworkSurface::HttpMcp, "http://127.0.0.1:12345/other").unwrap();
        validate_http_redirect(HttpNetworkSurface::HttpMcp, &original, &same_origin).unwrap();

        let cross_origin =
            validate_http_url(HttpNetworkSurface::HttpMcp, "http://127.0.0.1:23456/other").unwrap();
        let error = validate_http_redirect(HttpNetworkSurface::HttpMcp, &original, &cross_origin)
            .expect_err("MCP redirects must stay on the configured origin");
        assert!(error.contains("network policy denied redirect"));
    }

    #[test]
    fn web_search_policy_denies_local_private_and_redirect_targets() {
        for url in [
            "file:///etc/passwd",
            "http://localhost/",
            "http://127.0.0.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://10.0.0.1/",
            "http://192.168.1.1/",
        ] {
            let error = validate_http_url(HttpNetworkSurface::WebSearch, url)
                .expect_err("target should be blocked");
            assert!(
                error.contains("network policy denied URL"),
                "{url} returned {error:?}"
            );
        }

        let original = validate_http_url(
            HttpNetworkSurface::WebSearch,
            "https://duckduckgo.com/html/?q=rust",
        )
        .unwrap();
        let redirect = Url::parse("http://127.0.0.1:9/").unwrap();
        let error = validate_http_redirect(HttpNetworkSurface::WebSearch, &original, &redirect)
            .expect_err("WebSearch redirects must stay on public network targets");
        assert!(error.contains("network policy denied URL"), "{error}");
    }
}
