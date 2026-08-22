#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

mode="${1:-full}"
if [[ "$mode" != "full" && "$mode" != "--local-rc" ]]; then
  echo "usage: $0 [--local-rc]" >&2
  exit 2
fi

failures=0
bash_bin="${BASH:-bash}"

fail() {
  echo "FAIL: $*" >&2
  failures=$((failures + 1))
}

pass() {
  echo "OK: $*"
}

require_file() {
  if [[ -f "$1" ]]; then
    pass "file exists: $1"
  else
    fail "missing file: $1"
  fi
}

require_manifest_field() {
  local manifest="$1"
  local pattern="$2"
  if grep -Eq "$pattern" "$manifest"; then
    return 0
  fi
  fail "$manifest missing pattern: $pattern"
}

for file in \
  VERSION \
  LICENSE-MIT \
  LICENSE-APACHE \
  deny.toml \
  kiana-commands/src/eval.rs \
  kiana-commands/src/eda.rs \
  kiana-commands/src/memory.rs \
  kiana-commands/tests/eval_command.rs \
  kiana-commands/tests/eda_command.rs \
  kiana-entrypoints/tests/cli_eval.rs \
  kiana-tasks/src/workflow.rs \
  kiana-commands/src/tasks.rs \
  kiana-commands/tests/workflow_transition_command.rs \
  scripts/release-smoke.sh \
  scripts/package-release.sh \
  scripts/install-release-binary.sh \
  scripts/package-lifecycle-smoke.sh \
  scripts/product-shell-smoke.sh \
  scripts/validate-json-schema.py \
  scripts/schema-contract-smoke.sh \
  scripts/commercial-release-blockers-report.sh \
  scripts/source-control-proof-report.sh \
  scripts/distribution-review-report.sh \
  scripts/local-rc-evidence-report.sh \
  scripts/commercial-release-handoff-smoke.sh \
  scripts/stage-commercial-release-proofs.sh \
  scripts/entitlement-proof-report.sh \
  scripts/product-acceptance-report.sh \
  scripts/release-ops-report.sh \
  scripts/platform-security-proof-report.sh \
  scripts/provider-live-smoke.sh \
  scripts/remote-live-smoke.sh \
  scripts/sign-release-artifacts.sh \
  scripts/verify-commercial-release-artifacts.sh \
  scripts/release-signature-verification-smoke.sh \
  scripts/generate-sbom.sh \
  scripts/compliance-audit.sh \
  scripts/install-compliance-tools.sh \
  scripts/generate-distribution-manifests.sh \
  .github/workflows/release-smoke.yml \
  .github/workflows/release.yml
do
  require_file "$file"
done

if grep -Fq 'pub mod eval;' kiana-commands/src/lib.rs &&
  grep -Fq 'EvalCommand' kiana-commands/src/registry.rs &&
  grep -Fq 'kiana.eval-baseline.v1' scripts/package-lifecycle-smoke.sh &&
  grep -Fq -- '--baseline "$baseline"' scripts/release-smoke.sh &&
  grep -Fq 'kiana.eval-report.v1' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'smoke_eval_json "$release_bin"' scripts/release-smoke.sh &&
  grep -Fq 'smoke_eval_json "$installed_bin"' scripts/release-smoke.sh; then
  pass "offline eval command, packaged fixtures, and release smoke are wired"
else
  fail "offline eval command, packaged fixtures, or release smoke is not wired"
fi

if grep -Fq 'pub mod eda;' kiana-commands/src/lib.rs &&
  grep -Fq 'EdaCommand' kiana-commands/src/registry.rs &&
  grep -Fq 'smoke_eda_json "$release_bin"' scripts/release-smoke.sh &&
  grep -Fq 'smoke_eda_json "$installed_bin"' scripts/release-smoke.sh &&
  grep -Fq 'packaged EDA review did not pass' scripts/package-lifecycle-smoke.sh; then
  pass "EDA review command, schema, packaged smoke, and installed smoke are wired"
else
  fail "EDA review release wiring is incomplete"
fi

if grep -Fq 'mod swarm_process_identity;' kiana-commands/src/lib.rs &&
  grep -Fq 'kiana.swarm-process-identity-backend.v1' kiana-commands/src/swarm_process_identity.rs &&
  grep -Fq 'unsupported_platform' kiana-commands/src/swarm_process_identity.rs &&
  grep -Fq 'process_identity_backend' kiana-commands/src/tasks.rs &&
  grep -Fq 'process_identity_backend' kiana-commands/tests/swarm_command.rs &&
  grep -Fq 'kiana.swarm-worker-telemetry.v1' kiana-commands/src/tasks.rs &&
  grep -Fq 'kiana.swarm-worker-telemetry.v1' kiana-commands/tests/swarm_command.rs &&
  grep -Fq 'kiana.swarm-worker-health.v1' kiana-commands/src/tasks.rs &&
  grep -Fq 'attention_required' kiana-commands/tests/swarm_command.rs &&
  grep -Fq 'swarm_monitor_retries_worker_failed_until_success' kiana-commands/tests/swarm_command.rs &&
  grep -Fq 'process_identity_mismatch' kiana-commands/tests/swarm_command.rs &&
  grep -Fq 'command_digest_is_provenance_not_process_continuity' kiana-commands/src/swarm_process_identity.rs; then
  pass "bounded swarm process identity backend, telemetry, automatic retry, and health reporting tests are wired"
