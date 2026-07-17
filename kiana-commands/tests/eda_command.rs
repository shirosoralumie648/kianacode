use kiana_commands::eda::EdaCommand;
use kiana_commands::{create_default_command_registry, Command, CommandContext};
use kiana_tasks::{
    list_verification_packets, read_evidence_events, read_workflow_events, EvidenceKind,
    VerificationStatus, WorkflowEventKind,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

fn root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "kiana-eda-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn context(args: impl Into<String>, root: &Path) -> CommandContext {
    CommandContext {
        args: args.into(),
        app_state: HashMap::from([("cwd".to_string(), json!(root.to_string_lossy()))]),
    }
}

fn write_complete_fixture(root: &Path, cpl_package: &str) {
    fs::create_dir_all(root.join("hardware/gerber")).unwrap();
    fs::write(
        root.join("hardware/requirements.md"),
        "5 V input; 3.3 V rail; SWD bring-up; USB interface\n",
    )
    .unwrap();
    fs::write(
        root.join("hardware/main.kicad_sch"),
        "(kicad_sch (version 20231120) (generator kiana-test))\n",
    )
    .unwrap();
    fs::write(
        root.join("hardware/bom.csv"),
        "Designator,MPN,Package,Quantity\nR1,RC0402FR-0710KL,0402,1\nU1,STM32F103C8T6,LQFP48,1\n",
    )
    .unwrap();
    fs::write(
        root.join("hardware/cpl.csv"),
        format!(
            "Designator,Package,Mid X,Mid Y,Rotation,Layer\nR1,0402,10.0,8.0,0,Top\nU1,{cpl_package},20.0,15.0,90,Top\n"
        ),
    )
    .unwrap();
    fs::write(
        root.join("hardware/gerber/demo-F_Cu.gbr"),
        "G04 copper*\nM02*\n",
    )
    .unwrap();
    fs::write(
        root.join("hardware/gerber/demo-Edge_Cuts.gbr"),
        "G04 edge*\nM02*\n",
    )
    .unwrap();
    fs::write(root.join("hardware/gerber/demo-PTH.drl"), "M48\nM30\n").unwrap();
    fs::write(
        root.join("hardware/constraints.md"),
        "2 layers; minimum trace 0.15 mm; JLCPCB assembly\n",
    )
    .unwrap();
    fs::write(
        root.join("hardware/main.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<export>
  <components>
    <comp ref="R1"><value>10k</value><footprint>0402</footprint></comp>
    <comp ref="U1"><value>STM32F103C8T6</value><footprint>LQFP48</footprint></comp>
  </components>
  <nets>
    <net code="1" name="+3V3">
      <node ref="U1" pin="1" pinfunction="VDD" pintype="power_in" />
      <node ref="U1" pin="2" pinfunction="VOUT" pintype="power_out" />
    </net>
    <net code="2" name="SWDIO">
      <node ref="U1" pin="3" pinfunction="SWDIO" pintype="bidirectional" />
      <node ref="R1" pin="1" pinfunction="1" pintype="passive" />
    </net>
  </nets>
</export>
"#,
    )
    .unwrap();
}

fn complete_args() -> &'static str {
    "review --json --requirements hardware/requirements.md --schematic hardware/main.kicad_sch --bom hardware/bom.csv --gerber hardware/gerber --cpl hardware/cpl.csv --constraints hardware/constraints.md --netlist hardware/main.xml"
}

#[test]
fn default_registry_includes_eda_command() {
    let registry = create_default_command_registry();
    let command = registry.get("eda").expect("eda command must be registered");
    assert!(command.supports_non_interactive());
}

