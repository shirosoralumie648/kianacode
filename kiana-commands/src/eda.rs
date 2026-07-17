use crate::eda_netlist::{analyze_kicad_netlist, NetlistAnalysis};
use crate::types::{
    Command, CommandContext, CommandResult, CommandType, COMMAND_ARGV_APP_STATE_KEY,
};
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use kiana_tasks::{
    append_evidence_event, build_verification_packet, commit_immutable_artifacts_with_unique_event,
    initialize_workflow_run, list_verification_packets, read_evidence_events, resume_workflow_run,
    sha256_prefixed, validate_verification_packet_completion,
    validate_verification_packet_integrity, write_verification_packet, EvidenceError,
    EvidenceEvent, EvidenceEventDraft, EvidenceKind, EvidenceSource, EvidenceStatus,
    VerificationCheck, WorkflowArtifactBatch, WorkflowArtifactInput, WorkflowEventKind,
    WorkflowInit, WorkflowInputKind, WorkflowProfile,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

const REPORT_SCHEMA: &str = "kiana.eda-review.v1";
const RULE_VERSION: &str = "eda-review-rules.v2";
const MAX_TEXT_BYTES: u64 = 16 * 1024 * 1024;
const MAX_GERBER_FILES: usize = 2048;
const MAX_GERBER_BYTES: u64 = 128 * 1024 * 1024;

pub struct EdaCommand;

#[async_trait]
impl Command for EdaCommand {
    fn name(&self) -> &str {
        "eda"
    }

    fn description(&self) -> &str {
        "Review bounded EDA project artifacts and emit workflow evidence"
    }

    fn command_type(&self) -> CommandType {
        CommandType::Local
    }

    fn supports_non_interactive(&self) -> bool {
        true
    }

    async fn execute(&self, context: CommandContext) -> Result<CommandResult> {
        let argv = command_context_argv(&context).unwrap_or_else(|| split_words(&context.args));
        if argv.is_empty()
            || argv
                .iter()
                .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
        {
            return Ok(CommandResult::text(usage()));
        }
        let options = parse_args(&argv)?;
        let project_root = project_root(&context)?;
        let sources = resolve_sources(&project_root, &options)?;
        let analysis = analyze_sources(&sources)?;
        let run = prepare_workflow(&project_root, options.workflow.as_deref())?;
        let review_id = derive_review_id(&sources);
        let relative_dir = format!("eda/reviews/{review_id}");
        let report_relative = format!("{relative_dir}/eda_review.json");
        let bom_relative = format!("{relative_dir}/bom_risk.md");
        let bringup_relative = format!("{relative_dir}/bringup-plan.md");
        let artifact_paths = vec![
            report_relative.clone(),
            bom_relative.clone(),
            bringup_relative.clone(),
        ];
        let workflow_id = run.workflow_id.clone();
        let run_id = run.run_id.clone();
        let sources_for_report = sources.descriptors();
        let analysis_for_report = analysis.clone();
        let artifact_paths_for_report = artifact_paths.clone();
        let review_id_for_commit = review_id.clone();

        let commit = commit_immutable_artifacts_with_unique_event(
            &run.artifact_dir,
            WorkflowEventKind::ArtifactWritten,
            "hardware_review",
            "review_id",
            &review_id,
            move |_sequence, at_ms| {
                let report = build_report(
                    &workflow_id,
                    &run_id,
                    &review_id_for_commit,
                    at_ms,
                    sources_for_report,
                    analysis_for_report,
                    artifact_paths_for_report,
                );
                let report_bytes = serde_json::to_vec_pretty(&report)?;
                let bom_risk = render_bom_risk(&report);
                let bringup_plan = render_bringup_plan(&report);
                Ok(WorkflowArtifactBatch {
                    artifacts: vec![
                        WorkflowArtifactInput {
                            relative_path: report_relative,
                            contents: report_bytes,
                        },
                        WorkflowArtifactInput {
                            relative_path: bom_relative,
                            contents: bom_risk.into_bytes(),
                        },
                        WorkflowArtifactInput {
                            relative_path: bringup_relative,
                            contents: bringup_plan.into_bytes(),
                        },
                    ],
                    event_data: json!({
                        "review_id": review_id_for_commit,
                        "domain": "eda",
                        "rule_version": RULE_VERSION,
                    }),
                })
            },
        )?;

        let report_path = run.artifact_dir.join(&artifact_paths[0]);
        let report: EdaReviewReport = serde_json::from_slice(&fs::read(&report_path)?)?;
        let evidence = record_evidence_once(&run, &report, &commit.artifact_paths)?;
        record_verification_once(&run, &report, &evidence)?;

        if options.json {
            return Ok(CommandResult::text(serde_json::to_string_pretty(&report)?));
        }
        Ok(CommandResult::text(render_human(&report)))
    }
}

#[derive(Debug, Default)]
struct EdaOptions {
    workflow: Option<String>,
    requirements: Option<PathBuf>,
    schematic: Option<PathBuf>,
    bom: Option<PathBuf>,
    gerber: Option<PathBuf>,
    cpl: Option<PathBuf>,
    constraints: Option<PathBuf>,
    netlist: Option<PathBuf>,
    json: bool,
}

#[derive(Debug)]
struct PreparedRun {
    workflow_id: String,
    run_id: String,
    artifact_dir: PathBuf,
}

#[derive(Debug, Clone)]
struct ResolvedArtifact {
    kind: &'static str,
    relative_path: String,
    size: u64,
    sha256: String,
    media_type: &'static str,
    contents: Vec<u8>,
    directory_entries: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct ResolvedSources {
    requirements: Option<ResolvedArtifact>,
    schematic: Option<ResolvedArtifact>,
    bom: Option<ResolvedArtifact>,
    gerber: Option<ResolvedArtifact>,
    cpl: Option<ResolvedArtifact>,
    constraints: Option<ResolvedArtifact>,
    netlist: Option<ResolvedArtifact>,
}

impl ResolvedSources {
    fn ordered(&self) -> Vec<&ResolvedArtifact> {
        [
            self.requirements.as_ref(),
            self.schematic.as_ref(),
            self.bom.as_ref(),
            self.gerber.as_ref(),
            self.cpl.as_ref(),
            self.constraints.as_ref(),
            self.netlist.as_ref(),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn descriptors(&self) -> Vec<EdaSourceArtifact> {
        self.ordered()
            .into_iter()
            .map(|source| EdaSourceArtifact {
                kind: source.kind.to_string(),
                path: source.relative_path.clone(),
                size: source.size,
                sha256: source.sha256.clone(),
                media_type: source.media_type.to_string(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
struct Analysis {
    checks: Vec<EdaCheck>,
    findings: Vec<EdaFinding>,
    bom_rows: usize,
    cpl_rows: usize,
    gerber_files: usize,
    netlist_components: usize,
    net_count: usize,
    power_net_count: usize,
    interface_net_count: usize,
    dangling_net_count: usize,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct EdaReviewReport {
    schema: String,
    rule_version: String,
    workflow_id: String,
    run_id: String,
    review_id: String,
    created_at_ms: u64,
    status: EdaReviewStatus,
    sources: Vec<EdaSourceArtifact>,
    checks: Vec<EdaCheck>,
    findings: Vec<EdaFinding>,
    summary: EdaSummary,
    limitations: Vec<String>,
    approval_requirements: Vec<String>,
    next_action: String,
    artifacts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum EdaReviewStatus {
    Pass,
    ReviewRequired,
    Blocked,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct EdaSourceArtifact {
    kind: String,
    path: String,
    size: u64,
    sha256: String,
    media_type: String,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct EdaCheck {
    check_id: String,
    status: String,
    summary: String,
    evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct EdaFinding {
    code: String,
    severity: String,
    message: String,
    evidence: Vec<String>,
    recommendation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    designator: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct EdaSummary {
    blocked: u64,
    errors: u64,
    warnings: u64,
    info: u64,
    bom_rows: u64,
    cpl_rows: u64,
    gerber_files: u64,
    netlist_components: u64,
    net_count: u64,
    power_net_count: u64,
    interface_net_count: u64,
    dangling_net_count: u64,
}

fn parse_args(args: &[String]) -> Result<EdaOptions> {
    if args.first().map(String::as_str) != Some("review") {
        return Err(anyhow!("expected EDA subcommand 'review'\n\n{}", usage()));
    }
    let mut options = EdaOptions::default();
    let mut seen = BTreeSet::new();
    let mut index = 1;
    while index < args.len() {
        let token = args[index].as_str();
        match token {
            "--json" => {
                require_unique(&mut seen, token)?;
                options.json = true;
            }
            "--workflow" | "--requirements" | "--schematic" | "--bom" | "--gerber" | "--cpl"
            | "--constraints" | "--netlist" => {
                require_unique(&mut seen, token)?;
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| anyhow!("{token} requires a value"))?;
                match token {
                    "--workflow" => options.workflow = Some(value.clone()),
                    "--requirements" => options.requirements = Some(PathBuf::from(value)),
                    "--schematic" => options.schematic = Some(PathBuf::from(value)),
                    "--bom" => options.bom = Some(PathBuf::from(value)),
                    "--gerber" => options.gerber = Some(PathBuf::from(value)),
                    "--cpl" => options.cpl = Some(PathBuf::from(value)),
                    "--constraints" => options.constraints = Some(PathBuf::from(value)),
                    "--netlist" => options.netlist = Some(PathBuf::from(value)),
                    _ => unreachable!(),
                }
            }
            other => return Err(anyhow!("unknown EDA option '{other}'\n\n{}", usage())),
        }
        index += 1;
    }
    Ok(options)
}

fn require_unique(seen: &mut BTreeSet<String>, option: &str) -> Result<()> {
    if !seen.insert(option.to_string()) {
        return Err(anyhow!("duplicate option {option}"));
    }
    Ok(())
}

fn project_root(context: &CommandContext) -> Result<PathBuf> {
    let raw = context
        .app_state
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    let root = raw
        .canonicalize()
        .with_context(|| format!("failed to resolve project root {}", raw.display()))?;
    if !root.is_dir() {
        return Err(anyhow!("project root is not a directory"));
    }
    Ok(root)
}

fn resolve_sources(root: &Path, options: &EdaOptions) -> Result<ResolvedSources> {
    Ok(ResolvedSources {
        requirements: resolve_optional_file(
            root,
            "requirements",
            options.requirements.as_deref(),
            "text/markdown",
        )?,
        schematic: resolve_optional_file(
            root,
            "schematic",
            options.schematic.as_deref(),
            "application/octet-stream",
        )?,
        bom: resolve_optional_file(root, "bom", options.bom.as_deref(), "text/csv")?,
        gerber: resolve_optional_directory(root, "gerber", options.gerber.as_deref())?,
        cpl: resolve_optional_file(root, "cpl", options.cpl.as_deref(), "text/csv")?,
        constraints: resolve_optional_file(
            root,
            "constraints",
            options.constraints.as_deref(),
            "text/markdown",
        )?,
        netlist: resolve_optional_file(
            root,
            "netlist",
            options.netlist.as_deref(),
            "application/xml",
        )?,
    })
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(anyhow!(
            "EDA artifact path must be project-relative without '..': {}",
            path.display()
        ));
    }
    Ok(())
}

fn resolve_optional_file(
    root: &Path,
    kind: &'static str,
    path: Option<&Path>,
    media_type: &'static str,
) -> Result<Option<ResolvedArtifact>> {
    let Some(path) = path else { return Ok(None) };
    validate_relative_path(path)?;
    let candidate = root.join(path);
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("failed to resolve {kind} artifact {}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(anyhow!(
            "{kind} artifact escapes project root: {}",
            path.display()
        ));
    }
    if !canonical.is_file() {
        return Err(anyhow!("{kind} artifact is not a regular file"));
    }
    let bytes = read_bounded_file(&canonical, MAX_TEXT_BYTES, kind)?;
    Ok(Some(ResolvedArtifact {
        kind,
        relative_path: normalized_relative(root, &canonical)?,
        size: bytes.len() as u64,
        sha256: sha256_prefixed(&bytes),
        media_type,
        contents: bytes,
        directory_entries: Vec::new(),
    }))
}

fn resolve_optional_directory(
    root: &Path,
    kind: &'static str,
    path: Option<&Path>,
) -> Result<Option<ResolvedArtifact>> {
    let Some(path) = path else { return Ok(None) };
    validate_relative_path(path)?;
    let candidate = root.join(path);
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("failed to resolve {kind} artifact {}", path.display()))?;
    if !canonical.starts_with(root) {
        return Err(anyhow!(
            "{kind} artifact escapes project root: {}",
            path.display()
        ));
    }
    if !canonical.is_dir() {
        return Err(anyhow!(
            "{kind} artifact must be a directory in this release"
        ));
    }
    let (size, digest, directory_entries) = hash_gerber_directory(&canonical)?;
    Ok(Some(ResolvedArtifact {
        kind,
        relative_path: normalized_relative(root, &canonical)?,
        size,
        sha256: digest,
        media_type: "application/vnd.kiana.gerber-directory",
        contents: Vec::new(),
        directory_entries,
    }))
}

fn read_bounded_file(path: &Path, limit: u64, kind: &str) -> Result<Vec<u8>> {
    let file = fs::File::open(path)
        .with_context(|| format!("failed to open {kind} artifact {}", path.display()))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(anyhow!("{kind} artifact is not a regular file"));
    }
    if metadata.len() > limit {
        return Err(anyhow!("{kind} artifact exceeds {} byte limit", limit));
    }
    let mut bytes = Vec::with_capacity(metadata.len().min(limit) as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(anyhow!("{kind} artifact exceeds {} byte limit", limit));
    }
    Ok(bytes)
}

fn normalized_relative(root: &Path, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(root)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn prepare_workflow(root: &Path, requested_run_id: Option<&str>) -> Result<PreparedRun> {
    if let Some(run_id) = requested_run_id {
        let resume = resume_workflow_run(root, Some(run_id))?;
        if !resume.resume_status.can_continue() || !resume.eventlog_consistent {
            return Err(anyhow!(
                "EDA workflow cannot continue: {}",
                resume
                    .blocker
                    .unwrap_or_else(|| "workflow integrity blocked".to_string())
            ));
        }
        if resume.run.input_kind != WorkflowInputKind::Eda {
            return Err(anyhow!("workflow {run_id} is not an EDA workflow"));
        }
        return Ok(PreparedRun {
            workflow_id: resume.run.workflow_id,
            run_id: resume.run.run_id,
            artifact_dir: resume.run.artifact_dir,
        });
    }
    let run = initialize_workflow_run(
        root,
        WorkflowInit {
            request: "Review EDA project artifacts and produce bounded hardware evidence"
                .to_string(),
            input_kind: WorkflowInputKind::Eda,
            profile: WorkflowProfile::Gated,
            approval_required: false,
        },
    )?;
    Ok(PreparedRun {
        workflow_id: run.workflow_id,
        run_id: run.run_id,
        artifact_dir: run.artifact_dir,
    })
}

fn analyze_sources(sources: &ResolvedSources) -> Result<Analysis> {
    let mut analysis = Analysis::default();
    require_critical_source(&mut analysis, "schematic", sources.schematic.as_ref());
    require_critical_source(&mut analysis, "bom", sources.bom.as_ref());
    require_critical_source(&mut analysis, "gerber", sources.gerber.as_ref());

    if sources.requirements.is_none() {
        analysis.findings.push(finding(
            "requirements_missing",
            "warning",
            "No hardware requirements artifact was supplied.",
            vec![],
            "Add power, interface, environment, mechanical, cost, and assembly constraints.",
            None,
        ));
    }
    if sources.constraints.is_none() {
        analysis.findings.push(finding(
            "constraints_missing",
            "warning",
            "No fabrication or assembly constraints artifact was supplied.",
            vec![],
            "Record layer count, board stack, minimum geometry, assembly target, and safety constraints.",
            None,
        ));
    }

    let bom = if let Some(source) = sources.bom.as_ref() {
        let table = parse_component_csv(source, CsvKind::Bom)?;
        analysis.bom_rows = table.len();
        inspect_bom(&table, &mut analysis);
        Some(table)
    } else {
        None
    };
    let cpl = if let Some(source) = sources.cpl.as_ref() {
        let table = parse_component_csv(source, CsvKind::Cpl)?;
        analysis.cpl_rows = table.len();
        Some(table)
    } else {
        analysis.findings.push(finding(
            "cpl_missing",
            "warning",
            "No CPL/placement artifact was supplied, so assembly cross-checks were skipped.",
            vec![],
            "Export the placement file before assembly release.",
            None,
        ));
        None
    };
    if let (Some(bom), Some(cpl)) = (&bom, &cpl) {
        cross_check_bom_cpl(bom, cpl, &mut analysis);
    }
    let netlist = if let Some(source) = sources.netlist.as_ref() {
        let parsed = analyze_kicad_netlist(&source.contents)
            .map_err(|error| anyhow!("failed to analyze {}: {error}", source.relative_path))?;
        analysis.netlist_components = parsed.component_count;
        analysis.net_count = parsed.net_count;
        analysis.power_net_count = parsed.power_net_count;
        analysis.interface_net_count = parsed.interface_net_count;
        analysis.dangling_net_count = parsed.dangling_net_count;
        inspect_netlist(&parsed, bom.as_ref(), &mut analysis);
        Some(parsed)
    } else {
        None
    };
    if let Some(gerber) = sources.gerber.as_ref() {
        inspect_gerber(gerber, &mut analysis)?;
    }
    analysis.checks.push(EdaCheck {
        check_id: "schematic_presence".to_string(),
        status: if sources.schematic.is_some() {
            "pass"
        } else {
            "blocked"
        }
        .to_string(),
        summary: "Schematic artifact presence and digest".to_string(),
        evidence: sources
            .schematic
            .iter()
            .map(|source| source.relative_path.clone())
            .collect(),
    });
    analysis.checks.push(EdaCheck {
        check_id: "bom_cpl_structure".to_string(),
        status: if sources.bom.is_some() && sources.cpl.is_some() {
            "evaluated"
        } else {
            "partial"
        }
        .to_string(),
        summary: "BOM/CPL designator and package consistency".to_string(),
        evidence: [sources.bom.as_ref(), sources.cpl.as_ref()]
            .into_iter()
            .flatten()
            .map(|source| source.relative_path.clone())
            .collect(),
    });
    analysis.checks.push(EdaCheck {
        check_id: "netlist_structure".to_string(),
        status: match netlist.as_ref() {
            None => "not_supplied",
            Some(netlist) if netlist.issues.iter().any(|issue| issue.severity == "error") => "fail",
            Some(netlist)
                if netlist
                    .issues
                    .iter()
                    .any(|issue| issue.severity == "warning") =>
            {
                "review"
            }
            Some(_) => "pass",
        }
        .to_string(),
        summary: "KiCad XML netlist component, connectivity, power, and interface structure"
            .to_string(),
        evidence: sources
            .netlist
            .iter()
            .map(|source| source.relative_path.clone())
            .collect(),
    });
    Ok(analysis)
}

fn inspect_netlist(
    netlist: &NetlistAnalysis,
    bom: Option<&BTreeMap<String, ComponentRow>>,
    analysis: &mut Analysis,
) {
    for issue in &netlist.issues {
        analysis.findings.push(finding(
            issue.code,
            issue.severity,
            &issue.message,
            issue.evidence.clone(),
            "Resolve the structural netlist issue in the source design and rerun the review.",
            issue.designator.clone(),
        ));
    }

    let Some(bom) = bom else {
        return;
    };
    for designator in netlist
        .component_refs
        .difference(&bom.keys().cloned().collect())
    {
        analysis.findings.push(finding(
            "netlist_component_missing_from_bom",
            "error",
            &format!("Netlist component {designator} is missing from the BOM."),
            vec![designator.clone()],
            "Add the component to the BOM or remove the stale component from the design.",
            Some(designator.clone()),
        ));
    }
    for designator in bom.keys() {
        if !netlist.component_refs.contains(designator) {
            analysis.findings.push(finding(
                "bom_component_missing_from_netlist",
                "warning",
                &format!("BOM component {designator} is missing from the netlist."),
                vec![designator.clone()],
                "Regenerate the BOM and netlist from the same design revision.",
                Some(designator.clone()),
            ));
        }
    }
}

fn require_critical_source(
    analysis: &mut Analysis,
    kind: &'static str,
    source: Option<&ResolvedArtifact>,
) {
    if source.is_none() {
        analysis.findings.push(finding(
            &format!("{kind}_missing"),
            "blocked",
            &format!("Required {kind} artifact was not supplied."),
            vec![],
            &format!("Supply a project-relative {kind} artifact and rerun the review."),
            None,
        ));
    }
}

#[derive(Debug, Clone, Copy)]
enum CsvKind {
    Bom,
    Cpl,
}

#[derive(Debug, Clone)]
struct ComponentRow {
    designator: String,
    package: String,
    mpn: String,
}

fn parse_component_csv(
    source: &ResolvedArtifact,
    kind: CsvKind,
) -> Result<BTreeMap<String, ComponentRow>> {
    let text = std::str::from_utf8(&source.contents)
        .with_context(|| format!("{} CSV must be valid UTF-8", source.kind))?;
    let mut lines = text.lines();
    let header = lines
        .next()
        .ok_or_else(|| anyhow!("{} CSV is empty", source.kind))?;
    let headers = parse_csv_line(header)?;
    let normalized = headers
        .iter()
        .map(|value| normalize_header(value))
        .collect::<Vec<_>>();
    let designator_index = find_header(&normalized, &["designator", "reference", "refdes"])
        .ok_or_else(|| anyhow!("{} CSV is missing a designator column", source.kind))?;
    let package_index = find_header(&normalized, &["package", "footprint"])
        .ok_or_else(|| anyhow!("{} CSV is missing a package/footprint column", source.kind))?;
    let mpn_index = find_header(
        &normalized,
        &["mpn", "manufacturerpartnumber", "manufacturerpart"],
    );
    let mut rows = BTreeMap::new();
    for (offset, line) in lines.enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let fields = parse_csv_line(line)
            .with_context(|| format!("invalid {} CSV row {}", source.kind, offset + 2))?;
        let designator = fields
            .get(designator_index)
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("{} CSV row {} has no designator", source.kind, offset + 2))?;
        let package = fields
            .get(package_index)
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        let mpn = mpn_index
            .and_then(|index| fields.get(index))
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        let row = ComponentRow {
            designator: designator.clone(),
            package,
            mpn,
        };
        if rows.insert(designator.clone(), row).is_some() {
            return Err(anyhow!(
                "{} CSV contains duplicate designator {designator}",
                match kind {
                    CsvKind::Bom => "BOM",
                    CsvKind::Cpl => "CPL",
                }
            ));
        }
    }
    Ok(rows)
}

fn parse_csv_line(line: &str) -> Result<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                field.push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => {
                fields.push(field.trim().to_string());
                field.clear();
            }
            _ => field.push(ch),
        }
    }
    if quoted {
        return Err(anyhow!("unterminated quoted CSV field"));
    }
    fields.push(field.trim().to_string());
    Ok(fields)
}

fn normalize_header(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn find_header(headers: &[String], candidates: &[&str]) -> Option<usize> {
    headers
        .iter()
        .position(|header| candidates.iter().any(|candidate| header == candidate))
}

fn inspect_bom(rows: &BTreeMap<String, ComponentRow>, analysis: &mut Analysis) {
    for row in rows.values() {
        if row.package.trim().is_empty() {
            analysis.findings.push(finding(
                "bom_package_missing",
                "error",
                &format!("BOM component {} has no package.", row.designator),
                vec![row.designator.clone()],
                "Assign and verify the PCB footprint before fabrication release.",
                Some(row.designator.clone()),
            ));
        }
        if row.mpn.trim().is_empty() {
            analysis.findings.push(finding(
                "bom_mpn_missing",
                "warning",
                &format!(
                    "BOM component {} has no manufacturer part number.",
                    row.designator
                ),
                vec![row.designator.clone()],
                "Add an auditable MPN and approved alternatives.",
                Some(row.designator.clone()),
            ));
        }
    }
}

fn cross_check_bom_cpl(
    bom: &BTreeMap<String, ComponentRow>,
    cpl: &BTreeMap<String, ComponentRow>,
    analysis: &mut Analysis,
) {
    for (designator, bom_row) in bom {
        let Some(cpl_row) = cpl.get(designator) else {
            analysis.findings.push(finding(
                "bom_designator_missing_from_cpl",
                "error",
                &format!("BOM designator {designator} is missing from CPL."),
                vec![designator.clone()],
                "Regenerate CPL or mark the component as intentionally not fitted.",
                Some(designator.clone()),
            ));
            continue;
        };
        if !bom_row.package.eq_ignore_ascii_case(&cpl_row.package) {
            analysis.findings.push(finding(
                "bom_cpl_package_mismatch",
                "error",
                &format!(
                    "Package mismatch for {designator}: BOM='{}', CPL='{}'.",
                    bom_row.package, cpl_row.package
                ),
                vec![bom_row.package.clone(), cpl_row.package.clone()],
                "Resolve the footprint mapping and regenerate assembly outputs.",
                Some(designator.clone()),
            ));
        }
    }
    for designator in cpl.keys() {
        if !bom.contains_key(designator) {
            analysis.findings.push(finding(
                "cpl_designator_missing_from_bom",
                "error",
                &format!("CPL designator {designator} is missing from BOM."),
                vec![designator.clone()],
                "Regenerate BOM/CPL from the same design revision.",
                Some(designator.clone()),
            ));
        }
    }
}

fn inspect_gerber(source: &ResolvedArtifact, analysis: &mut Analysis) -> Result<()> {
    let files = source.directory_entries.clone();
    analysis.gerber_files = files.len();
    let lower = files
        .iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let has_copper = lower.iter().any(|path| {
        path.contains("f_cu")
            || path.contains("b_cu")
            || path.ends_with(".gtl")
            || path.ends_with(".gbl")
    });
    let has_edge = lower.iter().any(|path| {
        path.contains("edge_cuts") || path.contains("outline") || path.ends_with(".gko")
    });
    let has_drill = lower
        .iter()
        .any(|path| path.ends_with(".drl") || path.ends_with(".xln"));
    for (code, present, description) in [
        ("gerber_copper_missing", has_copper, "copper layer"),
        ("gerber_outline_missing", has_edge, "board outline"),
        ("gerber_drill_missing", has_drill, "drill file"),
    ] {
        if !present {
            analysis.findings.push(finding(
                code,
                "error",
                &format!("Gerber directory has no recognizable {description}."),
                files.clone(),
                "Regenerate a complete fabrication package from the same board revision.",
                None,
            ));
        }
    }
    analysis.checks.push(EdaCheck {
        check_id: "gerber_file_set".to_string(),
        status: if has_copper && has_edge && has_drill {
            "pass"
        } else {
            "fail"
        }
        .to_string(),
        summary: "Gerber copper, outline, and drill file presence".to_string(),
        evidence: files,
    });
    Ok(())
}

fn hash_gerber_directory(directory: &Path) -> Result<(u64, String, Vec<String>)> {
    let canonical_directory = directory.canonicalize()?;
    let mut entries = fs::read_dir(&canonical_directory)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    let mut digest = Sha256::new();
    let mut total = 0_u64;
    let mut names = Vec::new();
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(anyhow!("Gerber directory contains a symlink"));
        }
        if metadata.is_dir() {
            return Err(anyhow!(
                "Gerber directory must not contain nested directories"
            ));
        }
        if !metadata.is_file() {
            return Err(anyhow!("Gerber directory contains a non-regular entry"));
        }
        if names.len() >= MAX_GERBER_FILES {
            return Err(anyhow!("Gerber directory exceeds {MAX_GERBER_FILES} files"));
        }
        let canonical_path = path.canonicalize()?;
        if canonical_path.parent() != Some(canonical_directory.as_path()) {
            return Err(anyhow!("Gerber entry escapes the selected directory"));
        }
        let remaining = MAX_GERBER_BYTES
            .checked_sub(total)
            .ok_or_else(|| anyhow!("Gerber directory exceeds 128 MiB limit"))?;
        let bytes = read_bounded_file(&canonical_path, remaining, "Gerber entry")?;
        total = total
            .checked_add(bytes.len() as u64)
            .ok_or_else(|| anyhow!("Gerber byte count overflow"))?;
        if total > MAX_GERBER_BYTES {
            return Err(anyhow!("Gerber directory exceeds 128 MiB limit"));
        }
        let name = entry.file_name().to_string_lossy().to_string();
        digest.update(name.as_bytes());
        digest.update([0]);
        digest.update(&bytes);
        digest.update([0]);
        names.push(name);
    }
    Ok((total, format!("sha256:{:x}", digest.finalize()), names))
}

fn finding(
    code: &str,
    severity: &str,
    message: &str,
    evidence: Vec<String>,
    recommendation: &str,
    designator: Option<String>,
) -> EdaFinding {
    EdaFinding {
        code: code.to_string(),
        severity: severity.to_string(),
        message: message.to_string(),
        evidence,
        recommendation: recommendation.to_string(),
        designator,
    }
}

fn derive_review_id(sources: &ResolvedSources) -> String {
    let mut material = format!("{RULE_VERSION}\n");
    for kind in [
        "requirements",
        "schematic",
        "bom",
        "gerber",
        "cpl",
        "constraints",
        "netlist",
    ] {
        let source = sources
            .ordered()
            .into_iter()
            .find(|source| source.kind == kind);
        if let Some(source) = source {
            material.push_str(&format!(
                "{}\t{}\t{}\n",
                source.kind, source.relative_path, source.sha256
            ));
        } else {
            material.push_str(&format!("{kind}\tmissing\n"));
        }
    }
    let digest = sha256_prefixed(material.as_bytes());
    format!(
        "eda-{}",
        digest
            .trim_start_matches("sha256:")
            .chars()
            .take(20)
            .collect::<String>()
    )
}

fn build_report(
    workflow_id: &str,
    run_id: &str,
    review_id: &str,
    created_at_ms: u64,
    sources: Vec<EdaSourceArtifact>,
    analysis: Analysis,
    artifacts: Vec<String>,
) -> EdaReviewReport {
    let blocked = count_severity(&analysis.findings, "blocked");
    let errors = count_severity(&analysis.findings, "error");
    let warnings = count_severity(&analysis.findings, "warning");
    let info = count_severity(&analysis.findings, "info");
    let status = if blocked > 0 {
        EdaReviewStatus::Blocked
    } else if errors > 0 {
        EdaReviewStatus::ReviewRequired
    } else {
        EdaReviewStatus::Pass
    };
    EdaReviewReport {
        schema: REPORT_SCHEMA.to_string(),
        rule_version: RULE_VERSION.to_string(),
        workflow_id: workflow_id.to_string(),
        run_id: run_id.to_string(),
        review_id: review_id.to_string(),
        created_at_ms,
        status,
        sources,
        checks: analysis.checks,
        findings: analysis.findings,
        summary: EdaSummary {
            blocked,
            errors,
            warnings,
            info,
            bom_rows: analysis.bom_rows as u64,
            cpl_rows: analysis.cpl_rows as u64,
            gerber_files: analysis.gerber_files as u64,
            netlist_components: analysis.netlist_components as u64,
            net_count: analysis.net_count as u64,
            power_net_count: analysis.power_net_count as u64,
            interface_net_count: analysis.interface_net_count as u64,
            dangling_net_count: analysis.dangling_net_count as u64,
        },
        limitations: vec![
            "Schematic content is not electrically parsed; this review proves artifact presence and digest only.".to_string(),
            "KiCad XML netlist review is bounded to structural component, connectivity, power, and interface heuristics; it is not a full ERC.".to_string(),
            "No PCB geometry, CAM, SI/PI, thermal, safety certification, or manufacturing sign-off is performed.".to_string(),
            "No live inventory, lifecycle, price, supplier, or lead-time claim is made without an external verified source.".to_string(),
            "Kiana does not modify the design, generate production files, place orders, or replace engineering approval.".to_string(),
        ],
        approval_requirements: vec![
            "hardware_order".to_string(),
            "cost_incurred".to_string(),
            "production_file_mutation".to_string(),
            "automatic_component_replacement".to_string(),
        ],
        next_action: match status {
            EdaReviewStatus::Pass => "engineer_review_then_bringup".to_string(),
            EdaReviewStatus::ReviewRequired => "resolve_findings_and_rerun".to_string(),
            EdaReviewStatus::Blocked => "supply_required_artifacts".to_string(),
        },
        artifacts,
    }
}

fn count_severity(findings: &[EdaFinding], severity: &str) -> u64 {
    findings
        .iter()
        .filter(|finding| finding.severity == severity)
        .count() as u64
}

fn render_bom_risk(report: &EdaReviewReport) -> String {
    let mut output = format!(
        "# BOM Risk Review\n\n- Review: `{}`\n- Status: `{:?}`\n- BOM rows: {}\n- CPL rows: {}\n\n## Findings\n",
        report.review_id, report.status, report.summary.bom_rows, report.summary.cpl_rows
    );
    let findings = report
        .findings
        .iter()
        .filter(|finding| finding.code.starts_with("bom_") || finding.code.starts_with("cpl_"))
        .collect::<Vec<_>>();
    if findings.is_empty() {
        output.push_str(
            "\n- No structural BOM/CPL inconsistency detected by the bounded local rules.\n",
        );
    } else {
        for finding in findings {
            output.push_str(&format!(
                "\n- **{} / {}**: {} Recommendation: {}\n",
                finding.severity, finding.code, finding.message, finding.recommendation
            ));
        }
    }
    output.push_str("\n## Limits\n\n- Availability, lifecycle, pricing, and approved substitutes require verified external data.\n");
    output
}

fn render_bringup_plan(report: &EdaReviewReport) -> String {
    format!(
        "# Bring-up Plan\n\n- Review: `{}`\n- Review status: `{:?}`\n- Approval boundary: no ordering or production-file mutation was performed.\n\n## Preconditions\n\n1. Resolve every blocked/error finding and archive the revised review.\n2. Obtain engineer approval for schematic, PCB, BOM, and fabrication outputs.\n3. Confirm current limiting and safe bench setup before applying power.\n\n## Sequence\n\n1. Perform unpowered visual, continuity, and short-circuit inspection.\n2. Apply current-limited input power and verify input protection behavior.\n3. Measure each rail, startup order, ripple, and regulator temperature.\n4. Verify reset, boot straps, clocks, programming, and debug access.\n5. Bring up interfaces one at a time with known-good fixtures.\n6. Record measurements, failures, rework, firmware revision, and board serial as Evidence Ledger entries.\n\n## Stop Conditions\n\n- Unexpected current draw, rail deviation, overheating, smoke, unstable clock/reset, or unsafe touch voltage.\n- Any unresolved electrical, DFM, or component identity discrepancy.\n",
        report.review_id, report.status
    )
}

fn record_evidence_once(
    run: &PreparedRun,
    report: &EdaReviewReport,
    artifact_paths: &[String],
) -> Result<EvidenceEvent> {
    let event_id = format!("eda_{}", report.review_id);
    let status = match report.status {
        EdaReviewStatus::Pass => EvidenceStatus::Pass,
        EdaReviewStatus::ReviewRequired => EvidenceStatus::Fail,
        EdaReviewStatus::Blocked => EvidenceStatus::Blocked,
    };
    let draft = EvidenceEventDraft {
        event_id: Some(event_id.clone()),
        workflow_id: run.workflow_id.clone(),
        run_id: run.run_id.clone(),
        task_id: None,
        workpacket_id: None,
        recorded_at_ms: Some(report.created_at_ms),
        kind: EvidenceKind::EdaCheck,
        status,
        summary: format!(
            "EDA review {} finished with {:?}",
            report.review_id, report.status
        ),
        source: EvidenceSource {
            source_type: "eda_analyzer".to_string(),
            name: "kiana-eda-review".to_string(),
            actor: Some("kiana".to_string()),
        },
        payload: json!({
            "schema": report.schema,
            "review_id": report.review_id,
            "rule_version": report.rule_version,
            "status": report.status,
            "summary": report.summary,
            "artifacts": artifact_paths,
            "approval_requirements": report.approval_requirements,
        }),
        changed_files: artifact_paths.to_vec(),
        confidence: Some(0.85),
        severity: Some(
            match report.status {
                EdaReviewStatus::Pass => "info",
                EdaReviewStatus::ReviewRequired => "high",
                EdaReviewStatus::Blocked => "blocking",
            }
            .to_string(),
        ),
        next_action: Some(report.next_action.clone()),
        supersedes_event_id: None,
    };
    match append_evidence_event(&run.artifact_dir, draft.clone()) {
        Ok(event) => Ok(event),
        Err(EvidenceError::DuplicateEventId(_)) => {
            let existing = read_evidence_events(&run.artifact_dir)?
                .into_iter()
                .find(|event| event.event_id == event_id)
                .ok_or_else(|| anyhow!("duplicate EDA evidence event was not readable"))?;
            if !evidence_matches_draft(&existing, &draft) {
                return Err(anyhow!("existing EDA evidence identity is inconsistent"));
            }
            Ok(existing)
        }
        Err(error) => Err(error.into()),
    }
}

fn evidence_matches_draft(event: &EvidenceEvent, draft: &EvidenceEventDraft) -> bool {
    event.event_id == draft.event_id.as_deref().unwrap_or_default()
        && event.workflow_id == draft.workflow_id
        && event.run_id == draft.run_id
        && event.task_id == draft.task_id
        && event.workpacket_id == draft.workpacket_id
        && Some(event.recorded_at_ms) == draft.recorded_at_ms
        && event.kind == draft.kind
        && event.status == draft.status
        && event.summary == draft.summary
        && event.source == draft.source
        && event.payload == draft.payload
        && event.changed_files == draft.changed_files
        && event.confidence == draft.confidence
        && event.severity == draft.severity
        && event.next_action == draft.next_action
        && event.supersedes_event_id == draft.supersedes_event_id
}

fn record_verification_once(
    run: &PreparedRun,
    report: &EdaReviewReport,
    evidence: &EvidenceEvent,
) -> Result<()> {
    let events = read_evidence_events(&run.artifact_dir)?;
    let mut packet = build_verification_packet(
        run.workflow_id.clone(),
        run.run_id.clone(),
        None,
        "eda_review",
        vec![VerificationCheck {
            check_id: "eda_review_gate".to_string(),
            description: "Bounded EDA review completed with immutable evidence".to_string(),
            required: true,
            status: evidence.status,
            evidence_id: Some(evidence.event_id.clone()),
            command: None,
            reason: None,
        }],
        &events,
    )?;
    packet.verification_id = format!("vp_{}", report.review_id.replace('-', "_"));
    packet.created_at_ms = report.created_at_ms;
    validate_verification_packet_integrity(&packet, &events)?;

    if let Some((path, existing)) = list_verification_packets(&run.artifact_dir)?
        .into_iter()
        .find(|(_, existing)| existing.verification_id == packet.verification_id)
    {
        if existing != packet {
            return Err(anyhow!(
                "existing EDA verification packet identity is inconsistent"
            ));
        }
        validate_verification_packet_completion(&run.artifact_dir, &path, &existing)?;
        return Ok(());
    }

    match write_verification_packet(&run.artifact_dir, &packet) {
        Ok(path) => {
            validate_verification_packet_completion(&run.artifact_dir, &path, &packet)?;
            Ok(())
        }
        Err(EvidenceError::DuplicateVerificationPacket(_)) => {
            let (path, existing) = list_verification_packets(&run.artifact_dir)?
                .into_iter()
                .find(|(_, existing)| existing.verification_id == packet.verification_id)
                .ok_or_else(|| anyhow!("duplicate EDA verification packet was not readable"))?;
            if existing != packet {
                return Err(anyhow!(
                    "existing EDA verification packet identity is inconsistent"
                ));
            }
            validate_verification_packet_completion(&run.artifact_dir, &path, &existing)?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn render_human(report: &EdaReviewReport) -> String {
    format!(
        "EDA review {}\nstatus: {:?}\nblocked: {}\nerrors: {}\nwarnings: {}\nartifacts: {}",
        report.review_id,
        report.status,
        report.summary.blocked,
        report.summary.errors,
        report.summary.warnings,
        report.artifacts.join(", ")
    )
}

fn command_context_argv(context: &CommandContext) -> Option<Vec<String>> {
    context
        .app_state
        .get(COMMAND_ARGV_APP_STATE_KEY)?
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect()
        })
}

fn split_words(input: &str) -> Vec<String> {
    input.split_whitespace().map(ToString::to_string).collect()
}

fn usage() -> &'static str {
    "Usage: kiana eda review [--workflow <run_id>] [--requirements <path>] [--schematic <path>] [--bom <path>] [--gerber <directory>] [--cpl <path>] [--constraints <path>] [--netlist <path>] [--json]\n\nCreates or resumes a gated EDA WorkflowRun and emits bounded review evidence. KiCad XML netlists receive structural checks only; this release does not edit designs, run full ERC/CAM, order hardware, or replace engineer approval."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_bom_and_cpl_are_analyzed_from_the_hashed_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "kiana-eda-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("hardware/gerber")).unwrap();
        fs::write(root.join("hardware/main.kicad_sch"), "schematic\n").unwrap();
        fs::write(
            root.join("hardware/bom.csv"),
            "Designator,MPN,Package,Quantity\nU1,STM32F103C8T6,LQFP48,1\n",
        )
        .unwrap();
        fs::write(
            root.join("hardware/cpl.csv"),
            "Designator,Package,Mid X,Mid Y,Rotation,Layer\nU1,LQFP48,20,15,90,Top\n",
        )
        .unwrap();
        fs::write(
            root.join("hardware/main.xml"),
            r#"<export>
  <components><comp ref="U1" /></components>
  <nets><net code="1" name="SWDIO"><node ref="U1" pin="1" pintype="bidirectional" /></net></nets>
</export>"#,
        )
        .unwrap();
        fs::write(root.join("hardware/gerber/demo-F_Cu.gbr"), "G04 copper*\n").unwrap();
        fs::write(
            root.join("hardware/gerber/demo-Edge_Cuts.gbr"),
            "G04 edge*\n",
        )
        .unwrap();
        fs::write(root.join("hardware/gerber/demo-PTH.drl"), "M48\n").unwrap();
        let canonical_root = root.canonicalize().unwrap();
        let sources = resolve_sources(
            &canonical_root,
            &EdaOptions {
                schematic: Some(PathBuf::from("hardware/main.kicad_sch")),
                bom: Some(PathBuf::from("hardware/bom.csv")),
                gerber: Some(PathBuf::from("hardware/gerber")),
                cpl: Some(PathBuf::from("hardware/cpl.csv")),
                netlist: Some(PathBuf::from("hardware/main.xml")),
                ..EdaOptions::default()
            },
        )
        .unwrap();
        fs::write(
            canonical_root.join("hardware/cpl.csv"),
            "Designator,Package,Mid X,Mid Y,Rotation,Layer\nU1,QFN48,20,15,90,Top\n",
        )
        .unwrap();
        fs::write(canonical_root.join("hardware/main.xml"), "<invalid>").unwrap();

        let analysis = analyze_sources(&sources).unwrap();

        assert!(!analysis
            .findings
            .iter()
            .any(|finding| finding.code == "bom_cpl_package_mismatch"));
        assert_eq!(analysis.netlist_components, 1);
        let _ = fs::remove_dir_all(root);
    }
}