else
  fail "bounded swarm process identity backend, telemetry, automatic retry, or health reporting wiring is incomplete"
fi

if grep -Fq 'kiana.memory-record.v1' kiana-commands/src/memory.rs &&
  grep -Fq 'kiana.memory-status.v1' kiana-commands/src/memory.rs &&
  grep -Fq 'kiana.memory-search.v1' kiana-commands/src/memory.rs &&
  grep -Fq 'redaction_count' kiana-commands/src/memory.rs &&
  grep -Fq 'memory_append_redacts_obvious_secrets_before_persistence' kiana-commands/src/memory.rs &&
  grep -Fq 'memory_append_status_and_search_json_use_structured_store' kiana-commands/src/memory.rs; then
  pass "local structured memory schemas, redaction, and CLI tests are wired"
else
  fail "local structured memory schema, redaction, or CLI test wiring is incomplete"
fi

if [[ "${KIANA_PREFLIGHT_SKIP_COMPLIANCE:-}" == "1" ]]; then
  pass "compliance audit skipped by KIANA_PREFLIGHT_SKIP_COMPLIANCE=1"
elif [[ "$mode" == "full" ]]; then
  if "$bash_bin" scripts/compliance-audit.sh; then
    pass "full compliance audit passed"
  else
    fail "full compliance audit failed"
  fi
else
  if "$bash_bin" scripts/compliance-audit.sh --local-rc; then
    pass "local RC compliance audit passed"
  else
    fail "local RC compliance audit failed"
  fi
fi

if [[ "$mode" == "full" ]]; then
  if "$bash_bin" scripts/source-control-proof-report.sh full; then
    pass "source-control proof gate passed"
  else
    fail "source-control proof gate failed"
  fi
else
  if "$bash_bin" scripts/source-control-proof-report.sh --local-rc; then
    pass "local RC source-control proof generated"
  else
    fail "local RC source-control proof generation failed"
  fi
fi

if git check-ignore -q .kiana/tasks/default/1.json; then
  pass "local task state is ignored"
else
  fail ".kiana/tasks/default/1.json is not ignored"
fi

if git check-ignore -q .claude/ralph-loop.local.md; then
  pass "local assistant state is ignored"
else
  fail ".claude/ralph-loop.local.md is not ignored"
fi

if git check-ignore -q reference/claude-code-rev-main; then
  pass "raw reference checkout is ignored"
else
  fail "raw reference checkout is not ignored"
fi

unignored_local_state="$(git ls-files --others --exclude-standard | grep -E '(^|/)[.]kiana/(tasks|tmp|logs)/|(^|/)[.]claude/.*[.]local[.]md|post-tool-use-hook-.*post-hook[.]json' || true)"
if [[ -z "$unignored_local_state" ]]; then
  pass "no unignored local runtime state remains"
else
  echo "$unignored_local_state" | head -n 20 >&2
  fail "local runtime state is still visible to git"
fi

workspace_version="$(tr -d '\r\n' < VERSION)"
if grep -Eq "^version = \"${workspace_version}\"$" Cargo.toml; then
  pass "VERSION matches workspace package version"
else
  fail "VERSION does not match workspace package version"
fi

for manifest in kiana-*/Cargo.toml; do
  [[ -f "$manifest" ]] || continue
  require_manifest_field "$manifest" '^license[.]workspace = true$'
  require_manifest_field "$manifest" '^repository[.]workspace = true$'
  require_manifest_field "$manifest" '^rust-version[.]workspace = true$'
  require_manifest_field "$manifest" '^publish = false$'
done

if grep -Fq 'doctor --json' scripts/release-smoke.sh; then
  pass "doctor JSON smoke gate is wired"
else
  fail "release smoke does not exercise doctor --json"
fi

if grep -Fq 'model smoke --json' scripts/release-smoke.sh; then
  pass "model smoke JSON gate is wired"
else
  fail "release smoke does not exercise model smoke --json"
fi

if grep -Fq 'model catalog --json' scripts/release-smoke.sh; then
  pass "model catalog JSON gate is wired"
else
  fail "release smoke does not exercise model catalog --json"
fi

