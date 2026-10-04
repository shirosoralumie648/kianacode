#!/usr/bin/env python3
"""Display the roadmap goal and export its derived Ruflo checklist; runs no tests."""

import argparse
import collections
import datetime
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent
GOAL_ID = "goal-kiana-roadmap-completion-20261004"
TASK_ID = "task-1791102524181-0iivvl"
ROW = re.compile(
    r"^\| (?P<order>\d+) \| (?P<wave>W\d+) \| [^|]+ \| "
    r"\[`(?P<id>[^`]+)`\]\((?P<link>[^)]+)\) \| "
    r"(?P<title>.*?) \| (?P<dependencies>.*?) \| "
    r"(?P<state>✅|🔄|⏳|❓|🚫|⚠️) \| (?P<detail>.*)$"
)


def snapshot(root):
    steps = []
    for number, line in enumerate((root / "docs/roadmap.md").read_text().splitlines(), 1):
        match = ROW.match(line)
        if match:
            values = match.groupdict()
            steps.append({
                "id": values["id"],
                "order": int(values["order"]),
                "wave": values["wave"],
                "title": values["title"],
                "dependencies": re.findall(r"`([^`]+)`", values["dependencies"]),
                "roadmap_state": values["state"],
                "link": values["link"],
                "line": number,
            })
        elif re.match(r"^\| \d+ \| W\d+", line):
            raise ValueError(f"unrecognized roadmap row at line {number}")
    if not steps:
        raise ValueError("roadmap contains no Step queue")
    by_id = {step["id"]: step for step in steps}
    if len(by_id) != len(steps):
        raise ValueError("duplicate Step IDs in roadmap")
    for step in steps:
        unknown = [dep for dep in step["dependencies"] if dep not in by_id]
        if unknown:
            raise ValueError(f"{step['id']}: unregistered dependencies {unknown}")
        step["blocked_by"] = [
            dep for dep in step["dependencies"] if by_id[dep]["roadmap_state"] != "✅"
        ]
        step["ready"] = step["roadmap_state"] in ("🔄", "⏳", "⚠️") and not step["blocked_by"]
    source = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=root, text=True
    ).strip()
    return {
        "goal_id": GOAL_ID,
        "ruflo_task_id": TASK_ID,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "source_snapshot": source,
        "validation": "github-actions-only",
        "counts": dict(collections.Counter(step["roadmap_state"] for step in steps)),
        "total": len(steps),
        "ready_count": sum(step["ready"] for step in steps),
        "steps": steps,
        "limitations": "Derived roadmap index; current CI and whole-card acceptance require separate evidence.",
    }


def export_checklist(root, data):
    path = root / ".claude-flow/data/checklist.json"
    existing = json.loads(path.read_text()) if path.exists() else {"items": []}
    items = existing if isinstance(existing, list) else existing["items"]
    # Retain unrelated goals and pre-existing tasks in the shared Ruflo checklist.
    retained = [item for item in items if item.get("goal_id") != GOAL_ID]
    for step in data["steps"]:
        retained.append({
            "id": f"{GOAL_ID}/{step['id']}",
            "goal_id": GOAL_ID,
            "subject": f"{step['id']}: {step['title']}",
            "status": "completed" if step["roadmap_state"] == "✅" else "pending" if step["ready"] else "blocked",
            "dependencies": step["dependencies"],
            "roadmap_state": step["roadmap_state"],
            "source_snapshot": data["source_snapshot"],
        })
    if isinstance(existing, list):
        document = retained
    else:
        document = {**existing, "items": retained}
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(document, ensure_ascii=False, indent=2) + "\n")
    temporary.replace(path)
    return path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", action="store_true", help="print the entire dependency snapshot")
    parser.add_argument("--checklist", action="store_true", help="sync this goal to the local Ruflo checklist")
    parser.add_argument("--limit", type=int, default=15, help="maximum ready Steps displayed")
    args = parser.parse_args()
    try:
        data = snapshot(ROOT)
        if args.checklist:
            path = export_checklist(ROOT, data)
            if not args.json:
                print(f"Checklist: {path}")
        if args.json:
            print(json.dumps(data, ensure_ascii=False, indent=2))
        else:
            print(f"Goal: {GOAL_ID} ({TASK_ID})")
            print(f"Source: {data['source_snapshot']}")
            print(f"Steps: {data['total']}  States: {data['counts']}  Ready: {data['ready_count']}")
            for step in [item for item in data["steps"] if item["ready"]][:max(0, args.limit)]:
                print(f"{step['order']:03} {step['wave']} {step['id']}: {step['title']}")
            print(data["limitations"])
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"goal inventory failed: {error}\n")


if __name__ == "__main__":
    main()
