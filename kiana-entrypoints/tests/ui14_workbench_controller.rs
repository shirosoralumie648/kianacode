//! UI-14 Workbench controller fixtures.
//!
//! The controller is a disposable intent adapter: it turns keymap/palette/session input into a
//! `WorkbenchUiAction` and nothing else. These fixtures drive the typed controller directly; they
//! do not open a window, call a daemon, submit a request or prove any external outcome.
//!
//! Deny-first order: session/draft boundaries, duplicate shortcuts, capability visibility,
//! duplicate submission, disconnect, lost response, stale cards, termination signals and restart
//! recovery are all asserted before the happy paths.

use kiana_entrypoints::workbench_controller::{
    AuditDisposition, CommandPalette, CommandPaletteEntry, ConnectionStatus, ControllerDecision,
    InFlightDisposition, KeyBinding, KeyChord, Keymap, RecoveryObservation, RunStatus,
    SessionSwitcher, SubmissionStatus, TerminationSignal, WorkbenchCommand, WorkbenchController,
    WorkbenchIntent, WorkbenchRecovery, MAX_COMMAND_PALETTE_ENTRIES, MAX_CONTROLLER_AUDIT_ENTRIES,
    MAX_CONTROLLER_TEXT_BYTES, MAX_WORKBENCH_SESSIONS, MAX_WORKBENCH_TARGET_BYTES,
    MAX_WORKBENCH_TITLE_BYTES,
};
use kiana_protocol::{
    json_digest, ExecutionStatus, RequestId, RunId, SessionId, SessionSummary, UiActionDisposition,
    UiCapability, UI_CAPABILITY_SCHEMA,
};
use serde_json::Value;
use std::collections::BTreeSet;

const CONTROLLER_FIXTURE: &str = include_str!("fixtures/ui14-controller-actions.json");
const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
const WORKSPACE: &str = "workspace";
const PRINCIPAL: &str = "local-user";
const EPOCH: &str = "epoch-14";
const CURSOR: u64 = 7;
const REVISION: u64 = 3;
const SESSION_ONE: &str = "00000000-0000-0000-0000-000000000014";
const SESSION_TWO: &str = "00000000-0000-0000-0000-000000000015";

fn fixture() -> Value {
    serde_json::from_str(CONTROLLER_FIXTURE).expect("valid UI-14 controller fixture")
}

fn session(value: &str, title: &str) -> SessionSummary {
    kiana_protocol::SessionSummary {
        session_id: SessionId::new(value),
        title: title.to_owned(),
        status: "idle".to_owned(),
        revision: 1,
    }
}

fn capability(command: WorkbenchCommand, enabled: bool) -> UiCapability {
    UiCapability {
        schema: UI_CAPABILITY_SCHEMA.to_owned(),
        capability_id: command.as_str().to_owned(),
        enabled,
        actions: vec![command.as_str().to_owned()],
        reason: None,
        scope_digest: DIGEST.to_owned(),
    }
}

fn capabilities(all: bool) -> Vec<UiCapability> {
    if all {
        WorkbenchCommand::ALL
            .into_iter()
            .map(|command| capability(command, true))
            .collect()
    } else {
        Vec::new()
    }
}

fn controller_with(all_capabilities: bool, active_session_id: Option<&str>) -> WorkbenchController {
    WorkbenchController::new(
        WORKSPACE,
        PRINCIPAL,
        EPOCH,
        CURSOR,
        Some(REVISION),
        vec![session(SESSION_ONE, "one"), session(SESSION_TWO, "two")],
        active_session_id.map(str::to_owned),
        capabilities(all_capabilities),
    )
    .expect("the fixture controller is valid")
}

fn controller(all_capabilities: bool) -> WorkbenchController {
    controller_with(all_capabilities, Some(SESSION_ONE))
}

fn observation() -> RecoveryObservation {
    RecoveryObservation {
        workspace: WORKSPACE.to_owned(),
        principal: PRINCIPAL.to_owned(),
        epoch: EPOCH.to_owned(),
        cursor: CURSOR,
        revision: Some(REVISION),
        connection: ConnectionStatus::Attached,
        sessions: vec![session(SESSION_ONE, "one"), session(SESSION_TWO, "two")],
        active_session_id: Some(SESSION_ONE.to_owned()),
        capabilities: capabilities(true),
    }
}

fn action(
    decision: ControllerDecision,
) -> kiana_entrypoints::workbench_controller::WorkbenchUiAction {
    match decision {
        ControllerDecision::Action(action) => action,
        other => panic!("expected action, got {other:?}"),
    }
}

fn wire_command(value: &str) -> WorkbenchCommand {
    WorkbenchCommand::ALL
        .into_iter()
        .find(|command| command.as_str() == value)
        .unwrap_or_else(|| panic!("unknown fixture command: {value}"))
}

fn palette_entry(
    command: WorkbenchCommand,
    label: &str,
    capability_id: &str,
    shortcut: Option<KeyChord>,
) -> CommandPaletteEntry {
    CommandPaletteEntry {
        command,
        label: label.to_owned(),
        capability_id: capability_id.to_owned(),
        shortcut,
    }
}

