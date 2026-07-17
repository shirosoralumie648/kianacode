# Kiana W0 Baseline Intake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (- [ ]) syntax for tracking.

**Goal:** 在不丢失、回退、自动暂存或误归属任何现有用户变更的前提下，完成 E01-E04，形成经批准的 canonical baseline、38-reference capability ledger 和可执行的 170-task DAG。

**Architecture:** 唯一 Baseline Curator 在 root worktree 中串行捕获和审查。提交链先用临时 Git index 与 git commit-tree 离线构造，在独立 worktree 逐提交验证，用户批准后才原子推进当前分支；E03 只读并行扫描参考仓库，E04 串行冻结 catalog 和 gates。

**Tech Stack:** Git plumbing、Python 3 标准库、JSON/JSONL、SHA-256、jq、Codex read-only Researcher、现有 Cargo/Bash focused gates。

---

## Safety Contract

W0 全程只有一个 Baseline Curator。Builder concurrency 固定为 0；其他 Agent 只能读取 canonical root 或各自的 reference checkout。

禁止执行：

- git reset
- git checkout
- git restore
- git stash
- git clean
- 自动 rebase、自动 merge 或批量删除

用户已批准保留所有 intake 变更，但 branch advancement、疑似 secret/generated/reference drift 的处理，以及 excluded capability 仍需单独批准。

### Task 1: E01 Immutable Intake Capture

**Files:**

- Create: docs/agent-program/kiana-completion/baseline/intake-paths.json
- Create: docs/agent-program/kiana-completion/baseline/intake-manifest.json

- [ ] **Step 1: Prove E01 is the only Ready task**

