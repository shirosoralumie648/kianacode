//! Pure Web tab/session state contracts used by presenters.
//!
//! These values deliberately stay client-local.  A tab may keep a draft and a stable submission
//! key, but it cannot manufacture the authenticated principal, owner lease, permission or feed
//! cursor that the server returns and rechecks.

use kiana_domain::{RequestId, SessionId};
use kiana_protocol::{UiActionSubmissionV1, UiTabSessionV1};
use serde::{Deserialize, Serialize};

pub const WEB_CLIENT_TAB_SCHEMA: &str = "kiana.web-client-tab.v1";
pub const WEB_CLIENT_DRAFT_SCHEMA: &str = "kiana.web-client-draft.v1";
pub const WEB_CLIENT_SUBMISSION_SCHEMA: &str = "kiana.web-client-submission.v1";
pub const WEB_CLIENT_MAX_DRAFT_BYTES: usize = 64 * 1024;
/// Versioned conflict-copy catalogue for the multi-tab Web surface.
pub const WEB_TAB_CONFLICT_COPY_SCHEMA: &str = "kiana.web-tab-conflicts.v1";
pub const WEB_TAB_CONFLICT_MAX_REASONS: usize = 8;

fn required(value: &str, field: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max || value.contains('\0') {
        Err(format!("{field}_invalid"))
    } else {
        Ok(())
    }
}

/// One tab's local scope.  `server_lease` is optional before bootstrap and is never accepted as
/// authority for a mutation; it is only the last server projection seen by this client.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientTab {
    pub schema: String,
    pub tab_id: String,
    pub session_id: SessionId,
    #[serde(default)]
    pub server_lease: Option<UiTabSessionV1>,
}

impl WebClientTab {
    pub fn new(tab_id: impl Into<String>, session_id: impl Into<String>) -> Result<Self, String> {
        let tab = Self {
            schema: WEB_CLIENT_TAB_SCHEMA.to_owned(),
            tab_id: tab_id.into(),
            session_id: SessionId::new(session_id),
            server_lease: None,
        };
        tab.validate()?;
        Ok(tab)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_CLIENT_TAB_SCHEMA {
            return Err("web_client_tab_schema_invalid".to_owned());
        }
        required(&self.tab_id, "web_client_tab_id", 256)?;
        required(self.session_id.as_str(), "web_client_session_id", 256)?;
        if let Some(lease) = &self.server_lease {
            lease.validate()?;
            if lease.tab_id != self.tab_id || lease.session_id != self.session_id {
                return Err("web_client_tab_lease_scope_mismatch".to_owned());
            }
        }
        Ok(())
    }

    pub fn can_mutate(&self) -> bool {
        self.server_lease.as_ref().is_some_and(|lease| {
            lease.disposition == kiana_protocol::UiTabSessionDisposition::Owner && !lease.feed_only
        })
    }
}

/// Draft text is keyed by this tab/session pair and is never sent as an implicit command.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientDraft {
    pub schema: String,
    pub tab_id: String,
    pub session_id: SessionId,
    pub text: String,
    pub revision: u64,
}

impl WebClientDraft {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_CLIENT_DRAFT_SCHEMA || self.revision == 0 {
            return Err("web_client_draft_header_invalid".to_owned());
        }
        required(&self.tab_id, "web_client_draft_tab", 256)?;
        required(self.session_id.as_str(), "web_client_draft_session", 256)?;
        if self.text.len() > WEB_CLIENT_MAX_DRAFT_BYTES {
            return Err("web_client_draft_too_large".to_owned());
        }
        Ok(())
    }
}

/// Stable submission identity used for double-click/reconnect reconciliation.  The client must
/// reuse it for a retry and display the original server result; it must not synthesize a second
/// command ID after an unknown response.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebClientSubmission {
    pub schema: String,
    pub command_id: RequestId,
    pub idempotency_key: String,
    pub tab_id: String,
    pub session_id: SessionId,
    pub expected_revision: Option<u64>,
    pub submitted_at_unix_ms: u64,
}