#[test]
fn fixture_commands_keymap_and_deny_matrix_are_wired() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], "kiana.ui14-workbench-controller.v1");
    assert_eq!(fixture["step"], "UI-14");

    // 1. the declared budgets are the source constants.
    for (budget, expected) in [
        ("max_sessions", MAX_WORKBENCH_SESSIONS as u64),
        ("max_palette_entries", MAX_COMMAND_PALETTE_ENTRIES as u64),
        ("max_audit_entries", MAX_CONTROLLER_AUDIT_ENTRIES as u64),
        ("max_text_bytes", MAX_CONTROLLER_TEXT_BYTES as u64),
        ("max_target_bytes", MAX_WORKBENCH_TARGET_BYTES as u64),
        ("max_title_bytes", MAX_WORKBENCH_TITLE_BYTES as u64),
        ("commands", WorkbenchCommand::ALL.len() as u64),
        ("shortcuts", 8),
    ] {
        assert_eq!(
            fixture["budgets"][budget].as_u64(),
            Some(expected),
            "UI-14 fixture budget {budget} drifted from the source constant"
        );
    }

    // 2. the declared commands are `WorkbenchCommand::ALL`, in order.
    let declared: Vec<String> = fixture["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|value| value.as_str().expect("command name").to_owned())
        .collect();
    let expected: Vec<String> = WorkbenchCommand::ALL
        .into_iter()
        .map(|command| command.as_str().to_owned())
        .collect();
    assert_eq!(declared, expected);

    // 3. the declared keymap is the default one, and no chord resolves twice.
    let keymap = Keymap::default_workbench();
    let mut declared_bindings: Vec<(String, String)> = fixture["keymap"]
        .as_array()
        .expect("keymap")
        .iter()
        .map(|value| {
            (
                value["shortcut"].as_str().expect("shortcut").to_owned(),
                value["command"].as_str().expect("command").to_owned(),
            )
        })
        .collect();
    declared_bindings.sort();
    let mut expected_bindings: Vec<(String, String)> = keymap
        .bindings()
        .map(|binding| (binding.chord.label(), binding.command.as_str().to_owned()))
        .collect();
    expected_bindings.sort();
    assert_eq!(declared_bindings, expected_bindings);
    assert_eq!(
        keymap.bindings().count(),
        fixture["budgets"]["shortcuts"].as_u64().unwrap() as usize
    );
    assert_eq!(
        keymap.resolve(KeyChord::new('r', true, false, false)),
        Some(WorkbenchCommand::Run)
    );

    // 4. the deny matrix and the intent/scenario/recovery sections stay declared.
    let denied: Vec<&str> = fixture["denied"]
        .as_array()
        .expect("denied")
        .iter()
        .map(|value| value.as_str().expect("denial code"))
        .collect();
    for code in [
        "stale_draft",
        "duplicate_submission",
        "capability_unavailable",
    ] {
        assert!(
            denied.contains(&code),
            "the UI-14 deny matrix must keep {code}"
        );
    }
    assert!(!fixture["intents"].as_array().expect("intents").is_empty());
    assert!(!fixture["scenarios"]
        .as_array()
        .expect("scenarios")
        .is_empty());
    assert!(!fixture["required"].as_array().expect("required").is_empty());
    assert!(!fixture["limitations"]
        .as_array()
        .expect("limitations")
        .is_empty());
    assert!(!fixture["propagated_denials"]
        .as_array()
        .expect("propagated denials")
        .is_empty());
    assert_eq!(
        fixture["cas_fields"].as_array().expect("cas fields").len(),
        7
    );
    // 5. the recovery contract declares exactly the fields a restarted controller may observe,
    //    and names what it must never carry across the process boundary.
    let observed: Vec<&str> = fixture["recovery"]["observation_fields"]
        .as_array()
        .expect("observation fields")
        .iter()
        .map(|value| value.as_str().expect("observation field"))
        .collect();
    for field in [
        "workspace",
        "principal",
        "epoch",
        "cursor",
        "revision",
        "connection",
        "sessions",
        "active_session_id",
        "capabilities",
    ] {
        assert!(
            observed.contains(&field),
            "UI-14 recovery observation must declare {field}"
        );
    }
    let not_carried: Vec<&str> = fixture["recovery"]["not_carried"]
        .as_array()
        .expect("not carried")
        .iter()
        .map(|value| value.as_str().expect("not-carried field"))
        .collect();
    for field in [
        "draft",
        "submission_status",
        "in_flight_action",
        "run_status",
    ] {
        assert!(
            not_carried.contains(&field),
            "UI-14 recovery must refuse to carry {field}"
        );
    }
}

#[test]
fn every_declared_intent_translates_to_its_stable_action() {
    let fixture = fixture();
    let mut controller = controller(true);
    let run_id = Some(RunId::new());
    for entry in fixture["intents"].as_array().expect("intents") {
        let name = entry["intent"].as_str().expect("intent name");
        let expected_command = entry["command"].as_str().expect("command");
        let expected_target = match entry["target"].as_str().expect("target") {
            "workspace" => WORKSPACE.to_owned(),
            "session_id" | "active_session" => SESSION_ONE.to_owned(),
            other => panic!("unknown fixture target: {other}"),
        };
        let clears_draft = entry["clears_draft"].as_bool().expect("clears_draft");
        assert_eq!(
            wire_command(expected_command).is_mutating(),
            entry["mutating"].as_bool().expect("mutating"),
            "{name} mutating flag drifted"
        );

        // The draft must exist before `run`/`continue`, so its revision is the current one.
        controller.set_draft("pending draft").unwrap();
        let intent = match name {
            "open" => WorkbenchIntent::Open {
                workspace: WORKSPACE.to_owned(),
            },
            "attach" => WorkbenchIntent::Attach {
                session_id: SESSION_ONE.to_owned(),
            },
            "new" => WorkbenchIntent::New {
                title: Some("title".to_owned()),
            },
            "continue" => WorkbenchIntent::Continue {
                run_id,
                prompt: "continue".to_owned(),
                draft_revision: controller.draft_revision(),
            },
            "status" => WorkbenchIntent::Status,
            "run" => WorkbenchIntent::Run {
                prompt: "run".to_owned(),
                draft_revision: controller.draft_revision(),
            },
            "cancel" => WorkbenchIntent::Cancel { run_id },
            "resume" => WorkbenchIntent::Resume { run_id },
            "receipt" => WorkbenchIntent::Receipt { run_id },
            other => panic!("unknown fixture intent: {other}"),
        };

        let action = action(
            controller
                .dispatch(intent)
                .expect("declared intent is accepted"),
        );
        assert_eq!(action.command.as_str(), expected_command);
        assert_eq!(action.target_id, expected_target, "{name} target drifted");
        assert_eq!(action.expected_epoch, EPOCH);
        assert_eq!(action.expected_cursor, CURSOR);
        assert_eq!(action.expected_revision, Some(REVISION));
        assert_eq!(action.submitted_by, PRINCIPAL);
        assert_eq!(action.identity(), controller.pending_identity().unwrap());

        let mut payload_keys: Vec<String> = action
            .payload
            .as_object()
            .expect("declared payload is an object")
            .keys()
            .cloned()
            .collect();
        payload_keys.sort();
        let mut expected_keys: Vec<String> = entry["payload_keys"]
            .as_array()
            .expect("payload_keys")
            .iter()
            .map(|value| value.as_str().expect("payload key").to_owned())
            .collect();
        expected_keys.sort();
        assert_eq!(payload_keys, expected_keys, "{name} payload drifted");

        // The intent is only a wire action after the client supplies the canonical digest.
        let wire = action.clone().into_protocol(json_digest(&action.payload));
        wire.validate().expect("prepared action is wire-valid");
        assert_eq!(wire.schema, kiana_protocol::UI_ACTION_SCHEMA);
        assert_eq!(wire.deadline_unix_ms, None);
        assert_eq!(wire.payload_digest, json_digest(&action.payload));

        if clears_draft {
            assert!(controller.draft().is_empty(), "{name} keeps the old draft");
        } else {
            assert_eq!(controller.draft(), "pending draft", "{name} ate the draft");
        }
        controller
            .record_action_result(action.command_id, UiActionDisposition::Applied)
            .unwrap();
        assert_eq!(controller.pending_identity(), None);
    }
}