if grep -Fq 'auto-mode critique --model fake' scripts/release-smoke.sh &&
  grep -Fq 'mode: fake provider critique' scripts/release-smoke.sh &&
  grep -Fq 'provider: fake' scripts/release-smoke.sh &&
  grep -Fq 'model_findings:' scripts/release-smoke.sh; then
  pass "auto-mode fake provider critique smoke gate is wired"
else
  fail "release smoke does not exercise auto-mode fake provider critique"
fi

if grep -Fq 'release blockers --json' scripts/release-smoke.sh &&
  grep -Fq 'kiana.commercial-release-blockers.v1' scripts/release-smoke.sh &&
  grep -Fq 'action_plan' scripts/commercial-release-blockers-report.sh &&
  grep -Fq 'kiana.commercial-release-action-plan.v1' scripts/commercial-release-blockers-report.sh &&
  grep -Fq 'import-release-actions' kiana-commands/src/project.rs &&
  grep -Fq 'kiana.project-release-action-import.v1' kiana-commands/src/project.rs &&
  grep -Fq 'release evidence --json' scripts/release-smoke.sh &&
  grep -Fq 'kiana.local-rc-evidence.v1' scripts/release-smoke.sh &&
  grep -Fq 'KIANA_PYTHON_BIN' scripts/release-smoke.sh &&
  grep -Fq 'local_blocking' scripts/release-smoke.sh; then
  pass "release blockers action plan and evidence CLI smoke gates are wired"
else
  fail "release smoke does not exercise release blockers action plan and evidence CLI JSON"
fi

if grep -Fq 'run_installed release blockers --json' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'run_installed release evidence --json' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'run_installed release workflow-proof --json' scripts/package-lifecycle-smoke.sh &&
  grep -Fq -- '--latest-completed' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'KIANA_PYTHON_BIN' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'kiana.commercial-release-blockers.v1' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'kiana.release-workflow-proof.v1' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'kiana.local-rc-evidence.v1' scripts/package-lifecycle-smoke.sh; then
  pass "package lifecycle smoke covers release blockers, workflow proof, and evidence CLI"
else
  fail "package lifecycle smoke does not cover release blockers, workflow proof, and evidence CLI"
fi

if grep -Fq 'tasks workflow advance --json' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'validate --json --workflow' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'tasks workflow complete --json' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'kiana.workflow-transition.v1' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'kiana.workflow-completion.v1' scripts/package-lifecycle-smoke.sh; then
  pass "package lifecycle smoke covers workflow transition and completion"
else
  fail "package lifecycle smoke does not cover workflow transition and completion"
fi


if grep -Fq 'context index --json' scripts/release-smoke.sh &&
  grep -Fq 'context ingest --source' scripts/release-smoke.sh &&
  grep -Fq 'context search release --json --limit 1' scripts/release-smoke.sh &&
  grep -Fq 'context vector-search release flow --json --limit 1' scripts/release-smoke.sh; then
  pass "context index/ingest/search/vector-search JSON gates are wired"
else
  fail "release smoke does not exercise context index/ingest/search/vector-search --json"
fi

if grep -Fq 'license status --json' scripts/release-smoke.sh; then
  pass "license status JSON gate is wired"
else
  fail "release smoke does not exercise license status --json"
fi

if grep -Fq 'product-shell-smoke.sh' scripts/release-smoke.sh; then
  pass "product shell smoke gate is wired"
else
  fail "release smoke does not exercise product shell smoke"
fi

if grep -Fq 'team_runtime_parity_smoke_links_team_tools_resident_loop_and_shutdown' scripts/release-smoke.sh; then
  pass "team runtime parity smoke gate is wired"
else
  fail "release smoke does not exercise team runtime parity smoke"
fi

if grep -Fq 'notebook_execute' scripts/release-smoke.sh; then
  pass "notebook execution isolation smoke gate is wired"
else
  fail "release smoke does not exercise notebook execution isolation smoke"
fi

