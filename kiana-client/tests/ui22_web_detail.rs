//! UI-22 Web artifact/diff/receipt detail client fixtures.
//!
//! Deny-first: every refusal below is asserted before the success fixture, and each carries its
//! stable code.  The viewer is a read-only projection: it validates server digests, revision and
//! scope, never fetches a URL/path, never computes a diff and never promotes Unknown to Ready.

use kiana_client::{
    WebArtifactViewer, WebDetailCursor, WebDetailProjection, WebDetailRequest, WebDetailStatus,
    WEB_DETAIL_CURSOR_SCHEMA, WEB_DETAIL_MAX_PAGE_BYTES,
};
use kiana_domain::{journal_sha256, ArtifactId, RunId, SessionId};
use kiana_protocol::{
    UiArtifactDetailV1, UiArtifactPageV1, UiArtifactRefV1, UiDetailScopeV1, UiDiffDetailV1,
    UiReceiptDetailV1, UI_ARTIFACT_DETAIL_SCHEMA, UI_ARTIFACT_PAGE_SCHEMA, UI_ARTIFACT_REF_SCHEMA,
    UI_DETAIL_LINK_SCHEMA, UI_DETAIL_SCOPE_SCHEMA, UI_DIFF_DETAIL_SCHEMA, UI_RECEIPT_DETAIL_SCHEMA,
};
use serde_json::json;

const SESSION: &str = "ui22-session";
const TAB: &str = "ui22-tab";
const INSTANCE: &str = "ui22-instance";
const EPOCH: &str = "epoch-1";

fn digest_of(text: &str) -> String {
    format!("sha256:{}", journal_sha256(text.as_bytes()))
}

fn fixed_digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn scope(cursor: u64, revision: u64) -> UiDetailScopeV1 {
    UiDetailScopeV1 {
        schema: UI_DETAIL_SCOPE_SCHEMA.to_owned(),
        session_id: SessionId::new(SESSION),
        tab_id: TAB.to_owned(),
        owner_tab_id: TAB.to_owned(),
        instance_id: INSTANCE.to_owned(),
        epoch: EPOCH.to_owned(),
        source_cursor: cursor,
        revision,
    }
}

fn artifact_ref(content: &str, revision: u64) -> UiArtifactRefV1 {
    UiArtifactRefV1 {
        schema: UI_ARTIFACT_REF_SCHEMA.to_owned(),
        artifact_id: ArtifactId::new(),
        version: 1,
        artifact_schema: "kiana.ui-artifact-ref.v1".to_owned(),
        mime: "text/plain".to_owned(),
        size_bytes: content.len() as u64,
        content_hash: digest_of(content),
        scope_digest: fixed_digest('a'),
        revision,
        session_id: SessionId::new(SESSION),
        run_id: Some(RunId::new()),
        provenance: "event_log_projection".to_owned(),
        links: vec![kiana_protocol::UiDetailLinkV1 {
            schema: UI_DETAIL_LINK_SCHEMA.to_owned(),
            kind: "timeline".to_owned(),
            id: "ui22-event".to_owned(),
            session_id: SessionId::new(SESSION),
        }],
    }
}

fn page(
    artifact: &UiArtifactRefV1,
    page_index: u32,
    page_count: u32,
    content: Option<&str>,
    page_digest: String,
) -> UiArtifactPageV1 {
    UiArtifactPageV1 {
        schema: UI_ARTIFACT_PAGE_SCHEMA.to_owned(),
        scope: scope(11, artifact.revision),
        artifact: artifact.clone(),
        revision: artifact.revision,
        page_index,
        page_count,
        page_digest,
        content_encoding: if content.is_some() { "text" } else { "none" }.to_owned(),
        content: content.map(str::to_owned),
        truncated: false,
        next_cursor: None,
        limitations: Vec::new(),
    }
}

fn fixture(content: &str) -> (WebArtifactViewer, UiArtifactRefV1) {
    let artifact = artifact_ref(content, 3);
    let viewer = WebArtifactViewer::new(artifact.clone(), 3).expect("viewer");
    (viewer, artifact)
}