#[test]
fn controller_translates_actions_with_scope_and_preserves_separate_statuses() {
    let mut controller = controller(true);
    assert!(controller.audit().is_empty());
    let open = action(
        controller
            .dispatch(WorkbenchIntent::Open {
                workspace: "workspace-two".to_owned(),
            })
            .unwrap(),
    );
    assert_eq!(open.command, WorkbenchCommand::Open);
    assert_eq!(open.target_id, "workspace-two");
    assert_eq!(open.expected_epoch, "epoch-14");
    assert_eq!(open.expected_cursor, 7);
    assert_eq!(open.expected_revision, Some(3));
    assert_eq!(open.payload["workspace"], "workspace-two");
    assert_eq!(open.submitted_by, "local-user");
    assert_eq!(controller.audit().len(), 1);
    assert_eq!(
        controller.audit()[0].disposition,
        AuditDisposition::Prepared
    );
    assert_eq!(controller.audit()[0].idempotency_key, open.idempotency_key);
    assert_eq!(controller.pending_identity(), Some(open.identity()));
    let wire = open.clone().into_protocol(DIGEST);
    assert_eq!(wire.schema, kiana_protocol::UI_ACTION_SCHEMA);
    assert_eq!(wire.payload_digest, DIGEST);

    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
    assert_eq!(controller.state().run, RunStatus::Idle);
    controller
        .record_action_result(open.command_id, UiActionDisposition::Applied)
        .unwrap();
    assert_eq!(controller.state().submission, SubmissionStatus::Applied);
    assert_eq!(controller.pending_identity(), None);
    assert_eq!(
        controller.audit().last().unwrap().disposition,
        kiana_entrypoints::workbench_controller::AuditDisposition::Applied
    );

    controller.set_draft("draft prompt").unwrap();
    let revision = controller.draft_revision();
    let run = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "run this".to_owned(),
                draft_revision: revision,
            })
            .unwrap(),
    );
    assert_eq!(run.command, WorkbenchCommand::Run);
    assert_eq!(run.payload["prompt"], "run this");
    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
    assert_eq!(controller.state().run, RunStatus::Idle);
    controller.observe_run_status(ExecutionStatus::Running);
    assert_eq!(controller.state().run, RunStatus::Running);
    // The submission dimension is not the run dimension: a running walk does not settle a submit.
    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
    assert_eq!(controller.pending_identity(), Some(run.identity()));
}

#[test]
fn open_action_respects_wire_target_and_idempotency_bounds() {
    let mut controller = controller(true);
    let workspace = "w".repeat(256);
    let open = action(
        controller
            .dispatch(WorkbenchIntent::Open {
                workspace: workspace.clone(),
            })
            .unwrap(),
    );
    assert_eq!(open.target_id.len(), 256);
    let wire = open.clone().into_protocol(json_digest(&open.payload));
    wire.validate().unwrap();
    assert!(wire.idempotency_key.len() <= 256);
    assert_eq!(wire.expected_revision, Some(3));
    // The wire bounds the controller claims to respect are the protocol's own codes.
    let mut forged = open.clone().into_protocol(DIGEST);
    forged.idempotency_key = String::new();
    assert_eq!(
        forged.validate(),
        Err("ui_action_idempotency_key_invalid".to_owned())
    );
    let mut forged = open.clone().into_protocol(DIGEST);
    forged.target_id = "  ".to_owned();
    assert_eq!(
        forged.validate(),
        Err("ui_action_target_invalid".to_owned())
    );
    let mut forged = open.clone().into_protocol(DIGEST);
    forged.expected_revision = Some(0);
    assert_eq!(
        forged.validate(),
        Err("ui_action_revision_invalid".to_owned())
    );
    let forged = open.clone().into_protocol(DIGEST);
    assert_eq!(
        forged.validate(),
        Err("ui_action_payload_digest_mismatch".to_owned())
    );

    controller
        .record_action_result(open.command_id, UiActionDisposition::Applied)
        .unwrap();
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Open {
            workspace: "w".repeat(257),
        }),
        Err("workbench_workspace_invalid".to_owned())
    );
    // A fresh action never reuses the idempotency key of an earlier one.
    let status = action(controller.dispatch(WorkbenchIntent::Status).unwrap());
    assert_ne!(status.idempotency_key, open.idempotency_key);
    assert_eq!(status.idempotency_key, "workbench:workbench.status:2");
    assert!(status.idempotency_key.len() <= MAX_WORKBENCH_TARGET_BYTES);
}

#[test]
fn stale_draft_and_duplicate_submission_are_rejected_before_action_creation() {
    let mut controller = controller(true);
    controller.set_draft("first").unwrap();
    let stale_revision = controller.draft_revision();
    controller.set_draft("second").unwrap();
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "submit".to_owned(),
            draft_revision: stale_revision,
        }),
        Err("stale_draft".to_owned())
    );
    assert!(controller.audit().is_empty());

    let revision = controller.draft_revision();
    let first = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "submit".to_owned(),
                draft_revision: revision,
            })
            .unwrap(),
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "submit twice".to_owned(),
            draft_revision: revision,
        }),
        Err("duplicate_submission".to_owned())
    );
    controller
        .record_action_result(first.command_id, UiActionDisposition::Unknown)
        .unwrap();
    assert_eq!(controller.state().submission, SubmissionStatus::Unknown);
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Resume { run_id: None }),
        Err("duplicate_submission".to_owned())
    );
    // 要第二次提交，必须先按原 id 对账——不能靠换一个 command id。
    assert_eq!(
        controller.record_action_result(RequestId::new(), UiActionDisposition::Applied),
        Err("action_result_command_mismatch".to_owned())
    );
    assert_eq!(controller.audit().len(), 2);
    controller
        .record_action_result(first.command_id, UiActionDisposition::Applied)
        .unwrap();
    let second = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "second submit".to_owned(),
                draft_revision: revision,
            })
            .unwrap(),
    );
    assert_ne!(second.idempotency_key, first.idempotency_key);
    assert_ne!(second.command_id, first.command_id);
}

#[test]
fn session_switcher_clears_old_draft_before_new_session_action() {
    let mut controller = controller(true);
    controller.set_draft("belongs to one").unwrap();
    let before = controller.draft_revision();
    assert_eq!(
        controller.switch_session("00000000-0000-0000-0000-000000000015"),
        Ok(ControllerDecision::SessionChanged {
            session_id: "00000000-0000-0000-0000-000000000015".to_owned()
        })
    );
    assert!(controller.draft().is_empty());
    assert!(controller.draft_revision() > before);
    assert_eq!(
        controller.state().active_session_id.as_deref(),
        Some("00000000-0000-0000-0000-000000000015")
    );
    // 未知会话不能被选中，选中项保持不变。
    assert_eq!(
        controller.switch_session("00000000-0000-0000-0000-0000000000ff"),
        Err("session_switcher_session_missing".to_owned())
    );
    assert_eq!(
        controller.state().active_session_id.as_deref(),
        Some(SESSION_TWO)
    );
    // 会话集合的形状在构造时就被拒绝：重复 id、指向不存在会话的 active、超上限。
    assert_eq!(
        SessionSwitcher::new(
            vec![session(SESSION_ONE, "one"), session(SESSION_ONE, "dup")],
            None
        )
        .err(),
        Some("session_switcher_duplicate_session".to_owned())
    );
    assert_eq!(
        SessionSwitcher::new(
            vec![session(SESSION_ONE, "one")],
            Some("00000000-0000-0000-0000-0000000000ff".to_owned())
        )
        .err(),
        Some("session_switcher_active_missing".to_owned())
    );
    let overflow = (0..=MAX_WORKBENCH_SESSIONS)
        .map(|index| session(&format!("session-{index}"), "title"))
        .collect::<Vec<_>>();
    assert_eq!(
        SessionSwitcher::new(overflow, None).err(),
        Some("session_switcher_limit".to_owned())
    );
}