impl WebClientSubmission {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != WEB_CLIENT_SUBMISSION_SCHEMA
            || self.command_id.as_uuid().is_nil()
            || self.expected_revision == Some(0)
            || self.submitted_at_unix_ms == 0
        {
            return Err("web_client_submission_header_invalid".to_owned());
        }
        required(
            &self.idempotency_key,
            "web_client_submission_idempotency",
            256,
        )?;
        required(&self.tab_id, "web_client_submission_tab", 256)?;
        required(
            self.session_id.as_str(),
            "web_client_submission_session",
            256,
        )
    }

    pub fn protocol(
        &self,
        expected_epoch: impl Into<String>,
        expected_cursor: u64,
        payload_digest: impl Into<String>,
    ) -> UiActionSubmissionV1 {
        UiActionSubmissionV1 {
            schema: kiana_protocol::UI_ACTION_SUBMISSION_SCHEMA.to_owned(),
            command_id: self.command_id,
            idempotency_key: self.idempotency_key.clone(),
            session_id: self.session_id.clone(),
            tab_id: self.tab_id.clone(),
            expected_epoch: expected_epoch.into(),
            expected_cursor,
            expected_revision: self.expected_revision,
            payload_digest: payload_digest.into(),
            submitted_at_unix_ms: self.submitted_at_unix_ms,
        }
    }
}

/// Where a tab observed a refusal.  Presenters group copy by stage instead of inventing their own
/// wording, so the same refusal reads the same in every surface.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebTabConflictStage {
    Session,
    Submission,
}

/// The only retries a tab may describe for a refused submission.  A retry never creates a second
/// effect: it either queries the original command, refreshes the server snapshot first, or asks the
/// operator to use the owner tab.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WebTabConflictRetry {
    QueryOriginal,
    RefreshSnapshot,
    SwitchToOwnerTab,
    DoNotRetry,
}

/// Bounded user-visible copy for one stable refusal code.  This is a display contract only: it
/// cannot grant a lease, retry a command or decide that an unknown result succeeded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WebTabConflictCopy {
    pub code: &'static str,
    pub stage: WebTabConflictStage,
    pub retry: WebTabConflictRetry,
    pub message: &'static str,
    pub recovery: &'static str,
}

impl WebTabConflictCopy {
    pub fn validate(&self) -> Result<(), String> {
        required(self.code, "web_tab_conflict_code", 128)?;
        required(self.message, "web_tab_conflict_message", 256)?;
        required(self.recovery, "web_tab_conflict_recovery", 512)?;
        match self.retry {
            WebTabConflictRetry::QueryOriginal if !self.recovery.contains("原 command") => Err(
                "web_tab_conflict_recovery_unbound: query_original needs the original command"
                    .to_owned(),
            ),
            WebTabConflictRetry::RefreshSnapshot if !self.recovery.contains("刷新") => Err(
                "web_tab_conflict_recovery_unbound: refresh_snapshot needs a refresh step".to_owned(),
            ),
            WebTabConflictRetry::SwitchToOwnerTab if !self.recovery.contains("owner tab") => Err(
                "web_tab_conflict_recovery_unbound: switch_to_owner_tab needs the owner tab"
                    .to_owned(),
            ),
            WebTabConflictRetry::DoNotRetry if self.recovery.contains("自动") => Err(
                "web_tab_conflict_recovery_unbound: do_not_retry must never suggest an automatic retry"
                    .to_owned(),
            ),
            _ => Ok(()),
        }
    }
}

