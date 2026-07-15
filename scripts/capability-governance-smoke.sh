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
full_fixture="$fixtures_dir/full-38-repositories.json"
hostile_fixture="$fixtures_dir/hostile-rendering.json"
offline_fixture="$fixtures_dir/offline-source-identity.json"
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

  if [[ ! -f "$full_fixture" ]]; then
    echo "fixture_required: $full_fixture" >&2
    return 1
  fi

  "$python" - "$full_fixture" docs/agent-program/kiana-completion/references.json reference <<'PY'
import hashlib
import json
import pathlib
import re
import sys

fixture_path = pathlib.Path(sys.argv[1])
seed_path = pathlib.Path(sys.argv[2])
reference_root = pathlib.Path(sys.argv[3])
bundle = json.loads(fixture_path.read_text(encoding="utf-8"))
seed = json.loads(seed_path.read_text(encoding="utf-8"))
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

live_paths = {
    f"reference/{path.name}"
    for path in reference_root.iterdir()
    if path.is_dir()
}
if set(repo_paths) != live_paths:
    fail(
        "repository_path_reconciliation",
        f"missing={sorted(live_paths - set(repo_paths))} extra={sorted(set(repo_paths) - live_paths)}",
    )

seed_rows = {row["path"]: row for row in seed["references"]}
if seed.get("expected_count") != 38 or set(seed_rows) != live_paths:
    fail("seed_reconciliation", "checked-in seed must describe the live 38 paths")

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

print("OK: full 38-reference fixture matches seed, live paths, aliases, and decisions")
PY

  for required_fixture in "$hostile_fixture" "$offline_fixture"; do
    if [[ ! -f "$required_fixture" ]]; then
      echo "fixture_required: $required_fixture" >&2
      return 1
    fi
  done

  "$python" - "$hostile_fixture" "$offline_fixture" <<'PY'
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