if grep -Fq 'context.index.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.index.cache.write' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'build_persistent_context_index' kiana-entrypoints/src/cli.rs &&
  grep -Fq '.kiana/context-index.json' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'config.resolved.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/config/resolved' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.config-resolved.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'prompt.history.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/prompt-history' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.prompt-history.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'commands.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'commands.run' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/commands' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/commands/run' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.commands.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.command-run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'permissions.status.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/permissions/status' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.permissions-status.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'trust.status.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/trust/status' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.trust-status.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'auth.status.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/auth/status' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'license.status.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/license/status' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.license-status.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'model.catalog.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/models/catalog' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'model.list.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'model.current.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'model.current.write' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'direct_connect_app_model_current_post_handler' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/models/list' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/models/current' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'POST", "/app/models/current"' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'model.smoke.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/models/smoke' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.repo_map.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/repo-map' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'diff.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/diff' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'checkpoint.create' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/checkpoints' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'checks.dry_run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/checks/dry-run' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'checks.run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/checks' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'review.dry_run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/review/dry-run' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'review.run.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/review' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'release.distribution_review.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/release/distribution' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.artifact_ingest.write' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.search.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.vector_search.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'context.pack.read' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/index' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/ingest' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/search' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/vector-search' kiana-entrypoints/src/cli.rs &&
  grep -Fq '/app/context/pack' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.diff.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.checkpoint.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.checks.dry_run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.checks.run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.review.dry_run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.review.run.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.app-server.distribution-review.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.auth-status.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.model-catalog.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.model-list.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.team-plan.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-index.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-artifact-ingest.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.repo-map.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-search.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-vector-search.v1' kiana-entrypoints/src/cli.rs &&
  grep -Fq 'kiana.context-pack.v1' kiana-entrypoints/src/cli.rs; then
  pass "app-server prompt history, commands read/run, permissions, trust, config resolved, auth status, license status, model catalog/list/current read-write/smoke, release distribution review, checkpoint, diff, repo-map, context, checks dry-run/run, review dry-run/run, and cache endpoints are wired"
else
  fail "app-server prompt history, commands read/run, permissions, trust, config resolved, auth status, license status, model catalog/list/current read-write/smoke, release distribution review, checkpoint, diff, repo-map, context, checks dry-run/run, review dry-run/run, or cache endpoints are not wired"
fi

if grep -Fq 'kiana.plugin-install-receipt.v1' scripts/release-smoke.sh &&
  grep -Fq '.kiana-install-receipt.json' scripts/release-smoke.sh &&
  grep -Fq 'install_receipt_integrity' scripts/release-smoke.sh &&
  grep -Fq '"status": "tampered"' scripts/release-smoke.sh; then
  pass "plugin install receipt integrity smoke gate is wired"
else
  fail "release smoke does not exercise plugin install receipt integrity"
fi

if grep -Fq 'commercial_profile_does_not_let_normal_allow_rules_bypass_mutating_tools' kiana-tools/src/permissions.rs &&
  grep -Fq 'commercial_profile_cannot_bypass_unknown_project_trust' kiana-tools/src/permissions.rs &&
  grep -Fq 'commercial_profile_allows_mutating_tools_from_managed_allow_rules' kiana-tools/src/permissions.rs &&
  grep -Fq '!project_trust.allows_project_resources()' kiana-tools/src/permissions.rs &&
  grep -Fq 'ProjectTrust::Unknown' kiana-tools/src/permissions.rs &&
  grep -Fq 'normal allow rule' kiana-tools/src/permissions.rs &&
  grep -Fq 'managed_allowed_tools' kiana-tools/src/permissions.rs; then
  pass "commercial permission profile requires explicit project trust and managed allow for mutating normal-allow bypasses"
else
  fail "commercial permission profile does not lock mutating normal allow rules behind explicit project trust and managed approval"
fi

if grep -Fq 'smoke_project_trust "$release_bin"' scripts/release-smoke.sh &&
  grep -Fq 'smoke_project_trust "$installed_bin"' scripts/release-smoke.sh &&
  grep -Fq 'smoke_installed_project_trust' scripts/package-lifecycle-smoke.sh &&
  grep -Fq 'project_local_trust_is_not_authoritative' scripts/release-smoke.sh; then
  pass "external fail-closed project trust documentation and installed-binary lifecycle smoke are wired"
else
  fail "project trust release gates do not prove external unknown/trusted/reset lifecycle and legacy self-claim rejection"
fi

if grep -Fq 'ensure_project_scope_allowed' kiana-commands/src/plugin.rs &&
  grep -Fq 'requires trusted project' kiana-commands/src/plugin.rs &&
  grep -Fq 'unknown_and_untrusted_hide_project_and_local_plugin_summaries' kiana-commands/src/plugin.rs &&
  grep -Fq 'unknown_and_untrusted_reject_project_and_local_plugin_commands' kiana-commands/src/plugin.rs &&
  grep -Fq 'load_all_skills_default_uses_external_project_trust' kiana-skills/src/lib.rs &&
  grep -Fq 'installed_plugin_roots_do_not_fall_back_to_project_relative_plugins' kiana-types/src/plugin.rs &&
  grep -Fq 'project trust is unknown' scripts/release-smoke.sh; then
  pass "project/local plugin resources and default skill/plugin loaders are trust-gated"
else
  fail "project/local plugin or default skill/plugin loader trust gates are missing"
fi