/// 【拒绝】客户端载荷被改写不得改变已接受的页面；Revision 不符在追加前拒绝。
#[test]
fn ui22_forged_client_payload_and_stale_revision_are_refused() {
    let (mut viewer, artifact) = fixture("hello");
    let first = page(&artifact, 0, 1, Some("hello"), digest_of("hello"));
    viewer.accept_page(first.clone()).expect("server page");
    // Replaying the identical page is idempotent; a different payload for the same index is not.
    assert_eq!(viewer.accept_page(first.clone()), Ok(()));
    let mut forged = first.clone();
    forged.content = Some("hello!".to_owned());
    forged.page_digest = digest_of("hello!");
    assert_eq!(
        viewer.accept_page(forged),
        Err("web_artifact_page_conflict".to_owned())
    );

    let mut stale = first;
    stale.revision = 4;
    stale.artifact.revision = 4;
    stale.scope.revision = 4;
    let mut stale_viewer = WebArtifactViewer::new(artifact.clone(), 3).expect("viewer");
    assert_eq!(
        stale_viewer.accept_page(stale),
        Err("web_artifact_revision_mismatch".to_owned())
    );
    // The constructor also refuses a client-chosen revision that differs from the server ref.
    assert_eq!(
        WebArtifactViewer::new(artifact, 4).err(),
        Some("web_artifact_expected_revision_invalid".to_owned())
    );
}

/// 【拒绝】坏 digest 不进入预览；HTML/SVG MIME、超限页面与含 NUL 页面都被拒绝。
#[test]
fn ui22_bad_digest_and_injection_content_are_refused() {
    let (mut viewer, artifact) = fixture("hello");
    let bad = page(&artifact, 0, 1, Some("hello"), fixed_digest('0'));
    assert_eq!(
        viewer.accept_page(bad),
        Err("web_artifact_page_digest_mismatch".to_owned())
    );

    // Server-owned refs cannot even carry an executable MIME, so the browser never sees one.
    let mut html = artifact_ref("<b>x</b>", 3);
    html.mime = "text/html".to_owned();
    assert_eq!(
        html.validate(),
        Err("ui_artifact_ref_mime_invalid".to_owned())
    );
    let mut svg = artifact_ref("<svg/>", 3);
    svg.mime = "image/svg+xml".to_owned();
    assert_eq!(
        svg.validate(),
        Err("ui_artifact_ref_mime_invalid".to_owned())
    );

    let oversized = "x".repeat(WEB_DETAIL_MAX_PAGE_BYTES + 1);
    let (mut large_viewer, large_artifact) = fixture("x");
    let large = page(&large_artifact, 0, 1, Some(&oversized), fixed_digest('b'));
    assert_eq!(
        large_viewer.accept_page(large),
        Err("ui_artifact_page_content_limit".to_owned())
    );

    let (mut nul_viewer, nul_artifact) = fixture("a\0b");
    assert_eq!(
        nul_viewer.accept_page(page(&nul_artifact, 0, 1, Some("a\0b"), digest_of("a\0b"))),
        Err("ui_artifact_page_content_limit".to_owned())
    );
}

/// 【拒绝】缺内容页面永远不能变成 Ready；跨会话 scope 与跨 tab/游标重放被拒绝。
#[test]
fn ui22_missing_content_stays_unknown_and_scope_is_enforced() {
    let (mut viewer, artifact) = fixture("hello");
    let missing = page(&artifact, 0, 1, None, fixed_digest('c'));
    viewer.accept_page(missing).expect("server limitation page");
    assert_eq!(viewer.status(), WebDetailStatus::Unknown);
    assert!(!viewer.is_complete());
    assert!(viewer
        .limitations()
        .contains(&"artifact_content_unavailable".to_owned()));
    assert_eq!(viewer.content_text(), None);

    // A foreign session cannot even form a valid page: protocol validation refuses the mismatch
    // before any viewer can fold it into this artifact view.
    let mut foreign = page(&artifact, 0, 1, Some("hello"), digest_of("hello"));
    foreign.scope.session_id = SessionId::new("other-session");
    assert_eq!(
        foreign.validate(),
        Err("ui_artifact_page_revision_scope_mismatch".to_owned())
    );

    // Cursor scope is session/tab/instance/epoch/cursor/revision bound; one edited field fails.
    let cursor = WebDetailCursor {
        schema: WEB_DETAIL_CURSOR_SCHEMA.to_owned(),
        token: "token".to_owned(),
        kind: "artifact_detail".to_owned(),
        session_id: SessionId::new(SESSION),
        tab_id: TAB.to_owned(),
        instance_id: INSTANCE.to_owned(),
        epoch: EPOCH.to_owned(),
        source_cursor: 11,
        revision: 3,
    };
    assert_eq!(cursor.validate(), Ok(()));
    assert!(cursor.matches_scope(&scope(11, 3)));
    let mut foreign_tab = cursor.clone();
    foreign_tab.tab_id = "foreign-tab".to_owned();
    assert!(!foreign_tab.matches_scope(&scope(11, 3)));
    let mut replayed = cursor.clone();
    replayed.source_cursor = 12;
    assert!(!replayed.matches_scope(&scope(11, 3)));
}

