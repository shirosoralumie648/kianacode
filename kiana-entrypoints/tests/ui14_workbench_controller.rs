use kiana_entrypoints::workbench_controller::{
    CommandPalette, ConnectionStatus, ControllerDecision, KeyBinding, KeyChord, Keymap, RunStatus,
    SubmissionStatus, WorkbenchCommand, WorkbenchController, WorkbenchIntent,
};
use kiana_protocol::{
    json_digest, ExecutionStatus, RequestId, RunId, SessionId, UiActionDisposition, UiCapability,
    UI_CAPABILITY_SCHEMA,
};
use serde_json::Value;

const FIXTURE: &str = include_str!("fixtures/ui14-controller-actions.json");
const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

fn session(value: &str, title: &str) -> kiana_protocol::SessionSummary {
    kiana_protocol::SessionSummary {
        session_id: SessionId::parse_str(value).unwrap(),
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

fn controller(all_capabilities: bool) -> WorkbenchController {
    let capabilities = if all_capabilities {
        WorkbenchCommand::ALL
            .into_iter()
            .map(|command| capability(command, true))
            .collect()
    } else {
        Vec::new()
    };
    WorkbenchController::new(
        "workspace",
        "local-user",
        "epoch-14",
        7,
        Some(3),
        vec![
            session("00000000-0000-0000-0000-000000000014", "one"),
            session("00000000-0000-0000-0000-000000000015", "two"),
        ],
        Some("00000000-0000-0000-0000-000000000014".to_owned()),
        capabilities,
    )
    .unwrap()
}

fn action(
    decision: ControllerDecision,
) -> kiana_entrypoints::workbench_controller::WorkbenchUiAction {
    match decision {
        ControllerDecision::Action(action) => action,
        other => panic!("expected action, got {other:?}"),
    }
}

#[test]
fn fixture_commands_keymap_and_deny_matrix_are_wired() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    assert_eq!(fixture["schema"], "kiana.workbench-controller.v1");
    assert_eq!(
        fixture["commands"].as_array().unwrap().len(),
        WorkbenchCommand::ALL.len()
    );
    assert_eq!(fixture["keymap"].as_array().unwrap().len(), 8);
    assert!(fixture["deny_first"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "stale_draft"));

    let keymap = Keymap::default_workbench();
    assert_eq!(
        keymap.resolve(KeyChord::new('r', true, false, false)),
        Some(WorkbenchCommand::Run)
    );
    assert_eq!(keymap.bindings().count(), 8);
}

#[test]
fn controller_translates_actions_with_scope_and_preserves_separate_statuses() {
    let mut controller = controller(true);
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
    let wire = open.clone().into_protocol(DIGEST);
    assert_eq!(wire.schema, kiana_protocol::UI_ACTION_SCHEMA);
    assert_eq!(wire.payload_digest, DIGEST);

    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
    assert_eq!(controller.state().run, RunStatus::Idle);
    controller
        .record_action_result(open.command_id, UiActionDisposition::Applied)
        .unwrap();
    assert_eq!(controller.state().submission, SubmissionStatus::Applied);
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
    assert_eq!(controller.state().submission, SubmissionStatus::Preparing);
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
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Open {
            workspace: "w".repeat(257),
        }),
        Err("workbench_workspace_invalid".to_owned())
    );
}

#[test]
fn every_required_intent_emits_its_stable_action_command() {
    let mut controller = controller(true);
    let run_id = Some(RunId::new());
    let intents = vec![
        (
            WorkbenchIntent::Continue {
                run_id,
                prompt: "continue".to_owned(),
                draft_revision: controller.draft_revision(),
            },
            WorkbenchCommand::Continue,
        ),
        (WorkbenchIntent::Status, WorkbenchCommand::Status),
        (
            WorkbenchIntent::Run {
                prompt: "run".to_owned(),
                draft_revision: 0,
            },
            WorkbenchCommand::Run,
        ),
        (WorkbenchIntent::Cancel { run_id }, WorkbenchCommand::Cancel),
        (WorkbenchIntent::Resume { run_id }, WorkbenchCommand::Resume),
        (
            WorkbenchIntent::Receipt { run_id },
            WorkbenchCommand::Receipt,
        ),
    ];
    for (intent, expected) in intents {
        let intent = if matches!(&intent, WorkbenchIntent::Run { .. }) {
            WorkbenchIntent::Run {
                prompt: "run".to_owned(),
                draft_revision: controller.draft_revision(),
            }
        } else {
            intent
        };
        let action = action(controller.dispatch(intent).unwrap());
        assert_eq!(action.command, expected);
        controller
            .record_action_result(action.command_id, UiActionDisposition::Applied)
            .unwrap();
    }
    for (intent, expected) in [
        (
            WorkbenchIntent::Open {
                workspace: "workspace-two".to_owned(),
            },
            WorkbenchCommand::Open,
        ),
        (
            WorkbenchIntent::Attach {
                session_id: "00000000-0000-0000-0000-000000000015".to_owned(),
            },
            WorkbenchCommand::Attach,
        ),
        (
            WorkbenchIntent::New {
                title: Some("new session".to_owned()),
            },
            WorkbenchCommand::New,
        ),
    ] {
        let action = action(controller.dispatch(intent).unwrap());
        assert_eq!(action.command, expected);
        controller
            .record_action_result(action.command_id, UiActionDisposition::Applied)
            .unwrap();
    }
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

    let palette = CommandPalette::default_workbench();
    assert!(palette.visible(&[]).is_empty());
    let visible = palette.visible(&[capability(WorkbenchCommand::Run, true)]);
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].command, WorkbenchCommand::Run);
    assert!(controller(false).visible_commands().is_empty());
    assert_eq!(
        controller(false).dispatch(WorkbenchIntent::Status),
        Err("capability_unavailable:workbench.status".to_owned())
    );
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
    assert_eq!(
        controller.audit().last().unwrap().command,
        WorkbenchCommand::WindowClose
    );
    assert_eq!(
        controller.dispatch(WorkbenchIntent::Resume { run_id: None }),
        Err("workbench_controller_closed".to_owned())
    );
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
}