if grep -Fq 'smoke_commercial_security_doctor_json "$release_bin"' scripts/release-smoke.sh &&
  grep -Fq 'smoke_commercial_security_doctor_json "$installed_bin"' scripts/release-smoke.sh &&
  grep -Fq 'KIANA_PERMISSION_PROFILE=commercial' scripts/release-smoke.sh &&
  grep -Fq 'commercial doctor JSON failed readiness smoke checks' scripts/release-smoke.sh &&
  grep -Fq 'commercial-security-release-smoke' kiana-commands/src/doctor.rs; then
  pass "commercial security doctor release smoke is wired"
else
  fail "commercial security doctor release smoke is not wired"
fi

if grep -Fq 'auto-mode-critique' kiana-commands/src/doctor.rs &&
  grep -Fq 'fake-provider-auto-mode-critique' kiana-commands/src/doctor.rs; then
  pass "doctor provider registry reports auto-mode fake provider critique evidence"
else
  fail "doctor provider registry is missing auto-mode fake provider critique evidence"
fi

if grep -Fq 'release blockers --json' kiana-commands/src/release.rs &&
  grep -Fq 'release evidence --json' kiana-commands/src/release.rs &&
  grep -Fq 'release workflow-proof' kiana-commands/src/release.rs &&
  grep -Fq -- '--latest-completed' kiana-commands/src/release.rs &&
  grep -Fq '"selection": selection' kiana-commands/src/release.rs &&
  grep -Fq 'kiana.release-workflow-proof.v1' kiana-commands/src/release.rs &&
  grep -Fq 'commercial-release-blockers-report.sh' kiana-commands/src/release.rs &&
  grep -Fq 'KIANA_RELEASE_BLOCKERS_SCRIPT' kiana-commands/src/release.rs &&
  grep -Fq 'KIANA_LOCAL_RC_EVIDENCE_OUT' kiana-commands/src/release.rs &&
  grep -Fq 'latest-local-rc-dir.txt' kiana-commands/src/release.rs &&
  grep -Fq 'kiana.local-rc-evidence.v1' kiana-commands/src/release.rs &&
  grep -Fq 'KIANA_PYTHON_BIN' scripts/commercial-release-blockers-report.sh; then
  pass "release CLI wraps commercial blocker report, workflow proof, and local RC evidence"
else
  fail "release CLI is not wired to commercial blocker report, workflow proof, and local RC evidence"
fi

if grep -Fq 'plugin_install_from_local_marketplace_file_remote_git_source' kiana-commands/src/plugin.rs &&
  grep -Fq 'marketplace_entry_remote_source_path' kiana-commands/src/plugin.rs; then
  pass "local marketplace remote plugin source install is covered"
else
  fail "local marketplace remote plugin source install is not covered"
fi

if grep -Fq 'plugin_install_from_remote_marketplace_pip_file_package_source' kiana-commands/src/plugin.rs &&
  grep -Fq 'resolve_remote_file_package_source' kiana-commands/src/plugin.rs; then
  pass "offline pip file plugin source install is covered"
else
  fail "offline pip file plugin source install is not covered"
fi

if grep -Fq 'KIANA_MANAGED_PLUGIN_POLICY_FILE' kiana-commands/src/plugin.rs &&
  grep -Fq 'managed plugin policy' kiana-commands/src/plugin.rs &&
  grep -Fq 'require_signature' kiana-commands/src/plugin.rs &&
  grep -Fq 'require_signature_verification' kiana-commands/src/plugin.rs &&
  grep -Fq 'verify_marketplace_signature_command' kiana-commands/src/plugin.rs &&
  grep -Fq 'marketplace signature is required' kiana-commands/src/plugin.rs &&
  grep -Fq 'validate_marketplace_signature_content_hash' kiana-commands/src/plugin.rs &&
  grep -Fq 'plugin_install_rejects_signature_content_hash_mismatch_when_managed_policy_requires_signature' kiana-commands/src/plugin.rs &&
  grep -Fq 'plugin_install_runs_managed_signature_verification_command' kiana-commands/src/plugin.rs; then
  pass "managed plugin allow/deny/signature content-hash/signature-verification policy is wired"
else
  fail "managed plugin allow/deny/signature content-hash/signature-verification policy is not wired"
fi

if grep -Fq '"app-server"' scripts/product-acceptance-report.sh &&
  grep -Fq '"context-search"' scripts/product-acceptance-report.sh &&
  grep -Fq '"context-cache-recovery"' scripts/product-acceptance-report.sh &&
  grep -Fq '"context-cache-recovery"' scripts/stage-commercial-release-proofs.sh &&
  grep -Fq '"context-cache-recovery"' scripts/verify-commercial-release-artifacts.sh &&
  grep -Fq 'persistent_context_index_recovers_from_corrupt_cache' scripts/product-acceptance-report.sh &&
  grep -Fq 'cargo test -p kiana-commands --locked --offline context_search' scripts/product-acceptance-report.sh; then
  pass "product acceptance requires app-server, context-search, and context-cache-recovery workflows"