/// Every refusal a tab can observe while it owns or observes a session, with the copy a presenter
/// shows.  A code that is missing here is rendered as an unknown refusal and never as success.
pub const WEB_TAB_CONFLICT_COPY: &[WebTabConflictCopy] = &[
    WebTabConflictCopy {
        code: "session_owner_required",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::SwitchToOwnerTab,
        message: "此 tab 只观察该 session，不能提交变更。",
        recovery: "请在 session owner tab 中提交这条操作；观察 tab 只能读同一 feed。",
    },
    WebTabConflictCopy {
        code: "session_principal_mismatch",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "该 session 属于另一个主体，本 tab 无权读写。",
        recovery: "换回自己的 session 或工作区；本 tab 不会借用其它主体的租约。",
    },
    WebTabConflictCopy {
        code: "session_unknown",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "服务端不认识这个 session。",
        recovery: "从服务端 session 列表里重新选择；不要凭本地缓存猜 session。",
    },
    WebTabConflictCopy {
        code: "session_read_only",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "历史 session 只读，不能直接继续执行。",
        recovery: "先显式恢复该 session，再在新 turn 里提交；本 tab 不会隐式 resume。",
    },
    WebTabConflictCopy {
        code: "web_auth_required",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::RefreshSnapshot,
        message: "当前凭据已失效（例如服务端轮换了 token）。",
        recovery: "刷新页面重新获取凭据后再提交；不要重放旧 tab 里的请求。",
    },
    WebTabConflictCopy {
        code: "web_tab_required",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "请求缺少 tab 作用域。",
        recovery: "用页面生成的 tab 标识重试；服务端不接受无作用域的变更请求。",
    },
    WebTabConflictCopy {
        code: "web_tab_invalid",
        stage: WebTabConflictStage::Session,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "tab 标识非法。",
        recovery: "重新加载页面以获得新的 tab 标识；不要伪造 tab id。",
    },
    WebTabConflictCopy {
        code: "ui_action_required",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "这次提交没有动作身份，无法与重复点击区分。",
        recovery: "由页面用稳定的 action id 重新提交；服务端不会为无身份请求创建事实。",
    },
    WebTabConflictCopy {
        code: "ui_action_invalid",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "动作身份或期望位置不可用。",
        recovery: "重新从服务端快照取值后再提交；本地构造的位置不是协商标识。",
    },
    WebTabConflictCopy {
        code: "ui_action_owner_mismatch",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "同一个 action id 已被别的 tab 或主体使用。",
        recovery: "新 tab 必须使用自己的 action id；原 command 的结果只回给提交它的 tab。",
    },
    WebTabConflictCopy {
        code: "ui_action_payload_mismatch",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "同一个 action id 带着不同的内容提交。",
        recovery: "内容变了就必须换新的 action id；原 command 的结果不能替代新的意图。",
    },
    WebTabConflictCopy {
        code: "ui_action_in_flight",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::QueryOriginal,
        message: "上一次提交还在处理中。",
        recovery: "等待并查询原 command 的状态；不要用新 action id 再发一次。",
    },
    WebTabConflictCopy {
        code: "ui_action_submission_expired",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::QueryOriginal,
        message: "这次提交的记录已过期。",
        recovery: "查询原 command 的最终结果；如果它从未产生事实，再由操作者决定是否重新提交。",
    },
    WebTabConflictCopy {
        code: "ui_action_target_mismatch",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::DoNotRetry,
        message: "动作目标和 session 不一致。",
        recovery: "只对当前 session 提交动作；跨 session 的目标不会被执行。",
    },
    WebTabConflictCopy {
        code: "ui_action_stale",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::RefreshSnapshot,
        message: "页面看到的投影已经过期（stale card）。",
        recovery: "刷新服务端快照后，用新的 action id 重新提交这条操作。",
    },
    WebTabConflictCopy {
        code: "ui_action_replayed",
        stage: WebTabConflictStage::Submission,
        retry: WebTabConflictRetry::QueryOriginal,
        message: "这个 action id 已经被服务端受理过。",
        recovery: "查询原 command 的结果，并显示当时返回的同一份回执。",
    },
];

/// Look up the reviewed copy for a refusal code.  An unknown code returns `None`, which means
/// "display an unknown refusal", never "assume success".
pub fn web_tab_conflict_copy(code: &str) -> Option<&'static WebTabConflictCopy> {
    WEB_TAB_CONFLICT_COPY
        .iter()
        .find(|entry| entry.code == code)
}

/// Every code the catalogue explains, in declaration order.
pub fn web_tab_conflict_codes() -> Vec<&'static str> {
    WEB_TAB_CONFLICT_COPY
        .iter()
        .map(|entry| entry.code)
        .collect()
}