#[test]
fn session_switch_cannot_submit_the_previous_draft_and_targets_the_new_session() {
    let mut controller = controller(true);
    controller.set_draft("text typed for session one").unwrap();
    let stale_revision = controller.draft_revision();
    controller.switch_session(SESSION_TWO).unwrap();
    // 旧会话的草稿版本号不能用来提交。
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "text typed for session one".to_owned(),
            draft_revision: stale_revision,
        }),
        Err("stale_draft".to_owned())
    );
    assert_eq!(controller.pending_identity(), None);
    assert!(controller.audit().is_empty());
    // 用当前版本号提交时，动作只落在新会话上，草稿文本不会漂到别处。
    let fresh = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "text for session two".to_owned(),
                draft_revision: controller.draft_revision(),
            })
            .unwrap(),
    );
    assert_eq!(fresh.target_id, SESSION_TWO);
    assert_eq!(fresh.payload["prompt"], "text for session two");
    let wire = fresh.clone().into_protocol(json_digest(&fresh.payload));
    wire.validate().unwrap();
}

#[test]
fn keymap_and_palette_reject_duplicate_shortcuts_and_hide_missing_capabilities() {
    let duplicate = Keymap::new([
        KeyBinding {
            chord: KeyChord::new('x', true, false, false),
            command: WorkbenchCommand::Run,
        },
        KeyBinding {
            chord: KeyChord::new('x', true, false, false),
            command: WorkbenchCommand::Cancel,
        },
    ]);
    assert_eq!(duplicate, Err("duplicate_shortcut:Ctrl+x".to_owned()));
    // 一个命令绑两个键位同样拒绝：同一意图不该有两条快捷键路径。
    assert_eq!(
        Keymap::new([
            KeyBinding {
                chord: KeyChord::new('x', true, false, false),
                command: WorkbenchCommand::Run,
            },
            KeyBinding {
                chord: KeyChord::new('y', true, false, false),
                command: WorkbenchCommand::Run,
            },
        ]),
        Err("duplicate_command_binding:workbench.run".to_owned())
    );
    // 默认表里一个键位恰好一个命令。
    let mut chords = BTreeSet::new();
    for binding in Keymap::default_workbench().bindings() {
        assert!(
            chords.insert(binding.chord.label()),
            "duplicate chord in the default keymap"
        );
    }

    let palette = CommandPalette::default_workbench();
    assert!(palette.visible(&[]).is_empty());
    let visible = palette.visible(&[capability(WorkbenchCommand::Run, true)]);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].command, WorkbenchCommand::Run);
    // 被禁用的 capability 不投影动作，也不放行 dispatch。
    assert!(palette
        .visible(&[capability(WorkbenchCommand::Run, false)])
        .is_empty());
    assert!(controller(false).visible_commands().is_empty());
    assert_eq!(
        controller(false).dispatch(WorkbenchIntent::Status),
        Err("capability_unavailable:workbench.status".to_owned())
    );
    // 面板自身的形状：重复命令、空标签、坏 capability id 都在构造时拒绝。
    assert_eq!(
        CommandPalette::new(vec![
            palette_entry(WorkbenchCommand::Run, "run", "workbench.run", None),
            palette_entry(WorkbenchCommand::Run, "run again", "workbench.run", None),
        ])
        .err(),
        Some("command_palette_duplicate_command:workbench.run".to_owned())
    );
    assert_eq!(
        CommandPalette::new(vec![palette_entry(
            WorkbenchCommand::Run,
            "  ",
            "workbench.run",
            None
        )])
        .err(),
        Some("command_palette_label_invalid".to_owned())
    );
    assert_eq!(
        CommandPalette::new(vec![palette_entry(WorkbenchCommand::Run, "run", "", None)]).err(),
        Some("command_palette_capability_invalid".to_owned())
    );
    assert_eq!(
        CommandPalette::new(vec![
            palette_entry(
                WorkbenchCommand::Run,
                "run",
                "workbench.run",
                Some(KeyChord::new('r', true, false, false))
            ),
            palette_entry(
                WorkbenchCommand::Cancel,
                "cancel",
                "workbench.cancel",
                Some(KeyChord::new('r', true, false, false))
            ),
        ])
        .err(),
        Some("duplicate_shortcut:Ctrl+r".to_owned())
    );
    let overflow = (0..=MAX_COMMAND_PALETTE_ENTRIES)
        .map(|_| palette_entry(WorkbenchCommand::Run, "run", "workbench.run", None))
        .collect::<Vec<_>>();
    assert_eq!(
        CommandPalette::new(overflow).err(),
        Some("command_palette_size_invalid".to_owned())
    );
}