else
  fail "product acceptance does not require app-server, context-search, and context-cache-recovery workflows"
fi

if grep -Fq 'provider-live-smoke.sh --required' scripts/release-preflight.sh &&
  grep -Fq 'remote-live-smoke.sh --required' scripts/release-preflight.sh; then
  pass "live provider and remote smoke gates are documented and wired into full preflight"
else
  fail "live provider and remote smoke gates are not fully documented or wired"
fi





if grep -Fq 'team_plan_report' kiana-commands/src/tasks.rs &&
  grep -Fq '/app/team/plan' kiana-entrypoints/src/cli.rs; then
  pass "team plan role-runtime preflight schema and app endpoint are wired"
else
  fail "team plan role-runtime preflight schema or app endpoint is not wired"
fi

if grep -Fq 'validate_plugin_app_manifest_value' kiana-commands/src/plugin.rs &&
  grep -Fq 'plugin_validate_rejects_invalid_app_manifest_contract' kiana-commands/src/plugin.rs; then
  pass "plugin app manifest contract is schema-pinned and validated"
else
  fail "plugin app manifest contract is not schema-pinned and validated"
fi





















if grep -Fq 'source-control proof accepted' scripts/source-control-proof-report.sh &&
  grep -Fq 'source-control local RC proof written' scripts/source-control-proof-report.sh &&
  grep -Fq 'KIANA_SOURCE_CONTROL_PROOF_FILE' scripts/source-control-proof-report.sh &&
  grep -Fq 'KIANA_SOURCE_CONTROL_PROOF_OUT' scripts/source-control-proof-report.sh; then
  pass "source-control proof report gate is wired"
else
  fail "source-control proof report gate is not wired"
fi


if grep -Fq -- '--handoff-md' scripts/commercial-release-blockers-report.sh; then
  pass "commercial release blocker handoff contract is wired"
else
  fail "commercial release blocker handoff contract is missing"
fi


if grep -Fq 'required_proof_schemas' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'lifecycle_smoke_passed' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'local_rc_ready" if readiness_ready' scripts/local-rc-evidence-report.sh; then
  pass "local RC evidence readiness requires lifecycle smoke and required proof drafts"
else
  fail "local RC evidence readiness is not gated by lifecycle smoke and required proof drafts"
fi

if grep -Fq 'commercial-release-blockers.json' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'commercial-release-handoff.md' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'KIANA_COMMERCIAL_BLOCKERS_OUT' scripts/local-rc-evidence-report.sh &&
  grep -Fq -- '--handoff-md' scripts/local-rc-evidence-report.sh; then
  pass "local RC evidence stages commercial blocker JSON and handoff markdown"
else
  fail "local RC evidence does not stage commercial blocker JSON and handoff markdown"
fi

if grep -Fq 'blocking_ids_by_resolution_scope' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'sha256_file' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'BLOCKER_REPORT_OUT' scripts/local-rc-evidence-report.sh &&
  grep -Fq 'handoff_artifacts' scripts/commercial-release-handoff-smoke.sh; then
  pass "local RC evidence records blocker handoff artifacts and hashes"
else
  fail "local RC evidence does not record blocker handoff artifacts and hashes"
fi

if grep -Fq 'KIANA_DISTRIBUTION_REVIEW_OUT' scripts/distribution-review-report.sh &&
  grep -Fq 'kiana.app-server.distribution-review.v1' scripts/distribution-review-report.sh &&
  grep -Fq 'distribution-review.json' scripts/local-rc-evidence-report.sh; then
  pass "distribution review handoff report is wired into local RC evidence"
else
  fail "distribution review handoff report is not wired into local RC evidence"
fi


blockers_json="$("$bash_bin" scripts/commercial-release-blockers-report.sh --json || true)"
if printf '%s\n' "$blockers_json" | grep -Fq '"schema": "kiana.commercial-release-blockers.v1"'; then
  pass "commercial release blockers report JSON gate is wired"
else
  fail "commercial release blockers report JSON gate is not wired"
fi

if "$bash_bin" scripts/commercial-release-handoff-smoke.sh; then
  pass "commercial release handoff smoke passed"
else
  fail "commercial release handoff smoke failed"
fi

if "$bash_bin" scripts/schema-contract-smoke.sh; then
  pass "schema contract smoke passed"
else
  fail "schema contract smoke failed"
fi


if grep -Fq 'plugin_receipt_payload_hash' kiana-commands/src/plugin.rs &&
  grep -Fq 'install_receipt_integrity' kiana-commands/src/plugin.rs; then
  pass "plugin install receipt integrity is sealed and exposed"
else
  fail "plugin install receipt integrity is not sealed and exposed"
fi

