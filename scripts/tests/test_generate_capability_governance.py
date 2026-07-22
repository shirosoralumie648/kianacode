from __future__ import annotations

import importlib.util
import json
import shutil
import sys
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import capability_governance as governance


CLI = SCRIPTS / "generate-capability-governance.py"
SPEC = importlib.util.spec_from_file_location(
    "kiana_generate_capability_governance_tests",
    CLI,
)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("capability governance generator module is unavailable")
generator = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(generator)

GOVERNANCE_ROOT = ROOT / "docs/agent-program/kiana-completion/governance"


def read_json(path: Path) -> dict[str, object]:
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise AssertionError(f"expected JSON object at {path}")
    return value


def write_json(path: Path, value: dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )


class CapabilityGovernanceGeneratorTests(unittest.TestCase):
    def test_diff_requires_the_current_selected_family_ancestry(self) -> None:
        selected = GOVERNANCE_ROOT / "repository-registry/references-2026-07-22.json"
        ancestor = GOVERNANCE_ROOT / "repository-registry/references-2026-07-15.json"
        unrelated = GOVERNANCE_ROOT / "capability-decisions/review-2026-07-18.json"

        generator.validate_diff_family_authority("repository-registry", selected, ROOT)
        generator.validate_diff_family_authority("repository-registry", ancestor, ROOT)
        with self.assertRaisesRegex(
            governance.GovernanceUsageError,
            "to revision is not in the selected family ancestry",
        ):
            generator.validate_diff_family_authority("repository-registry", unrelated, ROOT)

    def test_diff_structurally_validates_intermediate_predecessors(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "scripts").mkdir()
            shutil.copy2(SCRIPTS / "validate-json-schema.py", root / "scripts")
            shutil.copytree(ROOT / "docs/schemas", root / "docs/schemas")

            registry_root = (
                root
                / "docs/agent-program/kiana-completion/governance/repository-registry"
            )
            genesis_path = registry_root / "references-2026-07-15-genesis.json"
            predecessor_path = registry_root / "references-2026-07-15.json"
            head_path = registry_root / "references-2026-07-22.json"

            genesis = read_json(
                GOVERNANCE_ROOT
                / "repository-registry/references-2026-07-15-genesis.json"
            )
            predecessor = read_json(
                GOVERNANCE_ROOT / "repository-registry/references-2026-07-15.json"
            )
            head = read_json(
                GOVERNANCE_ROOT / "repository-registry/references-2026-07-22.json"
            )
            predecessor.pop("repositories")
            head["previous_revision_sha256"] = governance.canonical_sha256(predecessor)

            write_json(genesis_path, genesis)
            write_json(predecessor_path, predecessor)
            write_json(head_path, head)

            with self.assertRaisesRegex(
                governance.GovernanceUsageError,
                "schema_validation_failed",
            ):
                generator.validated_diff_document(
                    "repository-registry",
                    genesis_path,
                    head_path,
                    root,
                )


if __name__ == "__main__":
    unittest.main()