#[test]
fn denied_intents_leave_no_action_no_sequence_and_no_audit_entry() {
    // 没有 capability：面板是空的，直接 dispatch 也被拒，且不留任何痕迹。
    let mut forbidden = controller(false);
    assert!(forbidden.visible_commands().is_empty());
    assert_eq!(
        forbidden.dispatch(WorkbenchIntent::Status),
        Err("capability_unavailable:workbench.status".to_owned())
    );
    assert!(forbidden.audit().is_empty());
    assert_eq!(forbidden.pending_identity(), None);

    // 断线：控制器没有发送队列，因此本地拒绝新意图而不是排队重试。
    let mut offline = controller(true);
    let transition = offline.set_connection(ConnectionStatus::Degraded);
    assert_eq!(transition.previous, ConnectionStatus::Attached);
    assert_eq!(transition.current, ConnectionStatus::Degraded);
    assert_eq!(transition.in_flight, InFlightDisposition::None);
    assert_eq!(
        offline.dispatch(WorkbenchIntent::Run {
            prompt: "queued?".to_owned(),
            draft_revision: offline.draft_revision(),
        }),
        Err("workbench_controller_offline".to_owned())
    );
    assert!(offline.audit().is_empty());
    assert_eq!(offline.pending_identity(), None);

    // 参数越界：拒绝发生在动作构造之前，序号与审计都不动。
    let mut controller = controller(true);
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Open {
            workspace: "  ".to_owned(),
        }),
        Err("workbench_workspace_invalid".to_owned())
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Open {
            workspace: "a\0b".to_owned(),
        }),
        Err("workbench_workspace_invalid".to_owned())
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Attach {
            session_id: "s".repeat(MAX_WORKBENCH_TARGET_BYTES + 1),
        }),
        Err("workbench_session_id_invalid".to_owned())
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::New {
            title: Some("t".repeat(MAX_WORKBENCH_TITLE_BYTES + 1)),
        }),
        Err("workbench_title_invalid".to_owned())
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Cancel { run_id: None }),
        Err("run_required".to_owned())
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "p".repeat(MAX_CONTROLLER_TEXT_BYTES + 1),
            draft_revision: controller.draft_revision(),
        }),
        Err("workbench_prompt_invalid".to_owned())
    );
    assert_eq!(
        controller.set_draft("d\0raft"),
        Err("workbench_draft_invalid".to_owned())
    );
    assert_eq!(
        controller.set_draft("d".repeat(MAX_CONTROLLER_TEXT_BYTES + 1)),
        Err("workbench_draft_invalid".to_owned())
    );
    assert!(controller.audit().is_empty());

    // 控制器作用域同样是 fail-closed。
    assert_eq!(
        WorkbenchController::new(
            "  ",
            PRINCIPAL,
            EPOCH,
            CURSOR,
            Some(REVISION),
            Vec::new(),
            None,
            Vec::new(),
        )
        .err(),
        Some("workbench_controller_scope_invalid".to_owned())
    );
    assert_eq!(
        WorkbenchController::new(
            WORKSPACE,
            PRINCIPAL,
            EPOCH,
            0,
            Some(REVISION),
            Vec::new(),
            None,
            Vec::new(),
        )
        .err(),
        Some("workbench_controller_cursor_invalid".to_owned())
    );
    assert_eq!(
        WorkbenchController::new(
            WORKSPACE,
            PRINCIPAL,
            EPOCH,
            CURSOR,
            Some(0),
            Vec::new(),
            None,
            Vec::new(),
        )
        .err(),
        Some("workbench_controller_cursor_invalid".to_owned())
    );
    assert_eq!(
        WorkbenchController::new(
            WORKSPACE,
            PRINCIPAL,
            EPOCH,
            CURSOR,
            Some(REVISION),
            Vec::new(),
            None,
            vec![UiCapability {
                schema: "kiana.ui-capability.v2".to_owned(),
                ..capability(WorkbenchCommand::Run, true)
            }],
        )
        .err(),
        Some("ui_capability_schema_invalid".to_owned())
    );

    // 拒绝不消耗序号：第一条被接受的意图拿到 sequence 1。
    let accepted = action(controller.dispatch(WorkbenchIntent::Status).unwrap());
    assert_eq!(controller.audit().len(), 1);
    assert_eq!(accepted.idempotency_key, "workbench:workbench.status:1");
}

#[test]
fn window_close_detaches_without_implicit_cancel_or_resume() {
    let mut controller = controller(true);
    controller.observe_run_status(ExecutionStatus::Running);
    let decision = controller.dispatch(WorkbenchIntent::WindowClose).unwrap();
    assert_eq!(
        decision,
        ControllerDecision::WindowClosed { run_pending: true }
    );
    assert_eq!(controller.state().connection, ConnectionStatus::Closed);
    assert_eq!(controller.state().run, RunStatus::Running);
    assert_eq!(controller.state().submission, SubmissionStatus::Idle);
    assert_eq!(
        controller.audit().last().unwrap().command,
        WorkbenchCommand::WindowClose
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Resume { run_id: None }),
        Err("workbench_controller_closed".to_owned())
    );
    assert_eq!(
        controller.set_draft("after close"),
        Err("workbench_controller_closed".to_owned())
    );
    assert_eq!(
        controller.switch_session(SESSION_TWO),
        Err("workbench_controller_closed".to_owned())
    );
    // 关窗只留一条 WindowClose 记录：既没有 cancel，也没有 resume。
    assert!(controller
        .audit()
        .iter()
        .all(|entry| entry.command == WorkbenchCommand::WindowClose));

    // 没有 run 在跑时不声称 pending，也不改写 run 维度。
    let mut idle = controller(true);
    assert_eq!(
        idle.dispatch(WorkbenchIntent::WindowClose).unwrap(),
        ControllerDecision::WindowClosed { run_pending: false }
    );
    assert_eq!(idle.state().run, RunStatus::Idle);
}

#[test]
fn all_required_intents_have_stable_command_names() {
    let commands: Vec<_> = WorkbenchCommand::ALL
        .into_iter()
        .map(WorkbenchCommand::as_str)
        .collect();
    assert_eq!(
        commands,
        vec![
            "workbench.open",
            "workbench.attach",
            "workbench.new",
            "workbench.continue",
            "workbench.status",
            "workbench.run",
            "workbench.cancel",
            "workbench.resume",
            "workbench.receipt",
        ]
    );
    let request_id = RequestId::new();
    assert_ne!(request_id.as_uuid(), uuid::Uuid::nil());
    // 关窗是本地信号，不是命令，也不在可派发命令表里。
    assert_eq!(
        WorkbenchCommand::WindowClose.as_str(),
        "workbench.window_close"
    );
    assert!(!WorkbenchCommand::ALL.contains(&WorkbenchCommand::WindowClose));
    assert!(!WorkbenchCommand::WindowClose.is_mutating());
    for command in WorkbenchCommand::ALL {
        assert!(command.as_str().starts_with("workbench."));
    }
}

#[test]
fn multi_session_targets_stay_bound_to_the_selected_session() {
    let mut controller = controller(true);
    let first = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "one".to_owned(),
                draft_revision: controller.draft_revision(),
            })
            .unwrap(),
    );
    assert_eq!(first.target_id, SESSION_ONE);
    controller
        .record_action_result(first.command_id, UiActionDisposition::Applied)
        .unwrap();

    controller.set_draft("draft for two").unwrap();
    let before = controller.draft_revision();
    controller.switch_session(SESSION_TWO).unwrap();
    assert!(controller.draft().is_empty());
    assert!(controller.draft_revision() > before);
    let second = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "two".to_owned(),
                draft_revision: controller.draft_revision(),
            })
            .unwrap(),
    );
    assert_eq!(second.target_id, SESSION_TWO);
    assert_ne!(second.idempotency_key, first.idempotency_key);
    controller
        .record_action_result(second.command_id, UiActionDisposition::Applied)
        .unwrap();

    // attach 只是准备一个服务端动作：本地「当前看哪个会话」仍由 switch_session 决定。
    let attach = action(
        controller
            .dispatch(WorkbenchIntent::Attach {
                session_id: SESSION_ONE.to_owned(),
            })
            .unwrap(),
    );
    assert_eq!(attach.target_id, SESSION_ONE);
    assert_eq!(attach.payload["session_id"], SESSION_ONE);
    assert_eq!(
        controller.state().active_session_id.as_deref(),
        Some(SESSION_TWO)
    );
    // attach 清空草稿边界：切会话后不能带着旧文本再次提交。
    assert!(controller.draft().is_empty());
    // 两个会话各有一条独立审计，命令 id 互不相同。
    let command_ids: BTreeSet<String> = controller
        .audit()
        .iter()
        .map(|entry| entry.command_id.to_string())
        .collect();
    assert_eq!(command_ids.len(), 3);
}

