"""Behavioral checks for the derived roadmap index, without project execution."""

import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).resolve().parents[1] / "roadmap-goal.py"
SPEC = importlib.util.spec_from_file_location("roadmap_goal", SCRIPT)
GOAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GOAL)


def row(order, step_id, state="🔄", dependencies=(), wave="W1"):
    deps = "、".join(f"`{dep}`" for dep in dependencies) or "—"
    return (
        f"| {order:03} | {wave} | 专项 | [`{step_id}`](#step-{step_id.lower()}) "
        f"| 工作项 {step_id} | {deps} | {state} | [专项卡](#detail) |\n"
    )


class RoadmapSnapshotTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        (self.root / "docs").mkdir()
        self.git = patch.object(GOAL.subprocess, "check_output", return_value="fixture-head\n")
        self.git.start()
        self.addCleanup(self.git.stop)

    def snapshot(self, text):
        (self.root / "docs/roadmap.md").write_text(text)
        return GOAL.snapshot(self.root)

    def test_current_table_format_and_existing_json_fields(self):
        data = self.snapshot("# 队列\n" + row(1, "A", "✅") + row(2, "B", dependencies=("A",)))
        self.assertEqual(data["goal_id"], GOAL.GOAL_ID)
        self.assertEqual(data["ruflo_task_id"], GOAL.TASK_ID)
        self.assertEqual(data["source_snapshot"], "fixture-head")
        self.assertEqual(data["total"], 2)
        self.assertEqual(data["counts"], {"✅": 1, "🔄": 1})
        self.assertEqual(data["ready_count"], 1)
        self.assertEqual(data["validation"], "derived-index-only")
        self.assertEqual(data["steps"][1]["line"], 3)
        self.assertEqual(data["steps"][1]["link"], "#step-b")
        self.assertEqual(data["steps"][1]["title"], "工作项 B")
        self.assertEqual(data["steps"][1]["order"], 2)
        self.assertEqual(data["steps"][1]["wave"], "W1")
        self.assertIn("generated_at", data)
        self.assertIn("separate verification", data["limitations"])
        self.assertEqual(json.loads(json.dumps(data)), data)

    def test_unknown_dependency_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "A: unregistered dependencies"):
            self.snapshot(row(1, "A", dependencies=("missing",)))

    def test_duplicate_id_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "duplicate Step IDs"):
            self.snapshot(row(1, "A") + row(2, "A"))

    def test_cycle_is_rejected_even_for_completed_rows(self):
        with self.assertRaisesRegex(ValueError, "dependency cycle: A -> B -> A"):
            self.snapshot(row(1, "A", "✅", ("B",)) + row(2, "B", "✅", ("A",)))

    def test_self_cycle_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "dependency cycle: A -> A"):
            self.snapshot(row(1, "A", dependencies=("A",)))

    def test_completed_with_open_dependencies_preserves_ready_semantics(self):
        data = self.snapshot(
            row(1, "A") + row(2, "B", "✅", ("A",)) + row(3, "C", "⏳", ("B",))
        )
        self.assertEqual(data["completed_with_open_dependencies_count"], 1)
        self.assertEqual(data["completed_with_open_dependencies"], [{"id": "B", "blocked_by": ["A"]}])
        self.assertEqual([step["ready"] for step in data["steps"]], [True, False, True])
        self.assertEqual(data["ready_count"], 2)
        self.assertEqual(data["execution_ready_count"], 1)
        self.assertEqual(data["steps"][2]["transitive_dependencies"], ["A", "B"])
        self.assertEqual(data["steps"][2]["execution_blocked_by"], ["A"])
        self.assertFalse(data["steps"][2]["execution_ready"])

    def test_ready_excludes_decision_frozen_and_completed_states(self):
        data = self.snapshot("".join(row(i, str(i), state) for i, state in enumerate(
            ("✅", "🔄", "⏳", "⚠️", "❓", "🚫"), 1
        )))
        self.assertEqual([step["ready"] for step in data["steps"]], [False, True, True, True, False, False])
        self.assertEqual(data["completed_with_open_dependencies"], [])

    def test_long_acyclic_graph_does_not_depend_on_recursion_limit(self):
        data = self.snapshot("".join(
            row(i, str(i), dependencies=(str(i + 1),) if i < 1100 else ())
            for i in range(1, 1101)
        ))
        self.assertEqual(data["total"], 1100)
        self.assertEqual(data["ready_count"], 1)
        self.assertEqual(data["execution_ready_count"], 1)
        self.assertEqual(len(data["steps"][0]["transitive_dependencies"]), 1099)
        self.assertEqual(len(data["steps"][0]["execution_blocked_by"]), 1099)

    def test_diamond_closure_is_unique_ordered_and_independent_work_is_ready(self):
        data = self.snapshot(
            row(1, "A") + row(2, "B", "✅", ("A",))
            + row(3, "C", "✅", ("A",)) + row(4, "D", dependencies=("B", "C"))
            + row(5, "independent")
        )
        self.assertTrue(data["steps"][3]["ready"])
        self.assertEqual(data["steps"][3]["transitive_dependencies"], ["A", "B", "C"])
        self.assertEqual(data["steps"][3]["execution_blocked_by"], ["A"])
        self.assertFalse(data["steps"][3]["execution_ready"])
        self.assertTrue(data["steps"][4]["execution_ready"])
        self.assertEqual(data["execution_ready_count"], 2)

    def test_checklist_blocks_hidden_open_ancestors_and_retains_unrelated_items(self):
        data = self.snapshot(
            row(1, "A") + row(2, "B", "✅", ("A",))
            + row(3, "C", dependencies=("B",)) + row(4, "independent")
        )
        path = self.root / ".claude-flow/data/checklist.json"
        path.parent.mkdir(parents=True)
        unrelated = {"id": "other", "goal_id": "other-goal", "status": "pending"}
        old_goal = {"id": "old", "goal_id": GOAL.GOAL_ID, "status": "pending"}
        for document in ([unrelated, old_goal], {"items": [unrelated, old_goal], "metadata": "keep"}):
            with self.subTest(document=document):
                path.write_text(json.dumps(document))
                GOAL.export_checklist(self.root, data)
                result = json.loads(path.read_text())
                items = result if isinstance(result, list) else result["items"]
                self.assertEqual(items[0], unrelated)
                self.assertFalse(any(item["id"] == "old" for item in items))
                by_id = {item["id"].split("/")[-1]: item for item in items[1:]}
                self.assertEqual(by_id["A"]["status"], "pending")
                self.assertEqual(by_id["B"]["status"], "completed")
                self.assertEqual(by_id["C"]["status"], "blocked")
                self.assertEqual(by_id["C"]["execution_blocked_by"], ["A"])
                self.assertEqual(by_id["independent"]["status"], "pending")
                if isinstance(result, dict):
                    self.assertEqual(result["metadata"], "keep")

    def test_malformed_queue_row_and_empty_queue_fail_closed(self):
        for text, message in (("# empty\n", "contains no Step queue"),
                              ("| 001 | W1 | invalid |\n", "unrecognized roadmap row")):
            with self.subTest(text=text), self.assertRaisesRegex(ValueError, message):
                self.snapshot(text)

    def test_cli_reports_open_dependencies_and_json_exports_them(self):
        self.snapshot(row(1, "A") + row(2, "B", "✅", ("A",)))
        for arguments in (["roadmap-goal.py", "--limit", "0"], ["roadmap-goal.py", "--json"]):
            output = io.StringIO()
            with patch.object(GOAL, "ROOT", self.root), patch("sys.argv", arguments), contextlib.redirect_stdout(output):
                GOAL.main()
            if "--json" in arguments:
                self.assertEqual(json.loads(output.getvalue())["completed_with_open_dependencies_count"], 1)
            else:
                self.assertIn("Completed Steps with open dependencies: 1", output.getvalue())


if __name__ == "__main__":
    unittest.main()
