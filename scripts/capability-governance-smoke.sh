#!/usr/bin/bash -p
set -euo pipefail

if ((BASH_VERSINFO[0] < 5)); then
  echo "capability governance smoke requires Bash 5 or newer" >&2
  exit 1
fi

ROOT="$(cd -- "${BASH_SOURCE[0]%/*}/.." && pwd -P)"
cd "$ROOT"

usage() {
  echo "usage: scripts/capability-governance-smoke.sh [{schemas|fixture-shapes|public-baseline|reference-governance|semantic-negative|drift-refresh|legacy-authority|generated-views|production}]" >&2
}

public_slices=(
  schemas
  fixture-shapes
  public-baseline
  reference-governance
  semantic-negative
  drift-refresh
  legacy-authority
  generated-views
  production
)

if (($# == 2)) && [[ "${1:-}" == "--internal-worker" ]]; then
  slice="$2"
  case "$slice" in
    schemas | fixture-shapes | public-baseline | reference-governance | semantic-negative | drift-refresh | legacy-authority | generated-views | production) ;;
    *) echo "supervisor_worker_invalid: unknown slice" >&2; exit 1 ;;
  esac
  worker_mode=1
elif (($# <= 1)); then
  supervisor_bin="$ROOT/target/debug/kiana-capability-governance-supervisor"
  if [[ ! -x "$supervisor_bin" ]]; then
    echo "supervisor_unavailable: build kiana-capability-governance-supervisor with --locked --offline" >&2
    exit 1
  fi
  unset BASH_ENV ENV PYTHONPATH PYTHONHOME PYTHONSTARTUP LD_PRELOAD LD_LIBRARY_PATH
  export PATH="/usr/bin:/bin"
  export LC_ALL="C"
  if (($# == 1)); then
    slice="$1"
    case "$slice" in
      schemas | fixture-shapes | public-baseline | reference-governance | semantic-negative | drift-refresh | legacy-authority | generated-views | production) ;;
      *) usage; exit 2 ;;
    esac
    exec "$supervisor_bin" "$slice"
  fi
  aggregate_tmp="$(mktemp -d)"
  aggregate_pids=()
  cleanup_aggregate() {
    local cleanup_status=$?
    local pid

    trap - EXIT INT TERM
    for pid in "${aggregate_pids[@]}"; do
      if kill -0 "$pid" 2>/dev/null; then
        kill -TERM "$pid" 2>/dev/null || true
      fi
      wait "$pid" 2>/dev/null || true
    done
    rm -rf "$aggregate_tmp"
    return "$cleanup_status"
  }
  trap cleanup_aggregate EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  for index in "${!public_slices[@]}"; do
    slice="${public_slices[$index]}"
    "$supervisor_bin" "$slice" \
      >"$aggregate_tmp/$index.stdout" \
      2>"$aggregate_tmp/$index.stderr" &
    aggregate_pids+=("$!")
  done
  aggregate_status=0
  for index in "${!aggregate_pids[@]}"; do
    if ! wait "${aggregate_pids[$index]}"; then
      aggregate_status=1
    fi
  done
  for index in "${!public_slices[@]}"; do
    cat "$aggregate_tmp/$index.stdout"
    cat "$aggregate_tmp/$index.stderr" >&2
  done
  trap - EXIT INT TERM
  rm -rf "$aggregate_tmp"
  aggregate_tmp=""
  exit "$aggregate_status"
else
  usage
  exit 2
fi

if [[ -z "${KIANA_GOVERNANCE_PYTHON:-}" ||
  "${KIANA_GOVERNANCE_PYTHON}" != /* ||
  ! -x "${KIANA_GOVERNANCE_PYTHON}" ]]; then
  echo "supervisor_worker_unavailable: trusted Python is missing" >&2
  exit 1
fi
python="$KIANA_GOVERNANCE_PYTHON"

run_python() {
  "$python" -I "$@"
}

unset PYTHONPATH PYTHONHOME PYTHONSTARTUP
export PYTHONNOUSERSITE=1
if [[ -n "${KIANA_GOVERNANCE_LAUNCH_FD:-}" ||
  -n "${KIANA_GOVERNANCE_COMPLETION_FD:-}" ]]; then
  echo "supervisor_worker_invalid: authority descriptors reached semantic worker" >&2
  exit 1
fi

if ! run_python - <<'PY'
import errno
import socket

try:
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
except OSError as exc:
    if exc.errno == errno.EPERM:
        raise SystemExit(0)
    raise SystemExit(2)
probe.close()
raise SystemExit(1)
PY
then
  echo "supervisor_worker_invalid: startup socket canary failed" >&2
  exit 1
fi

fixtures_dir="scripts/fixtures/capability-governance/valid"
minimal_fixture="$fixtures_dir/minimal-graph.json"
full_fixture="$fixtures_dir/full-38-repositories.json"
hostile_fixture="$fixtures_dir/hostile-rendering.json"
offline_fixture="$fixtures_dir/offline-source-identity.json"
refresh_request="$fixtures_dir/refresh-request.json"
refresh_result="$fixtures_dir/refresh-result.json"
protected_inputs=(
  "$minimal_fixture"
  "$full_fixture"
  "$hostile_fixture"
  "$offline_fixture"
  "$refresh_request"
  "$refresh_result"
  scripts/validate-json-schema.py
  docs/schemas/kiana-official-source-artifact.v1.schema.json
  docs/schemas/kiana-public-parity-baseline.v1.schema.json
  docs/schemas/kiana-reference-repository-registry.v1.schema.json
  docs/schemas/kiana-capability-decisions.v1.schema.json
  docs/schemas/kiana-capability-evidence-index.v1.schema.json
  docs/schemas/kiana-capability-governance-diff.v1.schema.json
  docs/schemas/kiana-legacy-authority-classification.v1.schema.json
  docs/schemas/kiana-capability-governance-bundle.v1.schema.json
)

tmp_dir="$(mktemp -d)"

cleanup_worker() {
  local cleanup_status=$?

  trap - EXIT INT TERM
  rm -rf "$tmp_dir"
  return "$cleanup_status"
}

trap cleanup_worker EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

run_schema_slice() {
  local schemas=(
    docs/schemas/kiana-official-source-artifact.v1.schema.json
    docs/schemas/kiana-public-parity-baseline.v1.schema.json
    docs/schemas/kiana-reference-repository-registry.v1.schema.json
    docs/schemas/kiana-capability-decisions.v1.schema.json
    docs/schemas/kiana-capability-evidence-index.v1.schema.json
    docs/schemas/kiana-capability-governance-diff.v1.schema.json
    docs/schemas/kiana-legacy-authority-classification.v1.schema.json
    docs/schemas/kiana-capability-governance-bundle.v1.schema.json
  )
  local schema fixture manifest schema_path instance_path
  local fixtures=()

  if ((${#schemas[@]} != 8)); then
    echo "schema_contract_count: expected 8 schemas" >&2
    return 1
  fi
  if ! run_python - <<'PY'
try:
    from jsonschema import Draft202012Validator, FormatChecker
except Exception:
    raise SystemExit(1)
PY
  then
    echo "schema_validator_unavailable: Python jsonschema is required" >&2
    return 1
  fi
  for schema in "${schemas[@]}"; do
    run_python -m json.tool "$schema" >/dev/null
  done

  fixtures=(
    "$minimal_fixture"
    "$full_fixture"
    "$hostile_fixture"
    "$offline_fixture"
  )

  for fixture in "${fixtures[@]}"; do
    manifest="$tmp_dir/$(basename "${fixture%.json}").instances.tsv"
    run_python - "$fixture" "$tmp_dir" >"$manifest" <<'PY'
import hashlib
import json
import pathlib
import sys

fixture_path = pathlib.Path(sys.argv[1])
output_dir = pathlib.Path(sys.argv[2]) / fixture_path.stem
output_dir.mkdir(parents=True, exist_ok=True)
bundle = json.loads(fixture_path.read_text(encoding="utf-8"))

schema_for = {
    "official_source_artifact": "docs/schemas/kiana-official-source-artifact.v1.schema.json",
    "public_baseline_revisions": "docs/schemas/kiana-public-parity-baseline.v1.schema.json",
    "repository_registry_revisions": "docs/schemas/kiana-reference-repository-registry.v1.schema.json",
    "capability_decision_revisions": "docs/schemas/kiana-capability-decisions.v1.schema.json",
    "evidence_revisions": "docs/schemas/kiana-capability-evidence-index.v1.schema.json",
    "legacy_authority_revisions": "docs/schemas/kiana-legacy-authority-classification.v1.schema.json",
    "bundle_manifest": "docs/schemas/kiana-capability-governance-bundle.v1.schema.json",
}


def canonical_sha256(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def emit(schema_path: str, name: str, value: object) -> None:
    target = output_dir / f"{name}.json"
    target.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(f"{schema_path}\t{target}")


for key, schema_path in schema_for.items():
    value = bundle[key]
    if key.endswith("_revisions"):
        if not isinstance(value, list):
            raise SystemExit(f"fixture_shape: {key} must be an array")
        for index, revision in enumerate(value):
            emit(schema_path, f"{key}-{index}", revision)
    else:
        emit(schema_path, key, value)

for family, key in (
    ("public-baseline", "public_baseline_revisions"),
    ("repository-registry", "repository_registry_revisions"),
):
    revisions = bundle[key]
    if len(revisions) < 2:
        continue
    previous = revisions[-2]
    current = revisions[-1]
    diff = {
        "schema": "kiana.capability-governance-diff.v1",
        "version": "1.0",
        "family": family,
        "from_revision_id": previous["revision_id"],
        "from_revision_sha256": canonical_sha256(previous),
        "to_revision_id": current["revision_id"],
        "to_revision_sha256": canonical_sha256(current),
        "added_ids": [],
        "removed_ids": [],
        "changed_ids": [],
    }
    emit(
        "docs/schemas/kiana-capability-governance-diff.v1.schema.json",
        f"{family}-diff",
        diff,
    )
PY

    while IFS=$'\t' read -r schema_path instance_path; do
      run_python scripts/validate-json-schema.py "$schema_path" "$instance_path" >/dev/null
    done <"$manifest"

    run_python - "$manifest" <<'PY'
import json
import pathlib
import sys

try:
    from jsonschema import Draft202012Validator, FormatChecker
    from jsonschema.exceptions import SchemaError
except Exception:
    raise SystemExit("schema_validator_unavailable: Python jsonschema is required")


def json_path(parts: list[object]) -> str:
    rendered = "$"
    for part in parts:
        if isinstance(part, int):
            rendered += f"[{part}]"
        else:
            rendered += f".{part}"
    return rendered


manifest_path = pathlib.Path(sys.argv[1])
validators: dict[pathlib.Path, Draft202012Validator] = {}
for line in manifest_path.read_text(encoding="utf-8").splitlines():
    schema_name, instance_name = line.split("\t", 1)
    schema_path = pathlib.Path(schema_name)
    instance_path = pathlib.Path(instance_name)
    if schema_path not in validators:
        schema = json.loads(schema_path.read_text(encoding="utf-8"))
        try:
            Draft202012Validator.check_schema(schema)
        except SchemaError as exc:
            keyword = exc.validator or "schema"
            raise SystemExit(
                f"schema_contract_invalid: keyword={keyword} schema={schema_path.name}"
            )
        validators[schema_path] = Draft202012Validator(
            schema,
            format_checker=FormatChecker(),
        )
    instance = json.loads(instance_path.read_text(encoding="utf-8"))
    errors = sorted(
        validators[schema_path].iter_errors(instance),
        key=lambda error: (
            tuple(str(part) for part in error.absolute_path),
            str(error.validator),
        ),
    )
    if errors:
        error = errors[0]
        keyword = error.validator or "unknown"
        raise SystemExit(
            "schema_validation_failed: "
            f"keyword={keyword} schema={schema_path.name} "
            f"instance={instance_path.name} path={json_path(list(error.absolute_path))}"
        )
PY
  done
}

run_fixture_shape_slice() {
  if [[ ! -f "$minimal_fixture" ]]; then
    echo "fixture_required: $minimal_fixture" >&2
    return 1
  fi

  run_python - "$minimal_fixture" <<'PY'
import hashlib
import json
import pathlib
import sys

fixture_path = pathlib.Path(sys.argv[1])
bundle = json.loads(fixture_path.read_text(encoding="utf-8"))
expected_bundle_keys = {
    "official_source_artifact",
    "public_baseline_revisions",
    "repository_registry_revisions",
    "capability_decision_revisions",
    "evidence_revisions",
    "legacy_authority_revisions",
    "bundle_manifest",
}
revision_families = {
    "public_baseline_revisions": "public_baseline",
    "repository_registry_revisions": "repository_registry",
    "capability_decision_revisions": "capability_decisions",
    "evidence_revisions": "evidence_head",
    "legacy_authority_revisions": "legacy_authority",
}
predecessor_keys = {
    "previous_revision_id",
    "previous_revision_path",
    "previous_revision_sha256",
}
alias_keys = {
    "kind",
    "value",
    "change_kind",
    "reason",
    "effective_at",
    "evidence_ids",
    "review_revision",
}


def fail(code: str, detail: str) -> None:
    raise SystemExit(f"{code}: {detail}")


def canonical_sha256(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def require_unique(values: list[str], label: str) -> None:
    if len(values) != len(set(values)):
        fail("duplicate_id", label)


def require_relative(path: str, label: str) -> None:
    parts = pathlib.PurePosixPath(path).parts
    if pathlib.PurePosixPath(path).is_absolute() or ".." in parts:
        fail("unsafe_path", label)


if set(bundle) != expected_bundle_keys:
    fail("bundle_keys", f"expected {sorted(expected_bundle_keys)}, got {sorted(bundle)}")

artifact = bundle["official_source_artifact"]
entry_ids = [entry["source_entry_id"] for entry in artifact["entries"]]
require_unique(entry_ids, "official source entry IDs")

heads: dict[str, dict] = {}
for family_key, binding_key in revision_families.items():
    revisions = bundle[family_key]
    if len(revisions) < 2:
        fail("ancestry_required", family_key)
    revision_ids = [revision["revision_id"] for revision in revisions]
    require_unique(revision_ids, f"{family_key} revision IDs")
    by_id: dict[str, dict] = {}
    for revision in revisions:
        revision_id = revision["revision_id"]
        kind = revision["revision_kind"]
        present_predecessor = predecessor_keys.intersection(revision)
        if kind == "genesis":
            if "genesis_reason" not in revision or present_predecessor:
                fail("genesis_ancestry", revision_id)
        elif kind == "successor":
            if "genesis_reason" in revision or present_predecessor != predecessor_keys:
                fail("successor_ancestry", revision_id)
            previous_id = revision["previous_revision_id"]
            if previous_id not in by_id:
                fail("previous_revision_id", revision_id)
            if revision["previous_revision_sha256"] != canonical_sha256(by_id[previous_id]):
                fail("previous_revision_sha256", revision_id)
            require_relative(revision["previous_revision_path"], revision_id)
        else:
            fail("revision_kind", revision_id)
        by_id[revision_id] = revision
    heads[binding_key] = revisions[-1]

for revision in bundle["public_baseline_revisions"]:
    source_binding = revision["source_artifact"]
    if source_binding["artifact_id"] != artifact["artifact_id"]:
        fail("source_artifact_id", revision["revision_id"])
    if source_binding["sha256"] != canonical_sha256(artifact):
        fail("source_artifact_sha256", revision["revision_id"])
    if source_binding["content_sha256"] != artifact["content_sha256"]:
        fail("source_content_sha256", revision["revision_id"])
    require_relative(source_binding["path"], revision["revision_id"])
    journey_ids = [row["journey_id"] for row in revision["journeys"]]
    capability_ids = [row["capability_id"] for row in revision["capabilities"]]
    require_unique(journey_ids, f"{revision['revision_id']} journey IDs")
    require_unique(capability_ids, f"{revision['revision_id']} capability IDs")
    mappings = revision["official_source_index"]
    mapping_ids = [row["source_entry_id"] for row in mappings]
    require_unique(mapping_ids, f"{revision['revision_id']} source mappings")
    if set(mapping_ids) != set(entry_ids):
        fail("source_mapping_coverage", revision["revision_id"])
    for mapping in mappings:
        if ("capability_ids" in mapping) == ("exclusion" in mapping):
            fail("source_mapping_choice", mapping["source_entry_id"])

has_git = False
has_content = False
for revision in bundle["repository_registry_revisions"]:
    repositories = revision["repositories"]
    repo_ids = [row["repo_id"] for row in repositories]
    repo_paths = [row["path"] for row in repositories]
    require_unique(repo_ids, f"{revision['revision_id']} repository IDs")
    require_unique(repo_paths, f"{revision['revision_id']} repository paths")
    for row in repositories:
        require_relative(row["path"], row["repo_id"])
        for alias in row["aliases"]:
            if set(alias) != alias_keys:
                fail("alias_shape", row["repo_id"])
        has_git = has_git or row["revision_kind"] == "git_commit"
        has_content = has_content or row["revision_kind"] == "content_tree_sha256"
if not (has_git and has_content):
    fail("revision_kind_coverage", "expected Git and content-only repositories")

for revision in bundle["capability_decision_revisions"]:
    decision_ids = [row["decision_id"] for row in revision["decisions"]]
    require_unique(decision_ids, f"{revision['revision_id']} decision IDs")

for revision in bundle["evidence_revisions"]:
    evidence_ids = [row["evidence_id"] for row in revision["records"]]
    require_unique(evidence_ids, f"{revision['revision_id']} evidence IDs")
    for row in revision["records"]:
        for axis in ("coverage_state", "proof_level", "freshness"):
            if axis not in row:
                fail("proof_axis", f"{row['evidence_id']} missing {axis}")

for revision in bundle["legacy_authority_revisions"]:
    paths = [row["path"] for row in revision["entries"]]
    require_unique(paths, f"{revision['revision_id']} legacy paths")
    for path in paths:
        require_relative(path, revision["revision_id"])

manifest = bundle["bundle_manifest"]
expected_manifest_keys = {
    "schema",
    "version",
    "official_source_artifact",
    "public_baseline",
    "repository_registry",
    "capability_decisions",
    "evidence_head",
    "legacy_authority",
    "evaluation_time",
}
if set(manifest) != expected_manifest_keys:
    fail("manifest_keys", "bundle manifest contains copied state")

artifact_binding = manifest["official_source_artifact"]
if set(artifact_binding) != {"artifact_id", "path", "sha256"}:
    fail("artifact_binding_shape", "official_source_artifact")
if artifact_binding["artifact_id"] != artifact["artifact_id"]:
    fail("artifact_binding_id", artifact_binding["artifact_id"])
if artifact_binding["sha256"] != canonical_sha256(artifact):
    fail("artifact_binding_sha256", artifact_binding["artifact_id"])
require_relative(artifact_binding["path"], artifact_binding["artifact_id"])

for binding_key, head in heads.items():
    binding = manifest[binding_key]
    if set(binding) != {"revision_id", "path", "sha256"}:
        fail("head_binding_shape", binding_key)
    if binding["revision_id"] != head["revision_id"]:
        fail("head_binding_id", binding_key)
    if binding["sha256"] != canonical_sha256(head):
        fail("head_binding_sha256", binding_key)
    require_relative(binding["path"], binding_key)

print("OK: minimal governance fixture shapes and hashes are valid")
PY

  if [[ ! -f "$full_fixture" ]]; then
    echo "fixture_required: $full_fixture" >&2
    return 1
  fi

  run_python - "$full_fixture" docs/agent-program/kiana-completion/references.json <<'PY'
import hashlib
import json
import pathlib
import re
import sys

fixture_path = pathlib.Path(sys.argv[1])
seed_path = pathlib.Path(sys.argv[2])
bundle = json.loads(fixture_path.read_text(encoding="utf-8"))
expected_bundle_keys = {
    "official_source_artifact",
    "public_baseline_revisions",
    "repository_registry_revisions",
    "capability_decision_revisions",
    "evidence_revisions",
    "legacy_authority_revisions",
    "bundle_manifest",
}
alias_keys = {
    "kind",
    "value",
    "change_kind",
    "reason",
    "effective_at",
    "evidence_ids",
    "review_revision",
}
prohibited_proof_keys = {
    "complete",
    "completion",
    "completion_status",
    "coverage_state",
    "proof_level",
    "required_proof_level",
    "product_complete",
}


def fail(code: str, detail: str) -> None:
    raise SystemExit(f"{code}: {detail}")


try:
    seed = json.loads(seed_path.read_text(encoding="utf-8"))
except FileNotFoundError:
    fail("seed_required", seed_path.as_posix())
except (OSError, UnicodeError, json.JSONDecodeError):
    fail("seed_invalid", seed_path.as_posix())


def canonical_sha256(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def walk_keys(value: object):
    if isinstance(value, dict):
        yield from value
        for child in value.values():
            yield from walk_keys(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk_keys(child)


if set(bundle) != expected_bundle_keys:
    fail("bundle_keys", fixture_path.as_posix())

registries = bundle["repository_registry_revisions"]
decision_revisions = bundle["capability_decision_revisions"]
if not registries or not decision_revisions:
    fail("full_fixture_heads", "registry and decision revisions are required")
registry = registries[-1]
decisions = decision_revisions[-1]
repositories = registry["repositories"]
if registry["expected_count"] != 38 or len(repositories) != 38:
    fail("repository_count", str(len(repositories)))

repo_ids = [row["repo_id"] for row in repositories]
repo_paths = [row["path"] for row in repositories]
if len(set(repo_ids)) != 38 or len(set(repo_paths)) != 38:
    fail("repository_identity", "IDs and paths must each be unique")
if any(re.fullmatch(r"[a-z0-9][a-z0-9._-]*", repo_id) is None for repo_id in repo_ids):
    fail("repository_id_normalization", "stable IDs must be normalized")

if (
    not isinstance(seed, dict)
    or set(seed) != {"schema", "expected_count", "references"}
    or seed.get("schema") != "kiana.agent-reference-catalog.v1"
    or seed.get("expected_count") != 38
    or not isinstance(seed.get("references"), list)
    or len(seed["references"]) != 38
):
    fail("seed_invalid", "expected closed 38-reference catalog")
seed_references = seed["references"]
if any(
    not isinstance(row, dict)
    or set(row) != {"id", "path", "domains"}
    or not isinstance(row["id"], str)
    or not isinstance(row["path"], str)
    or not isinstance(row["domains"], list)
    or not row["domains"]
    or any(not isinstance(domain, str) for domain in row["domains"])
    for row in seed_references
):
    fail("seed_invalid", "reference rows require id, path, and domains")
seed_ids = [row["id"] for row in seed_references]
seed_paths = [row["path"] for row in seed_references]
if len(set(seed_ids)) != 38 or len(set(seed_paths)) != 38:
    fail("seed_invalid", "reference IDs and paths must be unique")

seed_rows = {row["path"]: row for row in seed_references}
if set(repo_paths) != set(seed_rows):
    fail(
        "seed_reconciliation",
        f"missing={sorted(set(seed_rows) - set(repo_paths))} extra={sorted(set(repo_paths) - set(seed_rows))}",
    )

content_paths = {
    "reference/claude-code-main (2)",
    "reference/claude-code-rev-main",
}
for row in repositories:
    raw = seed_rows[row["path"]]
    aliases = row["aliases"]
    if any(set(alias) != alias_keys for alias in aliases):
        fail("alias_shape", row["repo_id"])
    alias_values = {(alias["kind"], alias["value"]) for alias in aliases}
    if ("repo_id", raw["id"]) not in alias_values or ("path", raw["path"]) not in alias_values:
        fail("legacy_alias_missing", row["repo_id"])
    if row["domains"] != raw["domains"]:
        fail("imported_domains", row["repo_id"])
    expected_kind = "content_tree_sha256" if row["path"] in content_paths else "git_commit"
    if row["revision_kind"] != expected_kind:
        fail("repository_revision_kind", row["repo_id"])
    if expected_kind == "git_commit" and "git_head" not in row:
        fail("git_head_required", row["repo_id"])
    if expected_kind == "content_tree_sha256" and "git_head" in row:
        fail("content_git_head_forbidden", row["repo_id"])

inventory_pairs = {
    (row["repo_id"], capability_id)
    for row in repositories
    for capability_id in row["inventory_capability_ids"]
}
current_rows = [row for row in decisions["decisions"] if row["freshness"] == "current"]
decision_pairs = [(row["repo_id"], row["capability_id"]) for row in current_rows]
if len(decision_pairs) != len(set(decision_pairs)):
    fail("duplicate_current_decision", "repository/capability pairs must be unique")
if set(decision_pairs) != inventory_pairs:
    fail("decision_coverage", "every inventory capability needs one current decision")
if {row["decision"] for row in current_rows} != {"adopt", "adapt", "reject"}:
    fail("decision_kinds", "adopt, adapt, and reject must all be sampled")

if prohibited_proof_keys.intersection(walk_keys(registry)):
    fail("registry_proof_state", "registry governance must not assert product proof")
if prohibited_proof_keys.intersection(walk_keys(decisions)):
    fail("decision_proof_state", "decisions must not assert product proof")

manifest = bundle["bundle_manifest"]
for binding_key, head in (
    ("repository_registry", registry),
    ("capability_decisions", decisions),
):
    binding = manifest[binding_key]
    if binding["revision_id"] != head["revision_id"]:
        fail("full_head_id", binding_key)
    if binding["sha256"] != canonical_sha256(head):
        fail("full_head_sha256", binding_key)

print("OK: full 38-reference fixture matches seed, optional live paths, aliases, and decisions")
PY

  for required_fixture in "$hostile_fixture" "$offline_fixture"; do
    if [[ ! -f "$required_fixture" ]]; then
      echo "fixture_required: $required_fixture" >&2
      return 1
    fi
  done

  run_python - "$hostile_fixture" "$offline_fixture" <<'PY'
import base64
import binascii
import hashlib
import json
import pathlib
import re
import sys

hostile_path = pathlib.Path(sys.argv[1])
offline_path = pathlib.Path(sys.argv[2])
hostile = json.loads(hostile_path.read_text(encoding="utf-8"))
offline = json.loads(offline_path.read_text(encoding="utf-8"))
expected_bundle_keys = {
    "official_source_artifact",
    "public_baseline_revisions",
    "repository_registry_revisions",
    "capability_decision_revisions",
    "evidence_revisions",
    "legacy_authority_revisions",
    "bundle_manifest",
}
family_bindings = {
    "public_baseline_revisions": "public_baseline",
    "repository_registry_revisions": "repository_registry",
    "capability_decision_revisions": "capability_decisions",
    "evidence_revisions": "evidence_head",
    "legacy_authority_revisions": "legacy_authority",
}


def fail(code: str, detail: str) -> None:
    raise SystemExit(f"{code}: {detail}")


def canonical_sha256(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def strings(value: object):
    if isinstance(value, str):
        yield value
    elif isinstance(value, dict):
        for child in value.values():
            yield from strings(child)
    elif isinstance(value, list):
        for child in value:
            yield from strings(child)


def keys(value: object):
    if isinstance(value, dict):
        yield from value
        for child in value.values():
            yield from keys(child)
    elif isinstance(value, list):
        for child in value:
            yield from keys(child)


def validate_manifest(bundle: dict, label: str) -> None:
    if set(bundle) != expected_bundle_keys:
        fail("bundle_keys", label)
    manifest = bundle["bundle_manifest"]
    artifact = bundle["official_source_artifact"]
    artifact_binding = manifest["official_source_artifact"]
    if artifact_binding["artifact_id"] != artifact["artifact_id"]:
        fail("artifact_binding_id", label)
    if artifact_binding["sha256"] != canonical_sha256(artifact):
        fail("artifact_binding_sha256", label)
    for family_key, binding_key in family_bindings.items():
        revisions = bundle[family_key]
        if not revisions:
            fail("fixture_family_required", f"{label}:{family_key}")
        head = revisions[-1]
        binding = manifest[binding_key]
        if set(binding) != {"revision_id", "path", "sha256"}:
            fail("head_binding_shape", f"{label}:{binding_key}")
        if binding["revision_id"] != head["revision_id"]:
            fail("head_binding_id", f"{label}:{binding_key}")
        if binding["sha256"] != canonical_sha256(head):
            fail("head_binding_sha256", f"{label}:{binding_key}")


validate_manifest(hostile, "hostile")
validate_manifest(offline, "offline")

hostile_text = "\n".join(strings(hostile))
for marker in ("**", "[", "]", "<", ">", "\n", "|", "`"):
    if marker not in hostile_text:
        fail("hostile_marker_missing", repr(marker))
for pattern in (
    r"(?i)<\s*(?:script|iframe|object|embed)\b",
    r"(?i)javascript\s*:",
    r"(?i)on[a-z]+\s*=",
    r"(?i)AKIA[0-9A-Z]{16}",
    r"(?i)BEGIN [A-Z ]*PRIVATE KEY",
    r"(?i)Bearer\s+[A-Za-z0-9._-]+",
    r"(?i)\bsk-[A-Za-z0-9]{12,}",
    r"(?:^|[\s\"'])/(?:home|tmp|var/tmp|Users)/",
    r"[A-Za-z]:\\",
):
    if re.search(pattern, hostile_text):
        fail("hostile_forbidden_text", pattern)
for uri_key in ("official_uri", "archive_uri"):
    if not hostile["official_source_artifact"][uri_key].startswith("https://"):
        fail("hostile_uri_scheme", uri_key)

artifact = offline["official_source_artifact"]
if artifact["normalization"]["method"] != "inline-base64-entry-lf-join":
    fail("offline_normalization_method", artifact["normalization"]["method"])
if not artifact["archive_uri"].startswith("https://"):
    fail("offline_archive_uri", artifact["archive_uri"])

entry_ids = []
normalized_entries = []
for entry in artifact["entries"]:
    entry_ids.append(entry["source_entry_id"])
    prefix = "inline-base64:"
    locator = entry["locator"]
    if not locator.startswith(prefix):
        fail("offline_locator", entry["source_entry_id"])
    try:
        normalized = base64.b64decode(locator[len(prefix):], validate=True)
        normalized.decode("utf-8")
    except (binascii.Error, UnicodeDecodeError) as exc:
        fail("offline_normalized_bytes", f"{entry['source_entry_id']}:{exc}")
    if hashlib.sha256(normalized).hexdigest() != entry["entry_sha256"]:
        fail("offline_entry_sha256", entry["source_entry_id"])
    normalized_entries.append(normalized)

if len(entry_ids) != len(set(entry_ids)):
    fail("offline_entry_ids", "source entry IDs must be unique")
normalized_artifact = b"\n".join(normalized_entries) + b"\n"
if hashlib.sha256(normalized_artifact).hexdigest() != artifact["content_sha256"]:
    fail("offline_content_sha256", artifact["artifact_id"])

public = offline["public_baseline_revisions"][-1]
source_binding = public["source_artifact"]
if source_binding["artifact_id"] != artifact["artifact_id"]:
    fail("offline_public_artifact_id", public["revision_id"])
if source_binding["sha256"] != canonical_sha256(artifact):
    fail("offline_public_artifact_sha256", public["revision_id"])
if source_binding["content_sha256"] != artifact["content_sha256"]:
    fail("offline_public_content_sha256", public["revision_id"])

mappings = public["official_source_index"]
mapping_ids = [row["source_entry_id"] for row in mappings]
if len(mapping_ids) != len(set(mapping_ids)) or set(mapping_ids) != set(entry_ids):
    fail("offline_mapping_coverage", public["revision_id"])
for row in mappings:
    if ("capability_ids" in row) == ("exclusion" in row):
        fail("offline_mapping_choice", row["source_entry_id"])
    if "exclusion" in row:
        exclusion = row["exclusion"]
        if not exclusion["rationale"] or not exclusion["evidence_ids"] or not exclusion["review_revision"]:
            fail("offline_exclusion_review", row["source_entry_id"])

if {
    "complete",
    "completion",
    "completion_status",
    "coverage_state",
    "proof_level",
}.intersection(keys(offline["bundle_manifest"])):
    fail("bundle_completion_authority", "selector contains proof or completion state")

print("OK: hostile rendering and offline source identity fixtures are safe and reproducible")
PY

  run_python - "$minimal_fixture" "$full_fixture" "$hostile_fixture" "$offline_fixture" <<'PY'
import hashlib
import json
import pathlib
import sys


class EvidenceContractError(Exception):
    pass


def fail(code: str, detail: str) -> None:
    raise EvidenceContractError(f"{code}: {detail}")


def canonical_sha256(value: object) -> str:
    payload = json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")
    return hashlib.sha256(payload).hexdigest()


def record_sha256(record: dict) -> str:
    return canonical_sha256({key: value for key, value in record.items() if key != "record_sha256"})


def add_expected(
    expected: dict[str, tuple[str, str, str, str, str, str]],
    evidence_id: str,
    binding: tuple[str, str, str, str, str, str],
    owner: str,
) -> None:
    previous = expected.get(evidence_id)
    if previous is not None and previous != binding:
        fail("evidence_binding_ambiguity", f"{evidence_id} is shared by {owner}")
    expected[evidence_id] = binding


def collect_references(value: object) -> set[str]:
    references: set[str] = set()
    if isinstance(value, dict):
        for key, child in value.items():
            if key.endswith("evidence_ids"):
                if not isinstance(child, list):
                    fail("evidence_reference_shape", key)
                references.update(child)
            references.update(collect_references(child))
    elif isinstance(value, list):
        for child in value:
            references.update(collect_references(child))
    return references


def expected_bindings(bundle: dict) -> dict[str, tuple[str, str, str, str, str, str]]:
    expected: dict[str, tuple[str, str, str, str, str, str]] = {}
    artifact_revision = bundle["official_source_artifact"]["content_sha256"]

    for revision in bundle["public_baseline_revisions"]:
        revision_id = revision["revision_id"]
        for capability in revision["capabilities"]:
            binding = (
                "capability",
                capability["capability_id"],
                revision_id,
                artifact_revision,
                revision_id,
                "local_contract",
            )
            for evidence_id in capability["evidence_ids"]:
                add_expected(expected, evidence_id, binding, capability["capability_id"])
        for mapping in revision["official_source_index"]:
            exclusion = mapping.get("exclusion")
            if exclusion is None:
                continue
            binding = (
                "public_baseline",
                revision["snapshot_id"],
                revision_id,
                artifact_revision,
                revision_id,
                "source_review",
            )
            for evidence_id in exclusion["evidence_ids"]:
                add_expected(expected, evidence_id, binding, mapping["source_entry_id"])

    for revision in bundle["repository_registry_revisions"]:
        revision_id = revision["revision_id"]
        for repository in revision["repositories"]:
            binding = (
                "repository",
                repository["repo_id"],
                revision_id,
                repository["revision_value"],
                revision_id,
                "source_review",
            )
            for evidence_id in repository["license_evidence_ids"]:
                add_expected(expected, evidence_id, binding, repository["repo_id"])
            for alias in repository["aliases"]:
                for evidence_id in alias["evidence_ids"]:
                    add_expected(expected, evidence_id, binding, repository["repo_id"])

    for revision in bundle["capability_decision_revisions"]:
        revision_id = revision["revision_id"]
        for decision in revision["decisions"]:
            binding = (
                "capability_decision",
                decision["decision_id"],
                revision_id,
                decision["source_revision_value"],
                decision["target_revision_value"],
                "source_review",
            )
            for evidence_id in decision["evidence_ids"]:
                add_expected(expected, evidence_id, binding, decision["decision_id"])
            for evidence_id in decision["security_review"]["evidence_ids"]:
                add_expected(expected, evidence_id, binding, decision["decision_id"])

    for revision in bundle["legacy_authority_revisions"]:
        revision_id = revision["revision_id"]
        for entry in revision["entries"]:
            binding = (
                "legacy_authority",
                revision_id,
                revision_id,
                entry["content_sha256"],
                revision_id,
                "source_review",
            )
            for evidence_id in entry["evidence_ids"]:
                add_expected(expected, evidence_id, binding, entry["path"])

    return expected


def validate_evidence(bundle: dict, label: str) -> None:
    revisions = bundle["evidence_revisions"]
    previous_records: list[dict] | None = None
    for revision in revisions:
        records = revision["records"]
        if previous_records is not None:
            if len(records) <= len(previous_records):
                fail("history_removal", revision["revision_id"])
            if records[: len(previous_records)] != previous_records:
                fail("history_rewrite", revision["revision_id"])
        previous_records = records

    head_records = revisions[-1]["records"]
    evidence_ids = [record["evidence_id"] for record in head_records]
    if len(evidence_ids) != len(set(evidence_ids)):
        fail("duplicate_evidence_id", label)

    references = collect_references(
        {key: value for key, value in bundle.items() if key != "bundle_manifest"}
    )
    missing = sorted(references - set(evidence_ids))
    if missing:
        fail("unknown_reference", f"{label}:{','.join(missing)}")

    if [record["sequence"] for record in head_records] != list(range(1, len(head_records) + 1)):
        fail("evidence_sequence", label)

    previous_hash = "genesis"
    for record in head_records:
        if record["previous_record_sha256"] != previous_hash:
            fail("record_chain", record["evidence_id"])
        if record["record_sha256"] != record_sha256(record):
            fail("record_sha256", record["evidence_id"])
        previous_hash = record["record_sha256"]

    records_by_id = {record["evidence_id"]: record for record in head_records}
    for evidence_id, binding in sorted(expected_bindings(bundle).items()):
        record = records_by_id[evidence_id]
        actual = (
            record["subject_family"],
            record["subject_id"],
            record["subject_revision_id"],
            record["source_binding"]["revision_value"],
            record["target_binding"]["revision_value"],
            record["environment"]["environment_kind"],
        )
        if actual != binding:
            fail("evidence_binding", evidence_id)


errors: list[str] = []
for fixture_name in sys.argv[1:]:
    fixture_path = pathlib.Path(fixture_name)
    try:
        validate_evidence(
            json.loads(fixture_path.read_text(encoding="utf-8")),
            fixture_path.name,
        )
    except EvidenceContractError as exc:
        errors.append(str(exc))

if errors:
    raise SystemExit("\n".join(errors))
print("OK: evidence histories, references, hashes, and bindings are closed")
PY
}

protected_hashes() {
  run_python - "${protected_inputs[@]}" <<'PY'
import hashlib
import pathlib
import sys

for name in sys.argv[1:]:
    path = pathlib.Path(name)
    try:
        payload = path.read_bytes()
    except OSError as exc:
        raise SystemExit(f"protected_input_unavailable: {path.as_posix()}:{exc}")
    print(f"{path.as_posix()}\t{hashlib.sha256(payload).hexdigest()}")
PY
}

validate_positive_fixture() {
  local fixture="$1"
  local output="$tmp_dir/$(basename "${fixture%.json}").validation.json"

  run_python scripts/validate-capability-governance.py \
    validate --fixture-bundle "$fixture" --json >"$output"
  run_python - "$output" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
report = json.loads(path.read_text(encoding="utf-8"))
if (
    report.get("schema") != "kiana.capability-governance-validation.v1"
    or report.get("status") != "valid"
    or report.get("errors") != []
):
    raise SystemExit("positive_validation_failed: production validator did not return valid")
PY
}

run_public_baseline_slice() {
  local before after
  before="$(protected_hashes)"

  validate_positive_fixture "$offline_fixture"
  validate_positive_fixture "$hostile_fixture"

  after="$(protected_hashes)"
  if [[ "$before" != "$after" ]]; then
    echo "protected_input_modified: public-baseline validation changed a protected input" >&2
    return 1
  fi
  echo "OK: production public-baseline semantics accept offline-source and hostile-rendering fixtures"
}

run_reference_governance_slice() {
  local before after
  before="$(protected_hashes)"

  validate_positive_fixture "$full_fixture"

  after="$(protected_hashes)"
  if [[ "$before" != "$after" ]]; then
    echo "protected_input_modified: reference-governance validation changed a protected input" >&2
    return 1
  fi
  echo "OK: production reference-governance semantics accept the 38-repository fixture"
}

run_semantic_negative_slice() {
  local before after
  before="$(protected_hashes)"

  run_python scripts/run-capability-governance-corpus.py --temp-root "$tmp_dir"

  after="$(protected_hashes)"
  if [[ "$before" != "$after" ]]; then
    echo "protected_input_modified: semantic-negative execution changed a protected input" >&2
    return 1
  fi
}

run_drift_refresh_slice() {
  local before after output_root
  before="$(protected_hashes)"
  output_root="$tmp_dir/refresh-output"

  run_python - "$refresh_request" "$tmp_dir" <<'PY'
import json
import importlib.util
import pathlib
import sys

request = pathlib.Path(sys.argv[1])
output = pathlib.Path(sys.argv[2])
module_path = pathlib.Path("scripts/freeze-capability-governance.py")
spec = importlib.util.spec_from_file_location("kiana_freeze_capability_governance", module_path)
if spec is None or spec.loader is None:
    raise SystemExit("drift_runner_unavailable: cannot load lifecycle module")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
cases = [
    "no_drift",
    "repository_head_drift",
    "repository_tree_drift",
    "license_hash_drift",
    "content_tree_drift",
    "official_source_hash_drift",
    "target_revision_drift",
    "source_unavailable",
]
expected = {
    "no_drift": (0, "current", []),
    **{case: (1, "stale", [case]) for case in cases[1:-1]},
    "source_unavailable": (1, "unavailable", ["source_unavailable"]),
}

for case in cases:
    path = output / f"drift-{case}.json"
    exit_code = module._check_drift(
        module.argparse.Namespace(
            fixture_bundle=request,
            case=case,
            output=path,
            manifest=None,
            live_reference_root=None,
            target_root=None,
            official_source_artifact=None,
        )
    )
    report = json.loads(path.read_text(encoding="utf-8"))
    expected_exit, expected_status, expected_codes = expected[case]
    actual_codes = [error["code"] for error in report.get("errors", [])]
    if (
        exit_code != expected_exit
        or report.get("status") != expected_status
        or actual_codes != expected_codes
        or (
            case == "source_unavailable"
            and (report.get("current") is not False or report.get("freshness") == "current")
        )
    ):
        raise SystemExit(
            f"drift_case_failed: {case}:{exit_code}:{report}"
        )
PY

  run_python scripts/freeze-capability-governance.py refresh \
    --fixture-bundle "$minimal_fixture" \
    --drift-report "$refresh_request" \
    --output-root "$output_root" \
    --revision-id fixture-refresh \
    --review-revision fixture-review
  diff -u "$refresh_result" "$output_root/refresh-result.json"
  run_python scripts/validate-capability-governance.py \
    validate --manifest "$output_root/current.json" --json >/dev/null

  after="$(protected_hashes)"
  if [[ "$before" != "$after" ]]; then
    echo "protected_input_modified: drift-refresh execution changed a protected input" >&2
    return 1
  fi
  echo "OK: drift-refresh exact cases and immutable successor contract pass"
}

run_legacy_authority_slice() {
  local governance_root="docs/agent-program/kiana-completion/governance"
  local genesis="$governance_root/legacy-authority/legacy-authority-2026-07-15-genesis.json"
  local head="$governance_root/legacy-authority/legacy-authority-2026-07-15.json"
  local current="$governance_root/current.json"

  run_python scripts/validate-json-schema.py docs/schemas/kiana-legacy-authority-classification.v1.schema.json "$genesis" >/dev/null
  run_python scripts/validate-json-schema.py docs/schemas/kiana-legacy-authority-classification.v1.schema.json "$head" >/dev/null
  run_python scripts/validate-json-schema.py docs/schemas/kiana-capability-governance-bundle.v1.schema.json "$current" >/dev/null
  run_python scripts/validate-capability-governance.py validate-history --head "$head" --json >/dev/null
  run_python - "$head" "$current" <<'PY'
import hashlib, json, pathlib, sys
head = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
current = json.loads(pathlib.Path(sys.argv[2]).read_text(encoding="utf-8"))
expected = {p.as_posix() for p in pathlib.Path("docs/reference_audit").glob("*.md")}
expected |= {"docs/reference-feature-matrix.md", "docs/reference-migration-roadmap.md", "docs/commercial-release-readiness.md", "docs/agent-program/kiana-completion/references.json"}
rows = {entry["path"]: entry for entry in head["entries"]}
if set(rows) != expected or len(rows) != len(head["entries"]): raise SystemExit("legacy_inventory_mismatch")
for path, entry in rows.items():
    if hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest() != entry["content_sha256"]: raise SystemExit(f"legacy_hash_drift:{path}")
    if not entry["rationale"] or not entry["replacement_view"]: raise SystemExit(f"legacy_classification_incomplete:{path}")
for key in ("official_source_artifact", "public_baseline", "repository_registry", "capability_decisions", "evidence_head", "legacy_authority"):
    binding = current[key]
    value = json.loads(pathlib.Path(binding["path"]).read_text(encoding="utf-8"))
    payload = json.dumps(value, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode("utf-8")
    if hashlib.sha256(payload).hexdigest() != binding["sha256"]: raise SystemExit(f"current_head_hash:{key}")
PY
  echo "OK: legacy authority inventory, ancestry, and explicit current heads match"
}

exercise_governance_diff() {
  local family="$1"
  local from_path="$2"
  local to_path="$3"
  local output_path="$4"
  local label="$5"
  local first_path="$tmp_dir/$label-first.json"
  local tampered_path="$tmp_dir/$label-tampered.json"

  run_python scripts/generate-capability-governance.py diff \
    --family "$family" --from "$from_path" --to "$to_path" --output "$output_path"
  cp "$output_path" "$first_path"
  run_python scripts/generate-capability-governance.py diff \
    --family "$family" --from "$from_path" --to "$to_path" --output "$output_path" --check
  run_python scripts/validate-json-schema.py \
    docs/schemas/kiana-capability-governance-diff.v1.schema.json "$output_path" >/dev/null
  run_python - "$output_path" "$from_path" "$to_path" <<'PY'
import hashlib
import json
import pathlib
import sys

output, before, after = map(pathlib.Path, sys.argv[1:])
document = json.loads(output.read_text(encoding="utf-8"))
before_value = json.loads(before.read_text(encoding="utf-8"))
after_value = json.loads(after.read_text(encoding="utf-8"))
expected = {
    "from_revision_id": before_value["revision_id"],
    "from_revision_sha256": hashlib.sha256(before.read_bytes()).hexdigest(),
    "to_revision_id": after_value["revision_id"],
    "to_revision_sha256": hashlib.sha256(after.read_bytes()).hexdigest(),
}
if any(document.get(key) != value for key, value in expected.items()):
    raise SystemExit("diff_binding_mismatch")
PY
  printf ' ' >>"$output_path"
  cp "$output_path" "$tampered_path"
  if run_python scripts/generate-capability-governance.py diff \
    --family "$family" --from "$from_path" --to "$to_path" --output "$output_path" --check \
    2>"$tmp_dir/$label-tamper.err"; then
    echo "diff_tamper_accepted: $family" >&2
    return 1
  fi
  cmp "$tampered_path" "$output_path"
  cp "$first_path" "$output_path"
}

run_generated_views_slice() {
  local before after
  local -a selected_paths
  local governance_root="docs/agent-program/kiana-completion/governance"
  local manifest="$governance_root/current.json"
  local output_root="$tmp_dir/generated"
  local compat_root="$tmp_dir/compat"
  local repository_root="$tmp_dir/repository"
  local hostile_root="$tmp_dir/hostile"
  local public_from="$governance_root/public-baselines/cc-public-2026-07-15-genesis.json"
  local registry_from="$governance_root/repository-registry/references-2026-07-15-genesis.json"
  local public_to registry_to

  run_python - "$manifest" >"$tmp_dir/generated-selected-paths.txt" <<'PY'
import json
import pathlib
import sys

manifest = json.loads(pathlib.Path(sys.argv[1]).read_text(encoding="utf-8"))
print(manifest["public_baseline"]["path"])
print(manifest["repository_registry"]["path"])
PY
  mapfile -t selected_paths <"$tmp_dir/generated-selected-paths.txt"
  if ((${#selected_paths[@]} != 2)); then
    echo "selected_head_paths_invalid: generated-views" >&2
    return 1
  fi
  public_to="${selected_paths[0]}"
  registry_to="${selected_paths[1]}"

  before="$(protected_hashes)"
  run_python scripts/generate-capability-governance.py render \
    --manifest "$manifest" --output-root "$output_root"
  run_python scripts/generate-capability-governance.py render \
    --manifest "$manifest" --output-root "$output_root" --check

  mkdir -p "$hostile_root"
  run_python - "$manifest" "$hostile_root/attempt.json" "$hostile_root/escape.txt" <<'PY'
import json
import pathlib
import sys

source, target, escape = map(pathlib.Path, sys.argv[1:])
value = json.loads(source.read_text(encoding="utf-8"))
value["repository_output_path"] = escape.as_posix()
target.write_text(json.dumps(value, sort_keys=True) + "\n", encoding="utf-8")
PY
  if run_python scripts/generate-capability-governance.py render \
    --manifest "$hostile_root/attempt.json" --output-root "$hostile_root/out" \
    2>"$tmp_dir/render-escape.err"; then
    echo "render_escape_accepted" >&2
    return 1
  fi
  run_python - "$hostile_root" <<'PY'
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
files = {path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()}
if files != {"attempt.json"} or (root / "escape.txt").exists():
    raise SystemExit(f"render_escape_write: {sorted(files)}")
PY

  run_python scripts/generate-capability-governance.py render-compat \
    --manifest "$manifest" --output-root "$compat_root" \
    --repository-output-root "$repository_root" \
    --compat-manifest "$governance_root/compat-outputs.json"
  run_python - "$compat_root" "$repository_root" "$tmp_dir/compat-before.json" <<'PY'
import hashlib
import json
import pathlib
import sys

roots = [pathlib.Path(value) for value in sys.argv[1:3]]
output = pathlib.Path(sys.argv[3])
rows = [
    [root.name, path.relative_to(root).as_posix(), hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_mtime_ns]
    for root in roots
    for path in sorted(root.rglob("*"))
    if path.is_file()
]
output.write_text(json.dumps(rows, sort_keys=True) + "\n", encoding="utf-8")
PY
  run_python scripts/generate-capability-governance.py render-compat \
    --manifest "$manifest" --output-root "$compat_root" \
    --repository-output-root "$repository_root" \
    --compat-manifest "$governance_root/compat-outputs.json" --check
  run_python - "$compat_root" "$repository_root" "$tmp_dir/compat-after.json" <<'PY'
import hashlib
import json
import pathlib
import sys

roots = [pathlib.Path(value) for value in sys.argv[1:3]]
output = pathlib.Path(sys.argv[3])
rows = [
    [root.name, path.relative_to(root).as_posix(), hashlib.sha256(path.read_bytes()).hexdigest(), path.stat().st_mtime_ns]
    for root in roots
    for path in sorted(root.rglob("*"))
    if path.is_file()
]
output.write_text(json.dumps(rows, sort_keys=True) + "\n", encoding="utf-8")
PY
  cmp "$tmp_dir/compat-before.json" "$tmp_dir/compat-after.json"
  run_python - "$repository_root" <<'PY'
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
files = {path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file()}
expected = {"docs/reference_audit/CAPABILITY-GOVERNANCE.generated.txt"}
if files != expected:
    raise SystemExit(f"compat_output_mismatch: {sorted(files)}")
PY

  exercise_governance_diff \
    public-baseline "$public_from" "$public_to" "$tmp_dir/public-diff.json" public
  exercise_governance_diff \
    repository-registry "$registry_from" "$registry_to" "$tmp_dir/registry-diff.json" registry
  cmp "$tmp_dir/public-diff.json" \
    "$governance_root/diffs/public-baseline/cc-public-2026-07-15.genesis-to-current.json"
  cmp "$tmp_dir/registry-diff.json" \
    "$governance_root/diffs/repository-registry/references-2026-07-15.genesis-to-current.json"
  diff -ru "$governance_root/generated" "$output_root"

  after="$(protected_hashes)"
  if [[ "$before" != "$after" ]]; then
    echo "protected_input_modified: generated-views changed a protected input" >&2
    return 1
  fi
  echo "OK: generated views, confinement, exact diffs, and tamper rejection pass"
}

run_production_slice() {
  local governance_root="docs/agent-program/kiana-completion/governance"
  local manifest="$governance_root/current.json"
  local started="$SECONDS"
  local drift_report="$tmp_dir/production-drift.json"
  local production_report="$tmp_dir/production-report.json"

  run_python scripts/generate-capability-governance.py verify-production \
    --manifest "$manifest" \
    --generated-root "$governance_root/generated" \
    --repository-output-root . \
    --compat-manifest "$governance_root/compat-outputs.json" \
    --public-from "$governance_root/public-baselines/cc-public-2026-07-15-genesis.json" \
    --public-to "$governance_root/public-baselines/cc-public-2026-07-15.json" \
    --public-diff "$governance_root/diffs/public-baseline/cc-public-2026-07-15.genesis-to-current.json" \
    --registry-from "$governance_root/repository-registry/references-2026-07-15-genesis.json" \
    --registry-to "$governance_root/repository-registry/references-2026-07-22.json" \
    --registry-diff "$governance_root/diffs/repository-registry/references-2026-07-15.genesis-to-current.json" \
    --report "$production_report" \
    --drift-report "$drift_report"
  run_python - "$production_report" "$manifest" <<'PY'
import json
import pathlib
import sys

root = pathlib.Path.cwd().resolve()
scripts = (root / "scripts").resolve()
module_path = scripts / "capability_governance.py"
if not module_path.is_file():
    raise SystemExit("production_report_validator_unavailable")
sys.path.insert(0, str(scripts))
import capability_governance as governance

if pathlib.Path(governance.__file__).resolve() != module_path:
    raise SystemExit("production_report_validator_untrusted")
report_path, manifest_path = map(pathlib.Path, sys.argv[1:])
report = json.loads(report_path.read_text(encoding="utf-8"))
manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
if not isinstance(manifest, dict) or not isinstance(manifest.get("evaluation_time"), str):
    raise SystemExit("production_manifest_invalid")
governance.validate_production_report(
    report,
    manifest_sha256=governance.canonical_sha256(manifest),
    evaluation_time=manifest["evaluation_time"],
)
PY
  if ((SECONDS - started >= 30)); then
    echo "production_deadline_exceeded" >&2
    return 1
  fi
  echo "OK: production governance report, drift, evidence, ancestry, and generated bytes pass"
}

case "$slice" in
  schemas) run_schema_slice ;;
  fixture-shapes) run_fixture_shape_slice ;;
  public-baseline) run_public_baseline_slice ;;
  reference-governance) run_reference_governance_slice ;;
  semantic-negative) run_semantic_negative_slice ;;
  drift-refresh) run_drift_refresh_slice ;;
  legacy-authority) run_legacy_authority_slice ;;
  generated-views) run_generated_views_slice ;;
  production) run_production_slice ;;
esac

# The Rust worker launcher owns the completion receipt. The semantic worker can
# only report its result through this reserved exit code.
trap - EXIT INT TERM
rm -rf "$tmp_dir"
tmp_dir=""
exit 80