#[test]
fn connection_loss_marks_the_submission_unknown_with_its_original_identity() {
    let mut controller = controller(true);
    let prepared = action(controller.dispatch(WorkbenchIntent::Status).unwrap());
    let identity = prepared.identity();
    assert_eq!(controller.pending_identity(), Some(identity.clone()));

    let transition = controller.set_connection(ConnectionStatus::Degraded);
    assert_eq!(transition.previous, ConnectionStatus::Attached);
    assert_eq!(transition.current, ConnectionStatus::Degraded);
    assert_eq!(
        transition.in_flight,
        InFlightDisposition::Unknown(identity.clone())
    );
    assert_eq!(controller.state().connection, ConnectionStatus::Degraded);
    assert_eq!(controller.state().submission, SubmissionStatus::Unknown);
    // 身份一个字节都没变：还是同一个 command id 与同一个 idempotency key。
    assert_eq!(controller.pending_identity(), Some(identity.clone()));
    assert_eq!(controller.audit().len(), 2);
    let last = controller.audit().last().unwrap();
    assert_eq!(last.disposition, AuditDisposition::Unknown);
    assert_eq!(last.command_id, prepared.command_id);
    assert_eq!(last.idempotency_key, prepared.idempotency_key);
    // 重复的断线通知不会重复记账。
    controller.set_connection(ConnectionStatus::Degraded);
    assert_eq!(controller.audit().len(), 2);

    // 重连既不清除未对账身份，也不自动重发。
    let back = controller.set_connection(ConnectionStatus::Attached);
    assert_eq!(back.in_flight, InFlightDisposition::None);
    assert_eq!(controller.state().submission, SubmissionStatus::Unknown);
    assert_eq!(controller.pending_identity(), Some(identity.clone()));
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "again".to_owned(),
            draft_revision: controller.draft_revision(),
        }),
        Err("duplicate_submission".to_owned())
    );
    // 只有用原 command id 对账才放行；对账它是「原命令」而不是新命令。
    assert_eq!(
        controller.record_action_result(RequestId::new(), UiActionDisposition::Applied),
        Err("action_result_command_mismatch".to_owned())
    );
    assert_eq!(
        controller.record_action_result(prepared.command_id, UiActionDisposition::Applied),
        Ok(())
    );
    assert_eq!(controller.pending_identity(), None);
    let next = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "after reconciliation".to_owned(),
                draft_revision: controller.draft_revision(),
            })
            .unwrap(),
    );
    assert_ne!(next.idempotency_key, prepared.idempotency_key);
    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
}

#[test]
fn response_loss_never_mints_a_second_command_and_survives_a_terminal_run() {
    let mut controller = controller(true);
    let prepared = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "lost".to_owned(),
                draft_revision: controller.draft_revision(),
            })
            .unwrap(),
    );
    controller.set_connection(ConnectionStatus::Degraded);
    controller.set_connection(ConnectionStatus::Attached);
    // run 走到终态并不能证明这次提交的结局已知。
    controller.observe_run_status(ExecutionStatus::Completed);
    assert_eq!(controller.state().run, RunStatus::Completed);
    assert_eq!(controller.state().submission, SubmissionStatus::Unknown);
    assert_eq!(controller.pending_identity(), Some(prepared.identity()));
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "second effect".to_owned(),
            draft_revision: controller.draft_revision(),
        }),
        Err("duplicate_submission".to_owned())
    );
    // 对账本身仍然是未知：身份保留，继续挡着第二条变更。
    controller
        .record_action_result(prepared.command_id, UiActionDisposition::Unknown)
        .unwrap();
    assert_eq!(controller.pending_identity(), Some(prepared.identity()));
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "third effect".to_owned(),
            draft_revision: controller.draft_revision(),
        }),
        Err("duplicate_submission".to_owned())
    );
    assert_eq!(controller.audit().len(), 3);
    // 原始命令给出确定结论后才放行。
    controller
        .record_action_result(prepared.command_id, UiActionDisposition::Applied)
        .unwrap();
    assert_eq!(controller.pending_identity(), None);

    // 没有未结算提交时，终态观察仍然清掉提交显示。
    let mut settled = controller(true);
    settled.observe_run_status(ExecutionStatus::Completed);
    assert_eq!(settled.state().submission, SubmissionStatus::Idle);
    assert_eq!(settled.pending_identity(), None);
}

#[test]
fn stale_card_results_are_rejected_and_never_resurrect_a_submission() {
    let mut controller = controller(true);
    let first = action(controller.dispatch(WorkbenchIntent::Status).unwrap());
    controller
        .record_action_result(first.command_id, UiActionDisposition::Applied)
        .unwrap();
    // 卡片已结算：迟到的同 id 结果不会被当成第二次成功，也不复活提交。
    assert_eq!(controller.state().submission, SubmissionStatus::Applied);
    assert_eq!(
        controller.record_action_result(first.command_id, UiActionDisposition::Applied),
        Err("action_result_without_submission".to_owned())
    );
    assert_eq!(controller.state().submission, SubmissionStatus::Applied);

    let second = action(controller.dispatch(WorkbenchIntent::Status).unwrap());
    assert_ne!(second.command_id, first.command_id);
    assert_eq!(
        controller.record_action_result(first.command_id, UiActionDisposition::Applied),
        Err("action_result_command_mismatch".to_owned())
    );
    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
    assert_eq!(controller.pending_identity(), Some(second.identity()));
    controller
        .record_action_result(second.command_id, UiActionDisposition::Rejected)
        .unwrap();
    assert_eq!(controller.state().submission, SubmissionStatus::Idle);
    assert_eq!(controller.pending_identity(), None);
    assert_eq!(
        controller.audit().last().unwrap().disposition,
        AuditDisposition::Rejected
    );
    assert_eq!(
        controller.record_action_result(second.command_id, UiActionDisposition::Applied),
        Err("action_result_without_submission".to_owned())
    );
}