Run:

    jq -s -e '[.[].tasks[] | select(.status == "ready") | .task_id] == ["E01"]' docs/agent-program/kiana-completion/tasks/*.json

Expected: exit 0.

- [ ] **Step 2: Run the precondition verification**

Run:

    jq -e '.schema == "kiana.baseline-intake.v1" and (.paths.count > 0) and (.git.head | length == 40)' docs/agent-program/kiana-completion/baseline/intake-manifest.json

Expected: non-zero because the live intake artifacts do not exist yet. If either output already exists, stop and classify it as baseline drift.

- [ ] **Step 3: Capture status, patch and per-path identities**

Run this exact command from the repository root:

    python3 - <<'PY'
    from __future__ import annotations
    import hashlib
    import json
    import os
    import stat
    import subprocess
    import tempfile
    from datetime import datetime, timezone
    from pathlib import Path

    root = Path.cwd()
    out = root / "docs/agent-program/kiana-completion/baseline"
    paths_path = out / "intake-paths.json"
    manifest_path = out / "intake-manifest.json"
    output_paths = {
        paths_path.relative_to(root).as_posix(),
        manifest_path.relative_to(root).as_posix(),
    }
    if paths_path.exists() or manifest_path.exists():
        raise SystemExit("capture output already exists")

    def run(*args, check=True):
        return subprocess.run(
            args,
            cwd=root,
            check=check,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )

    def digest_bytes(value):
        return hashlib.sha256(value).hexdigest()

    def file_identity(relative):
        path = root / relative
        try:
            info = path.lstat()
        except FileNotFoundError:
            return {"kind": "missing", "mode": None, "size": None, "sha256": None}
        mode = format(stat.S_IMODE(info.st_mode), "04o")
        if stat.S_ISLNK(info.st_mode):
            value = os.fsencode(os.readlink(path))
            return {"kind": "symlink", "mode": mode, "size": len(value), "sha256": digest_bytes(value)}
        if stat.S_ISREG(info.st_mode):
            value = path.read_bytes()
            return {"kind": "file", "mode": mode, "size": len(value), "sha256": digest_bytes(value)}
        return {"kind": "directory", "mode": mode, "size": info.st_size, "sha256": None}

    def parse_status(raw):
        records = raw.split(b"\0")
        result = []
        index = 0
        while index < len(records):
            record = records[index]
            index += 1
            if not record:
                continue
            kind = record[:1]
            original = None
            if kind == b"1":
                fields = record.split(b" ", 8)
                xy = fields[1].decode("ascii")
                relative = os.fsdecode(fields[8])
            elif kind == b"2":
                fields = record.split(b" ", 9)
                xy = fields[1].decode("ascii")
                relative = os.fsdecode(fields[9])
                original = os.fsdecode(records[index])
                index += 1
            elif kind == b"u":
                fields = record.split(b" ", 10)
                xy = fields[1].decode("ascii")
                relative = os.fsdecode(fields[10])
            elif kind == b"?":
                xy = "??"
                relative = os.fsdecode(record[2:])
            else:
                raise SystemExit("unsupported porcelain-v2 record")
            result.append(
                {
                    "path": relative,
                    "original_path": original,
                    "record_type": kind.decode("ascii"),
                    "xy": xy,
                    "current": file_identity(relative),
                }
            )
        return sorted(result, key=lambda item: (item["path"], item["original_path"] or ""))

    def status():
        return run("git", "status", "--porcelain=v2", "-z", "--untracked-files=all").stdout

    def atomic_json(path, value):
        payload = (json.dumps(value, indent=2, sort_keys=True) + "\n").encode()
        path.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temporary = tempfile.mkstemp(prefix="capture-", dir=path.parent)
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
        return payload

    raw_before = status()
    entries_before = parse_status(raw_before)
    if not entries_before:
        raise SystemExit("expected approved dirty-worktree inputs")

    paths_payload = atomic_json(
        paths_path,
        {"schema": "kiana.baseline-intake-paths.v1", "entries": entries_before},
    )
    worktree_patch = run("git", "diff", "--binary", "--no-ext-diff").stdout
    index_patch = run("git", "diff", "--cached", "--binary", "--no-ext-diff").stdout
    head = run("git", "rev-parse", "HEAD").stdout.decode().strip()
    tree = run("git", "rev-parse", "HEAD^{tree}").stdout.decode().strip()
    branch = run("git", "symbolic-ref", "--short", "-q", "HEAD", check=False).stdout.decode().strip() or None
    manifest = {
        "schema": "kiana.baseline-intake.v1",
        "captured_at": datetime.now(timezone.utc).isoformat(),
        "git": {
            "head": head,
            "head_tree": tree,
            "branch": branch,
            "raw_status_sha256": digest_bytes(raw_before),
            "worktree_patch_sha256": digest_bytes(worktree_patch),
            "index_patch_sha256": digest_bytes(index_patch),
        },
        "paths": {
            "file": paths_path.relative_to(root).as_posix(),
            "sha256": digest_bytes(paths_payload),
            "count": len(entries_before),
        },
        "capture_outputs": sorted(output_paths),
        "safety": {
            "source_files_modified": False,
            "destructive_git_commands_used": False,
        },
    }
    atomic_json(manifest_path, manifest)
    entries_after = [item for item in parse_status(status()) if item["path"] not in output_paths]
    if entries_after != entries_before:
        raise SystemExit("baseline drift detected during capture")
    print(f"captured_paths={len(entries_before)} head={head}")
    PY

Expected: one captured_paths line with a positive count and a 40-character HEAD. The only new files are the two capture outputs.

- [ ] **Step 4: Run the post-capture verification**

Run:

    jq -e '.schema == "kiana.baseline-intake.v1" and (.paths.count > 0) and (.git.head | test("^[0-9a-f]{40}$")) and .safety.source_files_modified == false and .safety.destructive_git_commands_used == false' docs/agent-program/kiana-completion/baseline/intake-manifest.json

    python3 - <<'PY'
    import hashlib
    import json
    from pathlib import Path
    root = Path("docs/agent-program/kiana-completion/baseline")
    manifest = json.loads((root / "intake-manifest.json").read_text())
    payload = (root / "intake-paths.json").read_bytes()
    entries = json.loads(payload)["entries"]
    assert hashlib.sha256(payload).hexdigest() == manifest["paths"]["sha256"]
    assert len(entries) == manifest["paths"]["count"]
    keys = [(item["path"], item["original_path"]) for item in entries]
    assert len(keys) == len(set(keys))
    print(f"intake_ok paths={len(entries)}")
    PY

Expected: jq exits 0 and Python prints intake_ok with the same count.

- [ ] **Step 5: Commit only E01 evidence**

Run:

    git add -- docs/agent-program/kiana-completion/baseline/intake-paths.json docs/agent-program/kiana-completion/baseline/intake-manifest.json
    git commit --only -m "chore: capture W0 dirty-worktree intake" -- docs/agent-program/kiana-completion/baseline/intake-paths.json docs/agent-program/kiana-completion/baseline/intake-manifest.json

Expected: one commit with exactly two paths. Previously staged, unstaged and untracked user inputs remain byte-identical.

### Task 2: E02 Classify Every Captured Path

**Files:**

- Create: docs/agent-program/kiana-completion/baseline/review-classification.json
- Create: docs/agent-program/kiana-completion/baseline/secret-review.json
- Create: docs/agent-program/kiana-completion/baseline/commit-plan.json

- [ ] **Step 1: Build the review table**

Create review-classification.json with schema kiana.baseline-review.v1. Every intake entry appears exactly once with:

    {
      "path": "exact repository path",
      "decision": "preserve",
      "ownership_packages": ["P01"],
      "change_set": "stable-kebab-case-slice-id",
      "risk": "critical",
      "content_class": "source",
      "generated_output": false,
      "reference_drift": false,
      "secret_review": "clean",
      "notes": "Concrete ownership and slice rationale."
    }

Allowed content_class values are source, test, schema, script, documentation, configuration, lockfile and runtime-artifact. A runtime artifact, generated output, reference drift or secret-remediation-required decision blocks E02 until the user approves a concrete disposition.

- [ ] **Step 2: Run a redacted secret heuristic over only captured files**

Run:

    python3 - <<'PY'
    import hashlib
    import json
    import re
    from pathlib import Path
    root = Path.cwd()
    baseline = root / "docs/agent-program/kiana-completion/baseline"
    entries = json.loads((baseline / "intake-paths.json").read_text())["entries"]
    patterns = {
        "private-key": re.compile(rb"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----"),
        "aws-access-key": re.compile(rb"AKIA[0-9A-Z]{16}"),
        "github-token": re.compile(rb"gh[pousr]_[A-Za-z0-9_]{30,}"),
        "generic-secret": re.compile(rb"(?i)(api[_-]?key|secret|token|password)[ ]*[:=][ ]*[^ \r\n]{16,}"),
    }
    findings = []
    for entry in entries:
        path = root / entry["path"]
        if not path.is_file() or path.is_symlink():
            continue
        payload = path.read_bytes()
        for kind, pattern in patterns.items():
            for match in pattern.finditer(payload):
                findings.append(
                    {
                        "path": entry["path"],
                        "line": payload.count(b"\n", 0, match.start()) + 1,
                        "kind": kind,
                        "match_sha256": hashlib.sha256(match.group(0)).hexdigest(),
                        "redacted": True,
                    }
                )
    report = {
        "schema": "kiana.baseline-secret-review.v1",
        "scanner": "bounded-redacted-heuristic-v1",
        "manual_review_required": True,
        "findings": findings,
    }
    (baseline / "secret-review.json").write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(f"redacted_findings={len(findings)}")
    PY

Expected: only path, line, type and one-way hash are stored; no secret value is printed or persisted in the report.

- [ ] **Step 3: Define the ordered commit plan**

Create commit-plan.json with schema kiana.baseline-commit-plan.v1 and this contract:

    {
      "schema": "kiana.baseline-commit-plan.v1",
      "intake_manifest_sha256": "actual SHA-256",
      "start_commit": "current E01 commit",
      "slices": [
        {
          "slice_id": "stable-kebab-case-slice-id",
          "title": "Imperative commit subject",
          "ownership_packages": ["P01"],
          "risk": "critical",
          "paths": ["exact repository path"],
          "verification_commands": ["exact focused command"],
          "approval_required": true,
          "approval_status": "pending"
        }
      ]
    }

Shared types/contracts precede consumers; implementation precedes product documentation; Cargo.lock belongs to the slice that changes dependency requirements. Each captured path appears in exactly one slice.

- [ ] **Step 4: Validate review completeness before branch mutation**

Run:

    python3 - <<'PY'
    import hashlib
    import json
    from pathlib import Path
    root = Path("docs/agent-program/kiana-completion")
    baseline = root / "baseline"
    manifest_bytes = (baseline / "intake-manifest.json").read_bytes()
    intake = json.loads((baseline / "intake-paths.json").read_text())["entries"]
    review = json.loads((baseline / "review-classification.json").read_text())
    plan = json.loads((baseline / "commit-plan.json").read_text())
    ownership = {item["id"] for item in json.loads((root / "ownership.json").read_text())["packages"]}
    input_paths = [item["path"] for item in intake]
    review_paths = [item["path"] for item in review["entries"]]
    slice_paths = [path for item in plan["slices"] for path in item["paths"]]
    assert plan["intake_manifest_sha256"] == hashlib.sha256(manifest_bytes).hexdigest()
    assert sorted(input_paths) == sorted(review_paths) == sorted(slice_paths)
    assert len(review_paths) == len(set(review_paths))
    assert len(slice_paths) == len(set(slice_paths))
    for item in review["entries"]:
        assert item["decision"] == "preserve"
        assert item["ownership_packages"] and set(item["ownership_packages"]) <= ownership
        assert item["secret_review"] in {"clean", "false-positive-approved"}
        assert item["generated_output"] is False
        assert item["reference_drift"] is False
    for item in plan["slices"]:
        assert item["verification_commands"]
        assert item["approval_status"] in {"pending", "approved"}
    print(f"review_ok paths={len(input_paths)} slices={len(plan['slices'])}")
    PY

Expected: review_ok with the captured path count and a positive slice count.

- [ ] **Step 5: Obtain explicit slice-plan approval**

Present each slice in order with exact paths, owner packages, risk, verification commands and all resolved findings. Do not construct commits until the user explicitly approves this complete plan. After approval, change every slice approval_status from pending to approved and run:

    jq -e 'all(.slices[]; .approval_status == "approved")' docs/agent-program/kiana-completion/baseline/commit-plan.json

Expected: exit 0 before Task 3 begins.

### Task 3: E02 Build And Verify The Commit Chain Offline

**Files:**

- Read: docs/agent-program/kiana-completion/baseline/commit-plan.json
- Create outside repository: /tmp/kiana-w0-chain.jsonl
- Do not update the current branch in this task

- [ ] **Step 1: Prove the root has not drifted**

Run:

    test "$(git rev-parse HEAD)" = "$(jq -r '.start_commit' docs/agent-program/kiana-completion/baseline/commit-plan.json)"

Then rehash every file/symlink from intake-paths.json and compare to its captured current.sha256. Expected: all identities match.

- [ ] **Step 2: Construct commits with a temporary index**

Run:

    python3 - <<'PY'
    import json
    import os
    import subprocess
    import tempfile
    from pathlib import Path

    root = Path.cwd()
    plan = json.loads((root / "docs/agent-program/kiana-completion/baseline/commit-plan.json").read_text())
    parent = plan["start_commit"]
    chain = []
    with tempfile.TemporaryDirectory(prefix="kiana-w0-index-") as temporary:
        index_path = str(Path(temporary) / "index")
        for item in plan["slices"]:
            Path(index_path).unlink(missing_ok=True)
            env = os.environ | {"GIT_INDEX_FILE": index_path}
            subprocess.run(["git", "read-tree", parent], cwd=root, env=env, check=True)
            subprocess.run(["git", "add", "-A", "--", *item["paths"]], cwd=root, env=env, check=True)
            tree = subprocess.check_output(["git", "write-tree"], cwd=root, env=env, text=True).strip()
            message = item["title"] + "\n\nW0-Slice: " + item["slice_id"] + "\n"
            commit = subprocess.check_output(
                ["git", "commit-tree", tree, "-p", parent],
                cwd=root,
                input=message,
                text=True,
            ).strip()
            chain.append({"slice_id": item["slice_id"], "parent": parent, "tree": tree, "commit": commit})
            parent = commit
    Path("/tmp/kiana-w0-chain.jsonl").write_text(
        "".join(json.dumps(item, sort_keys=True) + "\n" for item in chain)
    )
    Path("/tmp/kiana-w0-final-commit").write_text(parent + "\n")
    print(f"constructed_slices={len(chain)} final_commit={parent}")
    PY

Expected: the number of constructed commits equals the slice count. The current branch and worktree remain unchanged.

- [ ] **Step 3: Replay each slice verification in a detached worktree**

For each row in /tmp/kiana-w0-chain.jsonl, create a detached temporary worktree at its commit, run that slice's verification_commands with bash -lc, store command, exit status, timestamps and log SHA-256, then remove the worktree.

Expected: every command exits 0. A failure blocks branch advancement and keeps the synthetic chain for review.

- [ ] **Step 4: Prove final tree equality**

Create a second temporary index from start_commit, git add -A every approved slice path, run git write-tree, and compare it with:

    git rev-parse "$(cat /tmp/kiana-w0-final-commit)^{tree}"

Expected: both tree hashes are identical. This is the proof that no approved path was omitted or silently changed.

### Task 4: E02 Approve And Materialize The Canonical Baseline

**Files:**

- Create: docs/agent-program/kiana-completion/baseline/verification-records.jsonl
- Create: docs/agent-program/kiana-completion/baseline/baseline-manifest.json

- [ ] **Step 1: Obtain branch-advance approval**

Report start commit, ordered synthetic commits, verification results and final tree hash. Earlier approval to preserve changes does not replace this branch-advance approval.

- [ ] **Step 2: Atomically advance HEAD**

Run only after approval:

    START="$(jq -r '.start_commit' docs/agent-program/kiana-completion/baseline/commit-plan.json)"
    FINAL="$(cat /tmp/kiana-w0-final-commit)"
    test "$(git rev-parse HEAD)" = "$START"
    git update-ref HEAD "$FINAL" "$START"
    git read-tree "$FINAL"
    test "$(git rev-parse HEAD)" = "$FINAL"

Expected: HEAD equals the approved final commit. git read-tree updates only the index after tree equality has been proven; it does not rewrite worktree files.

- [ ] **Step 3: Write immutable baseline evidence**

verification-records.jsonl contains one record per slice with slice_id, commit, tree, exact commands, exit status, verifier, timestamps and log SHA-256.

baseline-manifest.json contains schema kiana.baseline-manifest.v1, intake and commit-plan SHA-256, intake HEAD, source materialization commit/tree, verification record hash, explicit approver identity/timestamp and clean source status.

- [ ] **Step 4: Validate and commit E02 evidence**

Run:

    jq -e '.schema == "kiana.baseline-manifest.v1" and .approval.status == "approved" and (.git.source_materialization_commit | test("^[0-9a-f]{40}$")) and (.git.source_materialization_tree | test("^[0-9a-f]{40}$"))' docs/agent-program/kiana-completion/baseline/baseline-manifest.json

Commit only review-classification.json, secret-review.json, commit-plan.json, verification-records.jsonl and baseline-manifest.json with:

    git commit --only -m "chore: record reviewed W0 baseline" -- docs/agent-program/kiana-completion/baseline/review-classification.json docs/agent-program/kiana-completion/baseline/secret-review.json docs/agent-program/kiana-completion/baseline/commit-plan.json docs/agent-program/kiana-completion/baseline/verification-records.jsonl docs/agent-program/kiana-completion/baseline/baseline-manifest.json

Expected: source materialization and E02 evidence are committed; git status is clean before E03.

### Task 5: E03 Freeze The Reference Capability Ledger

**Files:**

- Modify: docs/agent-program/kiana-completion/references.json
- Create: docs/agent-program/kiana-completion/capability-ledger.jsonl
- Modify: docs/agent-program/kiana-completion/tasks/E.json and D01.json through D18.json

- [ ] **Step 1: Freeze all 38 reference revisions**

For each references.json entry, record revision_type, revision and source_clean. Git-backed references use git rev-parse HEAD and require an empty git status; non-Git sources use a sorted content-manifest SHA-256.

Run:

    jq -e '.expected_count == 38 and (.references | length) == 38 and all(.references[]; (.revision_type == "git" or .revision_type == "content-manifest") and (.revision | test("^[0-9a-f]{40}$|^[0-9a-f]{64}$")) and .source_clean == true)' docs/agent-program/kiana-completion/references.json

Expected: exactly 38 frozen clean inputs.

- [ ] **Step 2: Dispatch read-only reference researchers**

Each researcher returns JSONL records with schema, capability_id, reference_id, reference_revision, name, behavior, failure_behavior, security_boundary, source_paths, disposition, disposition_reason, target_domains, target_task_ids, acceptance_route, approval and status.

Allowed dispositions are implement, equivalent, alternative and excluded. Excluded requires a Product Owner approval with reason and impact. Researchers never write the canonical ledger.

- [ ] **Step 3: Bind every domain Task Card to actual capability IDs**

Replace each D01-D18 reference_capabilities array with capability_id values that list that task in target_task_ids. Global enabling tasks may retain their global-enabler or contract-freeze identifiers.

- [ ] **Step 4: Run the exact E03 verification command**

Run:

    jq -r '.tasks[] | select(.task_id == "E03") | .verification.command' docs/agent-program/kiana-completion/tasks/E.json | bash

Expected: capability_ledger_ok with 38 references and a positive record count; every domain task binds real ledger capability IDs, and every exclusion is approved.

- [ ] **Step 5: Commit E03 artifacts**

Commit references.json, capability-ledger.jsonl and all Task Card files changed by capability binding with:

    git commit -m "docs: freeze reference capability ledger" -- docs/agent-program/kiana-completion/references.json docs/agent-program/kiana-completion/capability-ledger.jsonl docs/agent-program/kiana-completion/tasks

Expected: one E03 commit; no reference checkout content changes.

### Task 6: E04 Freeze Ownership, DAG And Verification Gates

**Files:**

- Modify: docs/agent-program/kiana-completion/program.json
- Verify: docs/agent-program/kiana-completion/task-card.schema.json
- Verify: docs/agent-program/kiana-completion/domains.json
- Verify: docs/agent-program/kiana-completion/ownership.json
- Verify: docs/agent-program/kiana-completion/tasks/*.json
- Verify: docs/agent-program/kiana-completion/dags/gates.json
- Verify: docs/agent-program/kiana-completion/acceptance/verification-gates.json

- [ ] **Step 1: Validate all Task Cards against JSON Schema**

Run the jsonschema command in Task 1 Step 2 of the master plan.

Expected: validated_task_cards=170.

- [ ] **Step 2: Run the exact E04 catalog verification**

Run:

    jq -r '.tasks[] | select(.task_id == "E04") | .verification.command' docs/agent-program/kiana-completion/tasks/E.json | bash

Expected: catalog_ok tasks=170 acyclic=true ownership=true gates=true.

- [ ] **Step 3: Verify W0 Builder lock and four verification levels**

Run:

    jq -e '.schema == "kiana.agent-wave-gates.v1" and (.wave_gates | length) == 7 and (.wave_gates[] | select(.wave_id == "W0") | .maximum_active_builders) == 0 and .dependency_unlock_policy.required_runtime_task_state == "accepted"' docs/agent-program/kiana-completion/dags/gates.json

    jq -e '.schema == "kiana.agent-verification-gates.v1" and .gate_order == ["focused", "domain", "integration", "commercial"] and (.verification_levels | length) == 4' docs/agent-program/kiana-completion/acceptance/verification-gates.json

Expected: both commands exit 0.

- [ ] **Step 4: Record the accepted W0 source baseline**

Update program.json without changing counts or catalog paths:

    "status": "w0_accepted"
    "baseline.final_base_commit": source_materialization_commit from baseline-manifest.json
    "baseline.final_base_status": "accepted"
    "execution.next_ready_task": null
    "execution.next_ready_tasks": ["E19", "E20", "E21", "E22"]

final_base_commit is the E02 source baseline. Each WorkPacket still binds the newer actual canonical HEAD containing E03/E04.

- [ ] **Step 5: Commit E04 acceptance**

Run:

    jq -e '.status == "w0_accepted" and .baseline.final_base_status == "accepted" and (.baseline.final_base_commit | test("^[0-9a-f]{40}$")) and .execution.next_ready_tasks == ["E19", "E20", "E21", "E22"]' docs/agent-program/kiana-completion/program.json
    git diff --check -- docs/agent-program/kiana-completion
    git commit -m "docs: accept W0 agent program baseline" -- docs/agent-program/kiana-completion/program.json

Expected: E04 acceptance is committed and git status is clean.

### Task 7: W1 Handoff

**Files:**

- Read: docs/agent-program/kiana-completion/tasks/E.json
- Do not modify product code in this task

- [ ] **Step 1: Print the W1 contract frontier**

Run:

    jq -r '.tasks[] | select(.task_id == "E19" or .task_id == "E20" or .task_id == "E21" or .task_id == "E22") | [.task_id, .title] | @tsv' docs/agent-program/kiana-completion/tasks/E.json

Expected: exactly E19, E20, E21 and E22.

- [ ] **Step 2: Stop at the W0 boundary**

Hand control to Task 3 of the master plan. W1 product implementation does not begin inside this W0 plan.