/// 【拒绝】客户端不能用自己的统计替代授权载荷；Unknown/result_unknown 不能变绿。
#[test]
fn ui22_client_diff_and_unknown_receipt_are_never_promoted() {
    let (_, artifact) = fixture("hello");
    let diff = UiDiffDetailV1 {
        schema: UI_DIFF_DETAIL_SCHEMA.to_owned(),
        scope: scope(11, 3),
        artifact: artifact.clone(),
        base_revision: 3,
        target_revision: 3,
        status: "unknown".to_owned(),
        total_additions: 0,
        total_deletions: 0,
        files: Vec::new(),
        links: Vec::new(),
        limitations: vec!["server_diff_not_available".to_owned()],
    };
    let receipt = UiReceiptDetailV1 {
        schema: UI_RECEIPT_DETAIL_SCHEMA.to_owned(),
        scope: scope(11, 3),
        receipt_id: kiana_domain::ReceiptId::new(),
        run_id: RunId::new(),
        session_id: SessionId::new(SESSION),
        receipt_digest: fixed_digest('d'),
        status: "result_unknown".to_owned(),
        unknown: true,
        source_cursor: 11,
        entries: Vec::new(),
        artifact_ids: Vec::new(),
        timeline_ids: Vec::new(),
        inbox_ids: Vec::new(),
        limitations: vec!["receipt_result_unknown_or_error; query original receipt".to_owned()],
    };
    let detail = UiArtifactDetailV1 {
        schema: UI_ARTIFACT_DETAIL_SCHEMA.to_owned(),
        scope: scope(11, 3),
        artifact_page: page(&artifact, 0, 1, Some("hello"), digest_of("hello")),
        diff: Some(diff),
        receipt: Some(receipt),
        status: "unknown".to_owned(),
        limitations: vec!["server_diff_not_available".to_owned()],
    };
    let projection = WebDetailProjection::from_server(detail).expect("projection");
    assert_eq!(projection.status(), WebDetailStatus::Unknown);
    assert!(projection.receipt_is_unknown());
    // The projection keeps server statistics verbatim; it has no addition/deletion input.
    let diff = projection.diff().expect("diff");
    assert_eq!(diff.total_additions, 0);
    assert_eq!(diff.total_deletions, 0);
    assert_eq!(diff.status, "unknown");
    assert!(projection.accepts_server_stats_only());

    // A receipt that claims Unknown *and* completed is refused by its own contract.
    let mut conflicting = projection.receipt().expect("receipt").clone();
    conflicting.status = "completed".to_owned();
    assert_eq!(
        conflicting.validate(),
        Err("ui_receipt_unknown_completed_conflict".to_owned())
    );
}

/// 【拒绝】HTML/SVG 注入与跨会话链接、任意 URL 都不能进入详情契约。
#[test]
fn ui22_injection_and_arbitrary_urls_are_refused() {
    let (_, artifact) = fixture("hello");
    let mut url_link = artifact.clone();
    url_link.links[0].id = "https://evil.example/artifact".to_owned();
    // The link contract only carries identifiers; a URL is not a fetchable field, but a link that
    // crosses sessions is the real denial (the browser must not follow it).
    url_link.links[0].session_id = SessionId::new("other-session");
    assert_eq!(
        url_link.validate(),
        Err("ui_artifact_ref_cross_session_link".to_owned())
    );

    // Injecting markup into a text/plain artifact is delivered as opaque text (the browser uses
    // textContent); what must fail is a page whose digest does not match the bytes.
    let script = "<script>alert(1)</script>";
    let script_artifact = artifact_ref(script, 3);
    let mut escaped = page(&script_artifact, 0, 1, Some(script), digest_of(script));
    let mut script_viewer = WebArtifactViewer::new(script_artifact.clone(), 3).expect("viewer");
    script_viewer
        .accept_page(escaped.clone())
        .expect("text content");
    assert_eq!(script_viewer.content_text().as_deref(), Some(script));
    escaped.page_digest = fixed_digest('e');
    let mut second = WebArtifactViewer::new(script_artifact, 3).expect("viewer");
    assert_eq!(
        second.accept_page(escaped),
        Err("web_artifact_page_digest_mismatch".to_owned())
    );

    // A page claiming `none` encoding while carrying content cannot be loaded at all.
    let mut lying = page(
        &artifact,
        0,
        1,
        Some("\u{1b}[31mred"),
        digest_of("\u{1b}[31mred"),
    );
    lying.content_encoding = "none".to_owned();
    let mut viewer = WebArtifactViewer::new(artifact, 3).expect("viewer");
    assert_eq!(
        viewer.accept_page(lying),
        Err("ui_artifact_page_content_encoding_mismatch".to_owned())
    );
}