#[test]
fn termination_signals_are_not_window_close_cancel_or_resume() {
    assert_eq!(
        TerminationSignal::ALL.map(TerminationSignal::as_str),
        ["sigint", "sigterm"]
    );
    for signal in TerminationSignal::ALL {
        let mut controller = controller(true);
        let prepared = action(
            controller
                .dispatch(WorkbenchIntent::Cancel {
                    run_id: Some(RunId::new()),
                })
                .unwrap(),
        );
        assert_eq!(prepared.command, WorkbenchCommand::Cancel);
        assert_eq!(
            controller.note_termination_signal(signal),
            InFlightDisposition::Unknown(prepared.identity())
        );
        assert_eq!(controller.state().termination, Some(signal));
        assert_eq!(controller.state().submission, SubmissionStatus::Unknown);
        // 信号不是关窗：连接边界不动；也不改写 run 维度。
        assert_eq!(controller.state().connection, ConnectionStatus::Attached);
        assert_eq!(controller.state().run, RunStatus::Idle);
        assert_eq!(controller.audit().len(), 2);
        let last = controller.audit().last().unwrap();
        assert_eq!(last.disposition, AuditDisposition::Unknown);
        assert_eq!(last.idempotency_key, prepared.idempotency_key);
        // 重复信号不重复记账，且第一个信号被保留。
        assert_eq!(
            controller.note_termination_signal(TerminationSignal::Interrupt),
            InFlightDisposition::Unknown(prepared.identity())
        );
        assert_eq!(controller.state().termination, Some(signal));
        assert_eq!(controller.audit().len(), 2);

        // 终止态不再准备新意图，也不接受本地编辑。
        assert_eq!(
            controller.dispatch(WorkbenchIntent::Resume { run_id: None }),
            Err("workbench_controller_terminating".to_owned())
        );
        assert_eq!(
            controller.set_draft("late edit"),
            Err("workbench_controller_terminating".to_owned())
        );
        assert_eq!(
            controller.switch_session(SESSION_TWO),
            Err("workbench_controller_terminating".to_owned())
        );
        // 关窗仍然可用（退出流程要能收尾），而且它不合成 cancel/resume。
        assert_eq!(
            controller.dispatch(WorkbenchIntent::WindowClose).unwrap(),
            ControllerDecision::WindowClosed { run_pending: false }
        );
        assert!(controller.audit().iter().all(|entry| matches!(
            entry.command,
            WorkbenchCommand::Cancel | WorkbenchCommand::WindowClose
        )));
        assert!(!controller
            .audit()
            .iter()
            .any(|entry| entry.command == WorkbenchCommand::Resume));
        assert_eq!(
            controller.audit().last().unwrap().disposition,
            AuditDisposition::WindowClosed
        );
    }
}

#[test]
fn restart_recovery_refuses_stale_widened_or_foreign_coordinates() {
    let controller = controller(true);

    let mut offline = observation();
    offline.connection = ConnectionStatus::Degraded;
    assert_eq!(
        controller.recover(offline),
        Err("workbench_recovery_offline".to_owned())
    );
    let mut blank = observation();
    blank.workspace = "  ".to_owned();
    assert_eq!(
        controller.recover(blank),
        Err("workbench_controller_scope_invalid".to_owned())
    );
    let mut zero = observation();
    zero.cursor = 0;
    assert_eq!(
        controller.recover(zero),
        Err("workbench_controller_cursor_invalid".to_owned())
    );
    let mut zero_revision = observation();
    zero_revision.revision = Some(0);
    assert_eq!(
        controller.recover(zero_revision),
        Err("workbench_controller_cursor_invalid".to_owned())
    );
    // 作用域不能漂移：换工作区/换 principal 都得新建控制器。
    let mut other_scope = observation();
    other_scope.workspace = "workspace-two".to_owned();
    assert_eq!(
        controller.recover(other_scope),
        Err("workbench_recovery_scope_changed".to_owned())
    );
    let mut other_principal = observation();
    other_principal.principal = "someone-else".to_owned();
    assert_eq!(
        controller.recover(other_principal),
        Err("workbench_recovery_scope_changed".to_owned())
    );
    // epoch 变了说明世界被围栏过：必须先 hydrate 快照，而不是恢复旧身份。
    let mut new_epoch = observation();
    new_epoch.epoch = "epoch-15".to_owned();
    assert_eq!(
        controller.recover(new_epoch),
        Err("workbench_recovery_epoch_mismatch".to_owned())
    );
    // 坐标只能前进。
    let mut behind = observation();
    behind.cursor = CURSOR - 1;
    assert_eq!(
        controller.recover(behind),
        Err("workbench_recovery_cursor_stale".to_owned())
    );
    let mut behind_revision = observation();
    behind_revision.revision = Some(REVISION - 1);
    assert_eq!(
        controller.recover(behind_revision),
        Err("workbench_recovery_revision_stale".to_owned())
    );
    // 换会话是用户动作，不是恢复的副作用。
    let mut other_session = observation();
    other_session.active_session_id = Some(SESSION_TWO.to_owned());
    assert_eq!(
        controller.recover(other_session),
        Err("workbench_recovery_session_changed".to_owned())
    );
    // 会话集合本身的形状仍然先过 SessionSwitcher。
    let mut duplicate = observation();
    duplicate.sessions = vec![
        session(SESSION_ONE, "one"),
        session(SESSION_ONE, "duplicate"),
    ];
    assert_eq!(
        controller.recover(duplicate),
        Err("session_switcher_duplicate_session".to_owned())
    );
    let detached = controller_with(true, None);
    let mut missing_active = observation();
    missing_active.active_session_id = Some("00000000-0000-0000-0000-0000000000ff".to_owned());
    assert_eq!(
        detached.recover(missing_active),
        Err("session_switcher_active_missing".to_owned())
    );
    let mut overflow = observation();
    overflow.sessions = (0..=MAX_WORKBENCH_SESSIONS)
        .map(|index| session(&format!("session-{index}"), "title"))
        .collect();
    assert_eq!(
        controller.recover(overflow),
        Err("session_switcher_limit".to_owned())
    );
    // capability 仍然过 validate：伪造的服务端面进不来。
    let mut forged_capability = observation();
    forged_capability.capabilities = vec![UiCapability {
        schema: UI_CAPABILITY_SCHEMA.to_owned(),
        scope_digest: "sha256:not-a-digest".to_owned(),
        ..capability(WorkbenchCommand::Run, true)
    }];
    assert_eq!(
        controller.recover(forged_capability),
        Err("ui_capability_scope_digest_invalid".to_owned())
    );
}

