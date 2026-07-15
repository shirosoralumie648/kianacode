#!/usr/bin/env bash
set -euo pipefail

if ((BASH_VERSINFO[0] < 5)); then
  echo "capability governance smoke requires Bash 5 or newer" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"

python_bin() {
  command -v python3 2>/dev/null || command -v python 2>/dev/null || {
    echo "capability governance smoke requires python3 or python" >&2
    exit 1
  }
}

usage() {
  echo "usage: scripts/capability-governance-smoke.sh {schemas|fixture-shapes}" >&2
}

if (($# != 1)); then
  usage
  exit 2
fi

slice="$1"
case "$slice" in
  schemas | fixture-shapes) ;;
  *)
    usage
    exit 2
    ;;
esac

python="$(python_bin)"
fixtures_dir="scripts/fixtures/capability-governance/valid"
minimal_fixture="$fixtures_dir/minimal-graph.json"
tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

# Every subprocess remains offline even if a later edit accidentally adds an
# HTTP-aware command. Current slices use only local Bash and Python operations.
export HTTP_PROXY="http://127.0.0.1:9"
export HTTPS_PROXY="http://127.0.0.1:9"
export ALL_PROXY="http://127.0.0.1:9"
export http_proxy="$HTTP_PROXY"
export https_proxy="$HTTPS_PROXY"
export all_proxy="$ALL_PROXY"
export NO_PROXY=""
export no_proxy=""
export PYTHONNOUSERSITE=1

start_ns="$("$python" -c 'import time; print(time.monotonic_ns())')"

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
  for schema in "${schemas[@]}"; do
    "$python" -m json.tool "$schema" >/dev/null
  done

  shopt -s nullglob
  fixtures=("$fixtures_dir"/*.json)
  shopt -u nullglob
  if ((${#fixtures[@]} == 0)); then
    echo "fixture_required: $minimal_fixture" >&2
    return 1
  fi

  for fixture in "${fixtures[@]}"; do
    manifest="$tmp_dir/$(basename "${fixture%.json}").instances.tsv"
    "$python" - "$fixture" "$tmp_dir" >"$manifest" <<'PY'
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
      "$python" scripts/validate-json-schema.py "$schema_path" "$instance_path" >/dev/null
    done <"$manifest"
  done
}

run_fixture_shape_slice() {
  if [[ ! -f "$minimal_fixture" ]]; then
    echo "fixture_required: $minimal_fixture" >&2
    return 1
  fi

  "$python" - "$minimal_fixture" <<'PY'
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
}

case "$slice" in
  schemas) run_schema_slice ;;
  fixture-shapes) run_fixture_shape_slice ;;
esac

end_ns="$("$python" -c 'import time; print(time.monotonic_ns())')"
elapsed_seconds="$("$python" - "$start_ns" "$end_ns" <<'PY'
import sys

elapsed = (int(sys.argv[2]) - int(sys.argv[1])) / 1_000_000_000
if elapsed >= 30:
    raise SystemExit(f"slice exceeded 30 seconds: {elapsed:.3f}")
print(f"{elapsed:.3f}")
PY
)"
printf 'OK: slice=%s elapsed_seconds=%s offline=true\n' "$slice" "$elapsed_seconds"
