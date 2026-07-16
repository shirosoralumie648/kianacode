from __future__ import annotations

import copy
import json
import sys
import unittest
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[2]
SCRIPTS = ROOT / "scripts"
if str(SCRIPTS) not in sys.path:
    sys.path.insert(0, str(SCRIPTS))

import capability_governance as governance


VALID_FIXTURE = (
    ROOT
    / "scripts"
    / "fixtures"
    / "capability-governance"
    / "valid"
    / "minimal-graph.json"
)


def load_minimal_bundle() -> dict[str, Any]:
    return json.loads(VALID_FIXTURE.read_text(encoding="utf-8"))


def error_codes(errors: list[governance.GovernanceError]) -> set[str]:
    return {error.code for error in errors}


def refresh_head_binding(bundle: dict[str, Any], family: str) -> None:
    binding_key = governance.REVISION_FAMILIES[family]
    head = bundle[family][-1]
    bundle["bundle_manifest"][binding_key]["sha256"] = governance.canonical_sha256(head)


def refresh_evidence_record_chain(bundle: dict[str, Any]) -> None:
    records = bundle["evidence_revisions"][-1]["records"]
    previous_hash = "genesis"
    for record in records:
        record["previous_record_sha256"] = previous_hash
        record["record_sha256"] = governance.record_sha256(record)
        previous_hash = record["record_sha256"]
    refresh_head_binding(bundle, "evidence_revisions")


class GovernanceCorrectionContractTests(unittest.TestCase):
    def setUp(self) -> None:
        self.bundle = load_minimal_bundle()

    def validate_codes(self, bundle: dict[str, Any]) -> set[str]:
        return error_codes(governance.validate_governance(bundle, root=ROOT))

    def ancestry_codes(self, revisions: list[dict[str, Any]]) -> set[str]:
        return error_codes(
            governance.validate_revision_ancestry(
                revisions,
                family="evidence_revisions",
            )
        )

    def test_genesis_without_reason_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[0].pop("genesis_reason")

        self.assertIn("genesis_reason_required", self.ancestry_codes(revisions))

    def test_successor_without_predecessor_field_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[-1].pop("previous_revision_path")

        self.assertIn("previous_revision_required", self.ancestry_codes(revisions))

    def test_predecessor_hash_mismatch_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[-1]["previous_revision_sha256"] = "0" * 64

        self.assertIn("previous_revision_hash_mismatch", self.ancestry_codes(revisions))

    def test_removed_historical_record_has_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        del revisions[-1]["records"][0]

        self.assertIn("history_removal", self.ancestry_codes(revisions))

    def test_reordered_historical_records_have_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        records = revisions[-1]["records"]
        records[0], records[1] = records[1], records[0]

        self.assertIn("history_reorder", self.ancestry_codes(revisions))

    def test_rewritten_historical_record_keeps_exact_code(self) -> None:
        revisions = copy.deepcopy(self.bundle["evidence_revisions"])
        revisions[-1]["records"][0]["result"] = "fail"

        self.assertIn("history_rewrite", self.ancestry_codes(revisions))

    def test_reject_without_rationale_has_exact_code(self) -> None:
        decisions = self.bundle["capability_decision_revisions"][-1]["decisions"]
        rejected = next(row for row in decisions if row["decision"] == "reject")
        rejected["reason"] = ""
        refresh_head_binding(self.bundle, "capability_decision_revisions")

        self.assertIn("reject_rationale_required", self.validate_codes(self.bundle))

    def test_verified_capability_without_evidence_has_exact_code(self) -> None:
        capabilities = self.bundle["public_baseline_revisions"][-1]["capabilities"]
        verified = next(row for row in capabilities if row["coverage_state"] == "verified")
        verified["evidence_ids"] = []
        refresh_head_binding(self.bundle, "public_baseline_revisions")

        self.assertIn("verified_evidence_required", self.validate_codes(self.bundle))

    def test_expired_evidence_has_exact_stale_diagnostic(self) -> None:
        records = self.bundle["evidence_revisions"][-1]["records"]
        record = next(row for row in records if row["evidence_id"] == "ev.cap.public.current")
        record["expires_at"] = "2026-07-15T00:01:30Z"
        refresh_evidence_record_chain(self.bundle)

        errors = governance.validate_governance(self.bundle, root=ROOT)
        matching = [
            error
            for error in errors
            if error.code == "evidence_expired"
            and error.subject_id == "cap.public.cli"
            and error.freshness == "stale"
        ]
        self.assertEqual(1, len(matching), [error.as_dict() for error in errors])

    def test_newer_failed_retest_has_exact_stale_diagnostic(self) -> None:
        records = self.bundle["evidence_revisions"][-1]["records"]
        previous = next(row for row in records if row["evidence_id"] == "ev.cap.public.current")
        failed = copy.deepcopy(previous)
        failed.update(
            {
                "sequence": len(records) + 1,
                "evidence_id": "ev.cap.public.failed-retest",
                "evidence_type": "failed_retest",
                "result": "fail",
                "observed_at": "2026-07-15T00:01:30Z",
                "supersedes_evidence_ids": [],
            }
        )
        failed.pop("expires_at", None)
        records.append(failed)
        capabilities = self.bundle["public_baseline_revisions"][-1]["capabilities"]
        capability = next(row for row in capabilities if row["capability_id"] == "cap.public.cli")
        capability["evidence_ids"].append(failed["evidence_id"])
        refresh_evidence_record_chain(self.bundle)
        refresh_head_binding(self.bundle, "public_baseline_revisions")

        errors = governance.validate_governance(self.bundle, root=ROOT)
        matching = [
            error
            for error in errors
            if error.code == "newer_failed_retest"
            and error.subject_id == "cap.public.cli"
            and error.freshness == "stale"
        ]
        self.assertEqual(1, len(matching), [error.as_dict() for error in errors])
        self.assertIn("ev.cap.public.current", {row["evidence_id"] for row in records})

    def test_invalid_alias_has_exact_code(self) -> None:
        aliases = self.bundle["repository_registry_revisions"][-1]["repositories"][0]["aliases"]
        aliases[0].pop("reason")
        refresh_head_binding(self.bundle, "repository_registry_revisions")

        self.assertIn("alias_contract_invalid", self.validate_codes(self.bundle))

    def test_secret_shaped_value_has_exact_code(self) -> None:
        decisions = self.bundle["capability_decision_revisions"][-1]["decisions"]
        decisions[0]["reason"] = "synthetic sk-abcdefghijklmnop value"
        refresh_head_binding(self.bundle, "capability_decision_revisions")

        self.assertIn("sensitive_value", self.validate_codes(self.bundle))

    def test_registry_count_mismatch_has_exact_summary_code(self) -> None:
        registry = self.bundle["repository_registry_revisions"][-1]
        registry["expected_count"] = 37
        refresh_head_binding(self.bundle, "repository_registry_revisions")

        self.assertIn("summary_mismatch", self.validate_codes(self.bundle))


if __name__ == "__main__":
    unittest.main()