if grep -Fq 'plugin_install_records_marketplace_signature_metadata_in_receipt' kiana-commands/src/plugin.rs &&
  grep -Fq '"signature": {' scripts/release-smoke.sh; then
  pass "plugin install receipt signature metadata is schema-pinned and smoke-covered"
else
  fail "plugin install receipt signature metadata is not schema-pinned and smoke-covered"
fi

if grep -Fq 'KIANA_MANAGED_PLUGIN_POLICY_FILE="$managed_policy_file"' scripts/release-smoke.sh; then
  pass "managed plugin policy JSON schema, signature metadata, and verification smoke are pinned"
else
  fail "managed plugin policy JSON schema, signature metadata, or verification smoke coverage is missing"
fi








if grep -Fq 'explicit_project_trust:required' scripts/platform-security-proof-report.sh &&
  grep -Fq 'doctor_report_sha256' scripts/stage-commercial-release-proofs.sh &&
  grep -Fq 'doctor_report_sha256' scripts/verify-commercial-release-artifacts.sh; then
  pass "platform security accepted-state schema contract is pinned"
else
  fail "platform security accepted-state schema contract is not pinned"
fi



if grep -Fq 'KIANA_SIGNATURE_VERIFY_COMMAND' scripts/sign-release-artifacts.sh &&
  grep -Fq 'KIANA_SIGNATURE_VERIFY_COMMAND' scripts/verify-commercial-release-artifacts.sh; then
  pass "release signature verification command and digest binding are enforced"
else
  fail "release signature verification command or digest binding is not enforced"
fi





if grep -Fq 'KIANA_RELEASE_BASE_URL=https://github.com/${GITHUB_REPOSITORY}/releases/download/${release_tag}' .github/workflows/release.yml; then
  pass "release workflow derives package manifest URLs from the active repository and tag"
else
  fail "release workflow does not derive KIANA_RELEASE_BASE_URL from the active repository and tag"
fi

if grep -Fq 'release-artifacts/**' .github/workflows/release.yml; then
  pass "GitHub release upload includes recursive artifacts"
else
  fail "GitHub release upload does not include recursive manifest/compliance artifacts"
fi

package_lifecycle_line="$(grep -n 'scripts/package-lifecycle-smoke.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
sign_artifacts_line="$(grep -n 'scripts/sign-release-artifacts.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
if [[ -n "$package_lifecycle_line" && -n "$sign_artifacts_line" && "$package_lifecycle_line" -lt "$sign_artifacts_line" ]]; then
  pass "release workflow validates package lifecycle before signing"
else
  fail "release workflow does not run package lifecycle smoke before signing"
fi

if grep -Fq 'dist/proofs/**' .github/workflows/release.yml &&
  grep -Fq 'KIANA_SOURCE_CONTROL_PROOF_OUT: dist/proofs/source-control/source-control.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_LIVE_SMOKE_DIR: dist/proofs/live-smoke' .github/workflows/release.yml &&
  grep -Fq 'KIANA_ENTITLEMENT_PROOF_OUT: dist/proofs/entitlement/entitlement-proof.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_PRODUCT_ACCEPTANCE_OUT: dist/proofs/product/product-acceptance.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_RELEASE_OPS_OUT: dist/proofs/release-ops/release-ops.json' .github/workflows/release.yml &&
  grep -Fq 'KIANA_PLATFORM_SECURITY_PROOF_OUT: dist/proofs/platform-security/platform-security-${{ runner.os }}.json' .github/workflows/release.yml; then
  pass "release workflow preserves source-control, live, entitlement, product, ops, and platform proof artifacts"
else
  fail "release workflow does not preserve source-control/live/entitlement/product/ops/platform proof artifacts"
fi

stage_proofs_line="$(grep -n 'stage-commercial-release-proofs.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
verify_artifacts_line="$(grep -n 'verify-commercial-release-artifacts.sh' .github/workflows/release.yml | head -n 1 | cut -d: -f1 || true)"
if [[ -n "$stage_proofs_line" && -n "$verify_artifacts_line" && "$stage_proofs_line" -lt "$verify_artifacts_line" ]]; then
  pass "commercial proof staging runs before artifact verification"
else
  fail "release workflow does not stage commercial proof manifest before artifact verification"
fi

if grep -Fq 'verify-commercial-release-artifacts.sh' .github/workflows/release.yml; then
  pass "GitHub release draft is gated by commercial artifact verification"
else
  fail "GitHub release workflow does not verify commercial artifacts before draft release"
fi

if grep -Fq 'sign-release-artifacts.sh' .github/workflows/release.yml &&
  grep -Fq 'KIANA_SIGNING_COMMAND' .github/workflows/release.yml &&
  grep -Fq 'KIANA_SIGNATURE_VERIFY_COMMAND' .github/workflows/release.yml; then
  pass "release workflow has explicit artifact signing and signature verification steps"
