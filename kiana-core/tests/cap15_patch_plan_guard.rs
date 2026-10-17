//! CAP-15 source guard for one typed patch plan shared by preview and commit.

#[path = "support/source_guard.rs"]
mod source_guard;

fn require(source: &str, markers: &[&str], label: &str) {
    for marker in markers {
        assert!(
            source.contains(marker),
            "CAP-15 {label} marker missing: {marker}"
        );
    }
}

#[test]
fn patch_preview_and_commit_share_parser_overlay_preconditions_and_digest() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");
    let baseline = include_str!("../../docs/roadmap/capability.md");
    require(
        patch,
        &[
            "preview_codex_patch",
            "apply_codex_patch",
            "parse_patch",
            "plan_hunks",
            "capture_preconditions",
            "verify_preconditions",
            "patch_preview",
            "patch_digest",
            "affected_paths",
            "before_hashes",
            "hunk_count",
            "MAX_PATCH_BYTES",
            "MAX_PATCH_HUNKS",
            "patch_affected_paths",
            "read_only",
        ],
        "shared patch plan",
    );
    require(
        harness,
        &[
            "apply_patch.preview",
            "ApplyPatchPreviewHandler",
            "prepare_patch_request",
            "preview_codex_patch",
            "patch_affected_paths",
            "AdapterCommitState::NotStarted",
        ],
        "preview capability",
    );
    require(
        baseline,
        &[
            "patch_preview_and_commit_use_identical_target_set",
            "malformed_late_hunk_makes_no_earlier_change",
            "move_source_and_target_both_require_authority",
        ],
        "CAP-15 card",
    );
    assert!(!patch.contains("parse_patch(patch)?;\n    let _lock"));
}

#[test]
fn patch_plan_never_turns_preview_into_a_write_path() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let preview = source_guard::item_source(patch, "fn preview_codex_patch(");
    let projection = source_guard::item_source(patch, "fn patch_preview(");
    assert!(projection.contains("read_only"));
    assert!(!preview.contains("ProjectPatchLock::acquire"));
    assert!(!preview.contains("commit_planned"));
}

#[test]
fn patch_scope_validation_reuses_the_typed_parse_instead_of_rescanning_lines() {
    let patch = include_str!("../../kiana-daemon/src/apply_patch.rs");
    let harness = include_str!("../../kiana-daemon/src/harness_capabilities.rs");

    // 唯一 parser 产出 typed 受影响路径；move 目标必须一并返回，scope 层不能只信源路径。
    let targets = source_guard::item_source(patch, "pub(crate) fn patch_affected_paths(");
    assert!(targets.contains("parse_patch(patch)?"));
    assert!(targets.contains("Hunk::UpdateFile"));
    assert!(targets.contains("move_path"));
    assert!(targets.contains("Hunk::AddFile"));
    assert!(targets.contains("Hunk::DeleteFile"));

    // 请求校验必须消费同一份 typed 路径，而不是自己逐行扫描 patch 文本。
    let prepare = source_guard::item_source(harness, "fn prepare_patch_request(");
    assert!(prepare.contains("crate::apply_patch::patch_affected_paths"));
    assert!(
        !prepare.contains("patch.lines()"),
        "CAP-15 scope validation must consume the shared typed parser, not rescan patch lines"
    );
    assert!(
        !prepare.contains("*** Add File: "),
        "CAP-15 scope validation must not re-declare patch header prefixes"
    );
}