#[tokio::test]
async fn eda_review_emits_immutable_reports_workflow_event_and_evidence() {
    let root = root("complete");
    write_complete_fixture(&root, "LQFP48");

    let output = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["schema"], "kiana.eda-review.v1");
    assert_eq!(report["rule_version"], "eda-review-rules.v2");
    assert_eq!(report["status"], "pass");
    assert_eq!(report["summary"]["blocked"], 0);
    assert_eq!(report["summary"]["errors"], 0);
    assert_eq!(report["approval_requirements"][0], "hardware_order");
    assert_eq!(report["summary"]["netlist_components"], 2);
    assert_eq!(report["summary"]["net_count"], 2);
    assert_eq!(report["summary"]["power_net_count"], 1);
    assert_eq!(report["summary"]["interface_net_count"], 1);
    assert_eq!(report["summary"]["dangling_net_count"], 0);
    assert!(report["sources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|source| source["kind"] == "netlist"));
    assert!(report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .any(|check| check["check_id"] == "netlist_structure" && check["status"] == "pass"));

    let run_id = report["run_id"].as_str().unwrap();
    let review_id = report["review_id"].as_str().unwrap();
    let review_dir = root
        .join(".kiana/workflows")
        .join(run_id)
        .join("eda/reviews")
        .join(review_id);
    assert!(review_dir.join("eda_review.json").is_file());
    assert!(review_dir.join("bom_risk.md").is_file());
    assert!(review_dir.join("bringup-plan.md").is_file());

    let artifact_dir = root.join(".kiana/workflows").join(run_id);
    let workflow_events = read_workflow_events(&artifact_dir).unwrap();
    assert!(workflow_events
        .iter()
        .any(|event| event.kind == WorkflowEventKind::ArtifactWritten));
    let evidence = read_evidence_events(&artifact_dir).unwrap();
    assert_eq!(evidence.len(), 1);
    assert_eq!(evidence[0].kind, EvidenceKind::EdaCheck);
    assert_eq!(evidence[0].payload["review_id"], review_id);
    let packets = list_verification_packets(&artifact_dir).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].1.profile, "eda_review");
    assert_eq!(packets[0].1.final_status, VerificationStatus::Pass);
    assert!(workflow_events
        .iter()
        .any(|event| event.kind == WorkflowEventKind::VerificationCompleted));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_reports_netlist_component_missing_from_bom() {
    let root = root("netlist-bom-coverage");
    write_complete_fixture(&root, "LQFP48");
    fs::write(
        root.join("hardware/main.xml"),
        r#"<?xml version="1.0" encoding="UTF-8"?>
<export>
  <components>
    <comp ref="U1"><value>STM32F103C8T6</value></comp>
    <comp ref="J1"><value>USB-C</value></comp>
  </components>
  <nets>
    <net code="1" name="USB_D+">
      <node ref="U1" pin="10" pinfunction="USB_DP" pintype="bidirectional" />
      <node ref="J1" pin="A6" pinfunction="D+" pintype="passive" />
    </net>
  </nets>
</export>
"#,
    )
    .unwrap();

    let output = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["status"], "review_required");
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |finding| finding["code"] == "netlist_component_missing_from_bom"
                && finding["designator"] == "J1"
                && finding["severity"] == "error"
        ));
    let run_id = report["run_id"].as_str().unwrap();
    let packets = list_verification_packets(root.join(".kiana/workflows").join(run_id)).unwrap();
    assert_eq!(packets[0].1.final_status, VerificationStatus::Fail);

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_normalizes_netlist_designators_for_bom_coverage() {
    let root = root("netlist-designator-normalization");
    write_complete_fixture(&root, "LQFP48");
    fs::write(
        root.join("hardware/main.xml"),
        r#"<export>
  <components><comp ref=" r1 "/><comp ref="u1"/></components>
  <nets>
    <net code="1" name="SWDIO">
      <node ref="R1" pin="1" />
      <node ref=" U1 " pin="3" />
    </net>
  </nets>
</export>
"#,
    )
    .unwrap();

    let output = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["status"], "pass");
    assert!(!report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| {
            matches!(
                finding["code"].as_str(),
                Some("netlist_component_missing_from_bom" | "bom_component_missing_from_netlist")
            )
        }));

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_rejects_malformed_netlist_before_creating_workflow() {
    let root = root("malformed-netlist");
    write_complete_fixture(&root, "LQFP48");
    fs::write(root.join("hardware/main.xml"), "<export><components>").unwrap();

    let error = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("KiCad netlist XML"));
    assert!(!root.join(".kiana/workflows").exists());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_rejects_unknown_netlist_component_before_creating_workflow() {
    let root = root("unknown-netlist-component");
    write_complete_fixture(&root, "LQFP48");
    fs::write(
        root.join("hardware/main.xml"),
        r#"<export>
  <components><comp ref="U1"><value>MCU</value></comp></components>
  <nets><net code="1" name="SWDIO"><node ref="J1" pin="1" /></net></nets>
</export>
"#,
    )
    .unwrap();

    let error = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("unknown component J1"));
    assert!(!root.join(".kiana/workflows").exists());
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_missing_critical_artifacts_is_blocked_not_passed() {
    let root = root("missing");
    fs::create_dir_all(&root).unwrap();

    let output = EdaCommand
        .execute(context("review --json", &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["status"], "blocked");
    assert!(report["summary"]["blocked"].as_u64().unwrap() >= 3);
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "schematic_missing"));
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "bom_missing"));
    assert!(report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "gerber_missing"));
    let run_id = report["run_id"].as_str().unwrap();
    let packets = list_verification_packets(root.join(".kiana/workflows").join(run_id)).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].1.final_status, VerificationStatus::Blocked);

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_reports_bom_cpl_package_mismatch() {
    let root = root("mismatch");
    write_complete_fixture(&root, "QFN48");

    let output = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap();
    let report: Value = serde_json::from_str(&output.value).unwrap();

    assert_eq!(report["status"], "review_required");
    let finding = report["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| finding["code"] == "bom_cpl_package_mismatch")
        .expect("package mismatch finding");
    assert_eq!(finding["designator"], "U1");
    assert_eq!(finding["severity"], "error");
    let run_id = report["run_id"].as_str().unwrap();
    let packets = list_verification_packets(root.join(".kiana/workflows").join(run_id)).unwrap();
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].1.final_status, VerificationStatus::Fail);

    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn eda_review_rejects_parent_escape_before_creating_workflow() {
    let root = root("parent-escape");
    fs::create_dir_all(&root).unwrap();

    let error = EdaCommand
        .execute(context(
            "review --json --schematic ../outside.kicad_sch",
            &root,
        ))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("project-relative"));
    assert!(!root.join(".kiana/workflows").exists());
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn eda_review_rejects_symlink_escape_before_creating_workflow() {
    use std::os::unix::fs::symlink;

    let project_root = root("symlink-escape");
    let outside = root("symlink-outside");
    fs::create_dir_all(&project_root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("main.kicad_sch"), "outside\n").unwrap();
    symlink(&outside, project_root.join("escaped")).unwrap();

    let error = EdaCommand
        .execute(context(
            "review --json --schematic escaped/main.kicad_sch",
            &project_root,
        ))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("escapes project root"));
    assert!(!project_root.join(".kiana/workflows").exists());
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(outside);
}