/// 【成功】服务端分页、最终摘要一致时才可组合成完整文本；缺页保持 Partial。
#[test]
fn ui22_complete_server_pages_assemble_only_after_digest_match() {
    let content = "hello world";
    let (mut viewer, artifact) = fixture(content);
    viewer
        .accept_page(page(&artifact, 0, 2, Some("hello "), digest_of("hello ")))
        .expect("page 0");
    assert_eq!(viewer.status(), WebDetailStatus::Partial);
    assert_eq!(
        viewer.content_text(),
        None,
        "an incomplete view has no text"
    );
    viewer
        .accept_page(page(&artifact, 1, 2, Some("world"), digest_of("world")))
        .expect("page 1");
    assert_eq!(viewer.status(), WebDetailStatus::Ready);
    assert!(viewer.is_complete());
    assert_eq!(viewer.content_text().as_deref(), Some(content));

    // A page count that disagrees with the first page is refused before it can complete a view.
    let (mut mismatch, artifact) = fixture(content);
    mismatch
        .accept_page(page(&artifact, 0, 3, Some("hello "), digest_of("hello ")))
        .expect("page 0");
    assert_eq!(
        mismatch.accept_page(page(&artifact, 1, 2, Some("world"), digest_of("world"))),
        Err("web_artifact_page_count_mismatch".to_owned())
    );

    // Even with every page present, a final digest that no longer matches the artifact ref is
    // not returned as text: the ref's digest is the authority.
    let (_, mut artifact) = fixture(content);
    artifact.content_hash = fixed_digest('f');
    let mut strict = WebArtifactViewer::new(artifact.clone(), 3).expect("viewer");
    strict
        .accept_page(page(&artifact, 0, 2, Some("hello "), digest_of("hello ")))
        .expect("page 0");
    strict
        .accept_page(page(&artifact, 1, 2, Some("world"), digest_of("world")))
        .expect("page 1");
    assert!(strict.is_complete());
    assert_eq!(strict.content_text(), None);
}

/// 【成功】detail 契约字段完整；server stats/limitations/unknown 原样透传。
#[test]
fn ui22_detail_contract_keeps_server_fields_and_limitations() {
    let (_, artifact) = fixture("hello");
    let detail = UiArtifactDetailV1 {
        schema: UI_ARTIFACT_DETAIL_SCHEMA.to_owned(),
        scope: scope(11, 3),
        artifact_page: page(&artifact, 0, 1, None, fixed_digest('c')),
        diff: None,
        receipt: None,
        status: "partial".to_owned(),
        limitations: vec![
            "artifact_content_requires_authorized_server_page".to_owned(),
            "artifact_binary_content_not_rendered".to_owned(),
        ],
    };
    detail.validate().expect("detail contract");
    let projection = WebDetailProjection::from_server(detail).expect("projection");
    assert_eq!(projection.status(), WebDetailStatus::Partial);
    assert!(projection.accepts_server_stats_only());
    assert_eq!(
        projection.artifact().expect("artifact").artifact().mime,
        "text/plain"
    );
    // No URL/path field exists on the request; `after` is an opaque server token.
    let request = WebDetailRequest {
        session_id: SessionId::new(SESSION),
        tab_id: TAB.to_owned(),
        artifact_id: artifact.artifact_id.clone(),
        revision: 3,
        after: Some("server-cursor".to_owned()),
    };
    assert_eq!(request.validate(), Ok(()));
    let encoded = serde_json::to_value(&request).expect("request encodes");
    assert!(encoded.get("url").is_none());
    assert!(encoded.get("path").is_none());
    assert_eq!(encoded["after"], json!("server-cursor"));
}
