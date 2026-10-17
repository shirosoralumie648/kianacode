//! UI-04 daemon adapter fixture：版本化界面意图 -> Domain 命令 -> ControlPlane journal。
//!
//! 这些用例覆盖组合根必须自己守住的那条边界：
//!
//! ```text
//! UiActionV1（客户端自报身份/摘要）
//!     -> command_from_surface_intent：绑定 daemon 已认证主体 + 服务端 scope/permit，重算摘要
//!     -> DaemonHost::admit_ui_action：durable CAS 准入
//!     -> DaemonHost::query_original_ui_action：按原键 + 本主体 owner + scope 对账
//! ```
//!
//! 这里不执行任何 UI effect、不调用模型；DaemonHost 使用内存 EventLog，因此证明上限是
//! source/local 行为，不是 live 界面路径。

use kiana_core::UiActionAuthoritySnapshot;
use kiana_daemon::{command_from_surface_intent, DaemonHost, ProjectTrustAuthority};
use kiana_domain::{json_digest, RequestId, UiActionState};
use kiana_protocol::{UiActionV1, UI_ACTION_SCHEMA};
use kiana_runner::{KianaHarness, UnavailableModel};
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

/// 组合根装配时使用的项目信任判定；这些用例不加载任何项目本地资源，固定信任即可。
struct TrustedProject;

impl ProjectTrustAuthority for TrustedProject {
    fn project_trusted(&self, _project_root: &Path) -> Result<bool, String> {
        Ok(true)
    }
}

fn host() -> DaemonHost {
    DaemonHost::with_harness_and_project_authority(
        KianaHarness::new(Arc::new(UnavailableModel)),
        Arc::new(TrustedProject),
    )
    .expect("daemon host")
}

fn scope_digest() -> String {
    json_digest(&json!({"scope":"run-1"}))
}

/// daemon 本地主体的 owner 与 cursor 固定为 7；与 [`intent`] 的 expected 值保持一致。
fn authority() -> UiActionAuthoritySnapshot {
    UiActionAuthoritySnapshot {
        authority_epoch: "epoch-1".to_owned(),
        cursor: 7,
        target_revision: Some(4),
        owner_id: "local-user".to_owned(),
        scope_digest: scope_digest(),
        permit_digest: Some(json_digest(&json!({"permit":"ui-04-adapter"}))),
        cancelled: false,
    }
}

fn intent(command_id: RequestId, key: &str, submitted_by: &str, payload: Value) -> UiActionV1 {
    UiActionV1 {
        schema: UI_ACTION_SCHEMA.to_owned(),
        command_id,
        idempotency_key: key.to_owned(),
        target_id: "run-target".to_owned(),
        expected_epoch: "epoch-1".to_owned(),
        expected_cursor: 7,
        expected_revision: Some(4),
        payload_digest: json_digest(&payload),
        payload,
        submitted_by: submitted_by.to_owned(),
        deadline_unix_ms: Some(10_000),
    }
}

#[test]
fn adapter_binds_server_owned_owner_scope_permit_and_recomputes_digests() {
    let intent = intent(
        RequestId::new(),
        "ui-04-adapter",
        "local-user",
        json!({"decision":"approve"}),
    );
    let authority = authority();
    let command = command_from_surface_intent(&intent, "local-user", &authority).expect("adapter");
    assert_eq!(command.owner_id, "local-user");
    assert_eq!(command.scope_digest, authority.scope_digest);
    assert_eq!(command.expected_cursor, 7);
    assert_eq!(command.deadline_unix_ms, 10_000);
    // 命令身份摘要由服务端重算，并与 domain validator 自己的规则一致。
    command.validate().expect("adapter output validates");

    // 客户端自报 submitted_by 与已认证主体不一致时必须拒绝，而不是静默替换成服务端身份。
    let error = command_from_surface_intent(&intent, "other-user", &authority).unwrap_err();
    assert_eq!(error, "ui_action_submitter_mismatch");
}

#[test]
fn adapter_refuses_missing_permit_foreign_owner_missing_deadline_and_forged_digest() {
    let good = intent(
        RequestId::new(),
        "ui-04-adapter-deny",
        "local-user",
        json!({"decision":"approve"}),
    );

    let mut no_permit = authority();
    no_permit.permit_digest = None;
    assert_eq!(
        command_from_surface_intent(&good, "local-user", &no_permit).unwrap_err(),
        "ui_action_permit_required"
    );

    let mut foreign_owner = authority();
    foreign_owner.owner_id = "other-owner".to_owned();
    assert_eq!(
        command_from_surface_intent(&good, "local-user", &foreign_owner).unwrap_err(),
        "ui_action_owner_mismatch"
    );

    let mut no_deadline = good.clone();
    no_deadline.deadline_unix_ms = None;
    assert_eq!(
        command_from_surface_intent(&no_deadline, "local-user", &authority()).unwrap_err(),
        "ui_action_deadline_required"
    );

    let mut forged = good;
    forged.payload_digest = json_digest(&json!({"decision":"deny"}));
    assert_eq!(
        command_from_surface_intent(&forged, "local-user", &authority()).unwrap_err(),
        "ui_action_payload_digest_mismatch"
    );
}

#[tokio::test]
async fn daemon_host_admits_applies_and_reconciles_the_original_record() {
    let host = host();
    let authority = authority();
    let intent = intent(
        RequestId::new(),
        "ui-04-host",
        "local-user",
        json!({"decision":"approve"}),
    );
    let command = host
        .ui_action_command(&intent, &authority)
        .expect("translate surface intent");

    let accepted = host
        .admit_ui_action(&command, &authority, 1_000)
        .await
        .expect("admission");
    assert_eq!(accepted.state, UiActionState::Accepted);

    let duplicate = host
        .admit_ui_action(&command, &authority, 1_100)
        .await
        .expect("duplicate submission returns the original record");
    assert_eq!(duplicate, accepted);

    let applied = host
        .apply_ui_action(&command, &json_digest(&json!({"receipt":"host"})), 1_200)
        .await
        .expect("apply");
    assert_eq!(applied.state, UiActionState::Applied);
    assert_eq!(applied.effect_count, 1);

    let queried = host
        .query_original_ui_action(&command.idempotency_key, &authority.scope_digest)
        .await
        .expect("query")
        .expect("record");
    assert_eq!(queried, applied);

    // 同一 daemon 上的另一个 scope 不能读到这条记录。
    let error = host
        .query_original_ui_action(
            &command.idempotency_key,
            &json_digest(&json!({"scope":"run-2"})),
        )
        .await
        .expect_err("foreign scope query refused");
    assert!(
        error.to_string().contains("ui_action_query_scope_mismatch"),
        "unexpected error: {error}"
    );
}