#[cfg(unix)]
#[tokio::test]
async fn eda_review_rejects_symlink_inside_gerber_directory_before_workflow() {
    use std::os::unix::fs::symlink;

    let project_root = root("gerber-child-symlink");
    let outside = root("gerber-child-outside");
    write_complete_fixture(&project_root, "LQFP48");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("outside.gbr"), "G04 outside copper*\nM02*\n").unwrap();
    fs::remove_file(project_root.join("hardware/gerber/demo-F_Cu.gbr")).unwrap();
    symlink(
        outside.join("outside.gbr"),
        project_root.join("hardware/gerber/demo-F_Cu.gbr"),
    )
    .unwrap();

    let error = EdaCommand
        .execute(context(complete_args(), &project_root))
        .await
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("Gerber directory contains a symlink"));
    assert!(!project_root.join(".kiana/workflows").exists());
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(outside);
}

#[tokio::test]
async fn eda_review_retry_in_same_workflow_is_idempotent() {
    let root = root("idempotent");
    write_complete_fixture(&root, "LQFP48");

    let first = EdaCommand
        .execute(context(complete_args(), &root))
        .await
        .unwrap();
    let first: Value = serde_json::from_str(&first.value).unwrap();
    let run_id = first["run_id"].as_str().unwrap();
    let args = format!("{} --workflow {run_id}", complete_args());
    let second = EdaCommand.execute(context(args, &root)).await.unwrap();
    let second: Value = serde_json::from_str(&second.value).unwrap();

    assert_eq!(first["review_id"], second["review_id"]);
    assert_eq!(first["artifacts"], second["artifacts"]);
    let artifact_dir = root.join(".kiana/workflows").join(run_id);
    let evidence = read_evidence_events(&artifact_dir).unwrap();
    assert_eq!(evidence.len(), 1);
    let artifact_events = read_workflow_events(&artifact_dir)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == WorkflowEventKind::ArtifactWritten)
        .count();
    assert_eq!(artifact_events, 1);
    let packets = list_verification_packets(&artifact_dir).unwrap();
    assert_eq!(packets.len(), 1);
    let verification_events = read_workflow_events(&artifact_dir)
        .unwrap()
        .into_iter()
        .filter(|event| event.kind == WorkflowEventKind::VerificationCompleted)
        .count();
    assert_eq!(verification_events, 1);

    let _ = fs::remove_dir_all(root);
}