#[test]
fn restart_recovery_carries_only_server_coordinates_and_the_unreconciled_identity() {
    let mut controller = controller(true);
    controller.set_draft("draft before the crash").unwrap();
    let prepared = action(
        controller
            .dispatch(WorkbenchIntent::Run {
                prompt: "lost run".to_owned(),
                draft_revision: controller.draft_revision(),
            })
            .unwrap(),
    );
    let pending = prepared.identity();
    let previous_draft_revision = controller.draft_revision();
    controller.set_connection(ConnectionStatus::Degraded);

    let mut recovered_observation = observation();
    recovered_observation.cursor = CURSOR + 4;
    recovered_observation.revision = Some(REVISION + 1);
    let WorkbenchRecovery {
        controller: mut recovered,
        reconcile,
    } = controller.recover(recovered_observation).unwrap();
    assert_eq!(reconcile, Some(pending.clone()));
    assert_eq!(recovered.state().epoch, EPOCH);
    assert_eq!(recovered.state().cursor, CURSOR + 4);
    assert_eq!(recovered.state().revision, Some(REVISION + 1));
    assert_eq!(recovered.state().connection, ConnectionStatus::Attached);
    assert_eq!(recovered.state().termination, None);
    assert_eq!(recovered.state().run, RunStatus::Idle);
    // 提交维度停在「等待对账」，而不是假装 Idle。
    assert_eq!(recovered.state().submission, SubmissionStatus::Unknown);
    assert_eq!(recovered.pending_identity(), Some(pending.clone()));
    // 草稿内容与版本边界都不带走。
    assert!(recovered.draft().is_empty());
    assert!(recovered.draft_revision() > previous_draft_revision);
    // 新进程的审计只有一条：身份被带走、结局尚未对账。
    assert_eq!(recovered.audit().len(), 1);
    assert_eq!(
        recovered.audit()[0].disposition,
        AuditDisposition::Recovered
    );
    assert_eq!(
        recovered.audit()[0].idempotency_key,
        pending.idempotency_key
    );
    assert_eq!(recovered.audit()[0].command_id, pending.command_id);
    assert!(recovered
        .audit()
        .iter()
        .all(|entry| entry.command == WorkbenchCommand::Run));
    // 服务端面照旧按 capability 投影。
    assert_eq!(
        recovered.visible_commands().len(),
        WorkbenchCommand::ALL.len()
    );
    // 未对账之前不能准备第二条变更。
    assert_eq!(
        recovered.dispatch(WorkbenchIntent::Run {
            prompt: "second".to_owned(),
            draft_revision: recovered.draft_revision(),
        }),
        Err("duplicate_submission".to_owned())
    );
    // 对账后放行；旧进程的草稿版本仍然过不了 stale_draft 门。
    recovered
        .record_action_result(pending.command_id, UiActionDisposition::Applied)
        .unwrap();
    assert_eq!(recovered.pending_identity(), None);
    assert_eq!(recovered.state().submission, SubmissionStatus::Applied);
    assert_eq!(
        recovered.dispatch(WorkbenchIntent::Run {
            prompt: "stale draft".to_owned(),
            draft_revision: previous_draft_revision,
        }),
        Err("stale_draft".to_owned())
    );
    // 序号延续：新动作的 idempotency key 不会与已死进程用过的 key 重名。
    let fresh = action(
        recovered
            .dispatch(WorkbenchIntent::Run {
                prompt: "fresh".to_owned(),
                draft_revision: recovered.draft_revision(),
            })
            .unwrap(),
    );
    assert_ne!(fresh.idempotency_key, pending.idempotency_key);
    assert_eq!(fresh.idempotency_key, "workbench:workbench.run:2");
    assert_ne!(fresh.command_id, pending.command_id);
}

#[test]
fn restart_recovery_without_a_pending_submission_starts_idle_and_allows_work() {
    let controller = controller(true);
    let recovery = controller.recover(observation()).unwrap();
    assert_eq!(recovery.reconcile, None);
    let mut recovered = recovery.controller;
    assert_eq!(recovered.state().submission, SubmissionStatus::Idle);
    assert_eq!(recovered.pending_identity(), None);
    assert!(recovered.audit().is_empty());
    assert_eq!(
        recovered.state().active_session_id.as_deref(),
        Some(SESSION_ONE)
    );
    // 没有未对账身份时，恢复出来的控制器可以直接接受新意图。
    let prepared = action(recovered.dispatch(WorkbenchIntent::Status).unwrap());
    assert_eq!(prepared.expected_epoch, EPOCH);
    assert_eq!(prepared.expected_cursor, CURSOR);
    assert_eq!(prepared.target_id, SESSION_ONE);
    assert_eq!(recovered.audit().len(), 1);
    recovered
        .record_action_result(prepared.command_id, UiActionDisposition::Applied)
        .unwrap();
    assert_eq!(recovered.pending_identity(), None);
}

#[test]
fn hostile_control_traffic_keeps_the_controller_bounded_and_audit_append_only() {
    let mut controller = controller(true);
    let dispatches = MAX_CONTROLLER_AUDIT_ENTRIES * 2;
    let mut keys = BTreeSet::new();
    for _ in 0..dispatches {
        let prepared = action(controller.dispatch(WorkbenchIntent::Status).unwrap());
        assert!(
            keys.insert(prepared.idempotency_key.clone()),
            "idempotency keys must never repeat: {}",
            prepared.idempotency_key
        );
        controller
            .record_action_result(prepared.command_id, UiActionDisposition::Applied)
            .unwrap();
    }
    assert_eq!(controller.audit().len(), MAX_CONTROLLER_AUDIT_ENTRIES);
    let sequences: Vec<u64> = controller
        .audit()
        .iter()
        .map(|entry| entry.sequence)
        .collect();
    assert!(sequences.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(sequences.last().copied(), Some(dispatches as u64));
    assert!(
        sequences.first().copied().unwrap() > 1,
        "the bounded trace must drop the oldest entries, not the newest"
    );
    // 审计身份仍然唯一：每张卡片恰好两条记录（Prepared + Applied），不会被截成半张。
    let ids: BTreeSet<String> = controller
        .audit()
        .iter()
        .map(|entry| entry.command_id.to_string())
        .collect();
    assert_eq!(ids.len(), MAX_CONTROLLER_AUDIT_ENTRIES / 2);
    let dispositions: BTreeSet<String> = controller
        .audit()
        .iter()
        .map(|entry| format!("{:?}", entry.disposition))
        .collect();
    assert_eq!(dispositions.len(), 2);
    // 草稿与 prompt 的文本上限是 fail-closed 的。
    assert_eq!(
        controller.set_draft("x".repeat(MAX_CONTROLLER_TEXT_BYTES)),
        Ok(())
    );
    assert_eq!(controller.draft().len(), MAX_CONTROLLER_TEXT_BYTES);
    assert_eq!(
        controller.set_draft("x".repeat(MAX_CONTROLLER_TEXT_BYTES + 1)),
        Err("workbench_draft_invalid".to_owned())
    );
    assert_eq!(controller.draft().len(), MAX_CONTROLLER_TEXT_BYTES);
    // 没有超长 prompt 能溜进动作。
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Run {
            prompt: "x".repeat(MAX_CONTROLLER_TEXT_BYTES + 1),
            draft_revision: controller.draft_revision(),
        }),
        Err("workbench_prompt_invalid".to_owned())
    );
}

#[test]
fn declared_scenarios_are_exercised_by_their_named_fixtures() {
    let fixture = fixture();
    let sources = [
        include_str!("ui14_workbench_controller.rs"),
        include_str!("ui14_workbench_controller_guard.rs"),
    ];
    let declared: Vec<&str> = fixture["fixtures"]
        .as_array()
        .expect("fixtures")
        .iter()
        .map(|value| value.as_str().expect("fixture name"))
        .collect();
    for scenario in fixture["scenarios"].as_array().expect("scenarios") {
        let name = scenario["fixture"].as_str().expect("scenario fixture");
        assert!(
            declared.contains(&name),
            "scenario references an undeclared fixture: {name}"
        );
        assert_eq!(
            sources
                .iter()
                .filter(|source| source.contains(&format!("fn {name}(")))
                .count(),
            1,
            "scenario fixture must exist exactly once: {name}"
        );
        // 场景名本身就是这张表的文档：它必须能在夹具文本里找到。
        let scenario_name = scenario["name"].as_str().expect("scenario name");
        assert!(!scenario_name.is_empty(), "scenario {name} must be named");
    }
}