else
  fail "release workflow does not run explicit artifact signing and signature verification"
fi

if grep -Fq 'KIANA_RELEASE_SIGNER is required' scripts/sign-release-artifacts.sh &&
  grep -Fq 'external-release-signer' scripts/sign-release-artifacts.sh; then
  pass "release signing requires a reviewed signer identity before proof generation"
else
  fail "release signing does not require a reviewed signer identity before proof generation"
fi

if grep -Fq 'dist/*.signature.json' .github/workflows/release.yml &&
  grep -Fq 'dist/*.notarization.json' .github/workflows/release.yml &&
  grep -Fq 'dist/*.sig' .github/workflows/release.yml; then
  pass "release workflow preserves signing and notarization proof artifacts"
else
  fail "release workflow does not upload signing/notarization proof artifacts"
fi

if grep -Fq 'KIANA_RELEASE_SIGNING_CONFIRMED' .github/workflows/release.yml; then
  fail "release workflow still accepts manual signing confirmation instead of artifact proof"
else
  pass "release workflow does not accept manual signing confirmation"
fi

if [[ "$mode" == "full" ]]; then
  expected_tag="v${workspace_version}"
  if git rev-parse --verify HEAD >/dev/null 2>&1; then
    pass "git HEAD exists"
  else
    fail "git HEAD does not exist; create an initial reviewed commit before release"
  fi

  if git remote get-url origin >/dev/null 2>&1 || [[ -n "$(git remote)" ]]; then
    pass "git remote is configured"
  else
    fail "no git remote configured"
  fi

  if [[ -z "$(git status --porcelain --untracked-files=no)" ]]; then
    pass "tracked worktree is clean"
  else
    fail "tracked worktree has uncommitted changes"
  fi

  if git tag --points-at HEAD | grep -Fxq "$expected_tag"; then
    pass "HEAD is tagged with ${expected_tag}"
  else
    fail "HEAD is not tagged with ${expected_tag}"
  fi

  if [[ -z "${KIANA_RELEASE_TAG:-}" || "${KIANA_RELEASE_TAG}" == "$expected_tag" ]]; then
    pass "release tag matches VERSION"
  else
    fail "KIANA_RELEASE_TAG=${KIANA_RELEASE_TAG} does not match expected ${expected_tag}"
  fi

  if "$bash_bin" scripts/provider-live-smoke.sh --required; then
    pass "provider live smoke passed"
  else
    fail "provider live smoke failed"
  fi

  if "$bash_bin" scripts/remote-live-smoke.sh --required; then
    pass "remote live smoke passed"
  else
    fail "remote live smoke failed"
  fi

  if "$bash_bin" scripts/product-acceptance-report.sh full; then
    pass "product acceptance gate passed"
  else
    fail "product acceptance gate failed"
  fi

  if "$bash_bin" scripts/entitlement-proof-report.sh full; then
    pass "entitlement proof gate passed"
  else
    fail "entitlement proof gate failed"
  fi

  if "$bash_bin" scripts/release-ops-report.sh full; then
    pass "release ops gate passed"
  else
    fail "release ops gate failed"
  fi

  if "$bash_bin" scripts/platform-security-proof-report.sh full; then
    pass "platform security proof gate passed"
  else
    fail "platform security proof gate failed"
  fi

  pass "full release signing/channel proof is enforced by release artifact verification"
else
  if KIANA_PRODUCT_ACCEPTANCE_ALLOW_LOCAL_RC_GATE_SKIP="${KIANA_PRODUCT_ACCEPTANCE_ALLOW_LOCAL_RC_GATE_SKIP:-1}" "$bash_bin" scripts/product-acceptance-report.sh --local-rc; then
    pass "local RC product acceptance report generated"
  else
    fail "local RC product acceptance report failed"
  fi

  if "$bash_bin" scripts/entitlement-proof-report.sh --local-rc; then
    pass "local RC entitlement proof report generated"
  else
    fail "local RC entitlement proof report failed"
  fi

  if "$bash_bin" scripts/release-ops-report.sh --local-rc; then
    pass "local RC release ops report generated"
  else
    fail "local RC release ops report failed"
  fi

  if "$bash_bin" scripts/platform-security-proof-report.sh --local-rc; then
    pass "local RC platform security proof report generated"
  else
    fail "local RC platform security proof report failed"
  fi

  pass "local RC mode: git remote, HEAD, clean tracked tree, and signing checks skipped"
fi

if "$bash_bin" scripts/release-signature-verification-smoke.sh; then
  pass "commercial release verifier smoke passed"
else
  fail "commercial release verifier smoke failed"
fi

if (( failures > 0 )); then
  echo "release preflight failed with ${failures} issue(s)" >&2
  exit 1
fi

echo "release preflight passed"
