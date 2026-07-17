# Kiana 百 Agent 项目完成主控 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 以 170 张 Task Card、120 Agent 标准注册池和最多 32 个活跃槽位，把当前 Kiana 工作树收敛为经过独立审查、fresh verification、跨平台与商用验收的 canonical 产品基线。

**Architecture:** 版本化目录 `docs/agent-program/kiana-completion/` 保存稳定计划，外部控制面 `tools/agent-program/` 保存 SQLite WAL 运行状态并生成不可变 WorkPacket。W0 由唯一 Baseline Curator 串行完成；E19-E24 建立公共契约和控制面后，后续任务按 Dependency DAG、path/semantic conflict graph 和资源 semaphore 分 micro-wave 执行，最后以 shadow mode 将调度能力迁回 Kiana。全项目不采用 TDD：先按批准的合同实现，再运行 focused、adversarial、integration 和 review 验证。

**Tech Stack:** Git、Codex CLI、JSON Schema 2020-12、Python 3 标准库与 `jsonschema`、SQLite WAL、Rust/Cargo、Bash release gates。

---

## 0. Authority And Source Of Truth

执行期间按以下优先级解释状态：

1. canonical Git commit 与 clean worktree。
2. SQLite 中带 fencing epoch 的 Attempt、Review、Verification 与 Integration 记录。
3. 内容寻址的 WorkPacket、ResultPacket 和 evidence artifacts。
4. 本目录中的 Task Card 与 catalog revision。
5. Agent 文本回报。

计划输入固定为：

- `docs/superpowers/specs/2026-07-13-kiana-hundred-agent-completion-program-design.md`
- `docs/agent-program/kiana-completion/program.json`
- `docs/agent-program/kiana-completion/task-card.schema.json`
- `docs/agent-program/kiana-completion/domains.json`
- `docs/agent-program/kiana-completion/ownership.json`
- `docs/agent-program/kiana-completion/references.json`
- `docs/agent-program/kiana-completion/dags/gates.json`
- `docs/agent-program/kiana-completion/acceptance/verification-gates.json`
- `docs/agent-program/kiana-completion/tasks/E.json`
- `docs/agent-program/kiana-completion/tasks/D01.json` 至 `D18.json`

Task Card 是可修订的规划对象。只有 W0 接受后，由 Program Compiler 绑定实际 `base_commit`、`base_tree`、contract hashes、lease、fencing epoch、write scope 和预算生成的 WorkPacket 才可派发。

## 1. Fixed Execution Envelope

| Wave | Task 数 | Builder 上限 | 主要出口门禁 |
| --- | ---: | ---: | --- |
| W0 | 4 | 0 | reviewed `BaselineManifest`、38-reference ledger、170-task DAG |
| W1 | 13 | 4-8 | 四组 contract、lease/fencing、integration queue |
| W2 | 14 | 8-12 | 热点行为不变拆分与 golden parity |
| W3 | 44 | 16 | core domain、stress、recovery |
| W4 | 47 | 16 | provider/integration/client 端到端验收 |
| W5 | 40 | 8-12 | cross-domain train、repository fresh gate |
| W6 | 8 | 2-4 | 三平台、live service、signing、channel、customer evidence |

标准注册池为 120：5 Control Plane、18 Domain Lead、60 Builder/Researcher、20 Reviewer、12 Verifier、5 Integrator/Release。默认活跃 32 槽中最多 16 个 Builder；W0 未接受时 Builder 槽固定为 0。

资源上限：

- Cargo focused compile/test：最多 4。
- workspace test：1。
- release/security gate：1。
- canonical integration writer：1。
- read-only reference research：最多 32，但不得占用 Builder lease。

任何任务只有同时满足全部 `depends_on` 已进入其 `base_commit`、approval 有效、artifact/contract hash 匹配、scope 完整、无 path/semantic lock 冲突、reviewer/verifier/资源槽可用时才是 Ready。

### Task 1: Accept The Versioned Planning Catalog

**Files:**
- Verify: `docs/agent-program/kiana-completion/**/*.json`
- Verify: `docs/superpowers/specs/2026-07-13-kiana-hundred-agent-completion-program-design.md`

- [ ] **Step 1: Parse every JSON artifact**

Run:

```bash
jq empty \
  docs/agent-program/kiana-completion/*.json \
  docs/agent-program/kiana-completion/tasks/*.json \
  docs/agent-program/kiana-completion/dags/*.json \
  docs/agent-program/kiana-completion/acceptance/*.json
```

Expected: exit `0` with no output.

- [ ] **Step 2: Validate all 170 Task Cards against the schema**

Run:

```bash
python3 - <<'PY'
import glob
import json
from pathlib import Path

from jsonschema import Draft202012Validator

root = Path("docs/agent-program/kiana-completion")
schema = json.loads((root / "task-card.schema.json").read_text())
validator = Draft202012Validator(schema)
count = 0
for name in sorted(glob.glob(str(root / "tasks" / "*.json"))):
    document = json.loads(Path(name).read_text())
    for task in document["tasks"]:
        validator.validate(task)
        count += 1
assert count == 170, count
print(f"validated_task_cards={count}")
PY
```

Expected: `validated_task_cards=170`.

- [ ] **Step 3: Verify counts, references, ownership, dependency direction and acyclicity**

Run:

```bash
python3 - <<'PY'
import json
from pathlib import Path

root = Path("docs/agent-program/kiana-completion")
tasks = []
for path in sorted((root / "tasks").glob("*.json")):
    tasks.extend(json.loads(path.read_text())["tasks"])
program = json.loads((root / "program.json").read_text())
domains = json.loads((root / "domains.json").read_text())["domains"]
ownership = json.loads((root / "ownership.json").read_text())["packages"]
references = json.loads((root / "references.json").read_text())["references"]

by_id = {task["task_id"]: task for task in tasks}
assert len(tasks) == program["counts"]["total_tasks"] == 170
assert len(by_id) == 170
assert len(domains) == program["counts"]["domains"] == 18
assert len(ownership) == program["counts"]["ownership_packages"] == 30
assert len(references) == program["counts"]["references"] == 38
assert [task["task_id"] for task in tasks if task["status"] == "ready"] == ["E01"]

ownership_ids = {item["id"] for item in ownership}
wave_rank = {f"W{index}": index for index in range(7)}
for task in tasks:
    assert set(task["ownership_packages"]) <= ownership_ids, task["task_id"]
    assert len(task["candidate_write_paths"]) <= 8, task["task_id"]
    for dependency in task["depends_on"]:
        assert dependency in by_id, (task["task_id"], dependency)
        assert wave_rank[by_id[dependency]["wave"]] <= wave_rank[task["wave"]], (
            task["task_id"], dependency
        )

visiting = set()
visited = set()
def visit(task_id):
    if task_id in visiting:
        raise AssertionError(f"dependency cycle at {task_id}")
    if task_id in visited:
        return
    visiting.add(task_id)
    for dependency in by_id[task_id]["depends_on"]:
        visit(dependency)
    visiting.remove(task_id)
    visited.add(task_id)

for task_id in sorted(by_id):
    visit(task_id)
print("catalog_ok tasks=170 domains=18 ownership=30 references=38 ready=E01")
PY
```

Expected: `catalog_ok tasks=170 domains=18 ownership=30 references=38 ready=E01`.

- [ ] **Step 4: Confirm planning files are committed and isolated from user changes**

Run:

```bash
test -f docs/superpowers/plans/2026-07-13-kiana-w0-baseline-intake-implementation-plan.md
git status --porcelain=v1 -- \
  docs/agent-program/kiana-completion \
  docs/superpowers/plans/2026-07-13-kiana-agent-program-master-plan.md \
  docs/superpowers/plans/2026-07-13-kiana-w0-baseline-intake-implementation-plan.md
```

Expected: no output. Other pre-existing worktree changes are allowed and remain untouched.

### Task 2: Execute W0 Baseline Intake And Freeze

**Files:**
- Execute: `docs/superpowers/plans/2026-07-13-kiana-w0-baseline-intake-implementation-plan.md`
- Create: `docs/agent-program/kiana-completion/baseline/*`
- Create: `docs/agent-program/kiana-completion/capability-ledger.jsonl`
- Modify after approval: `docs/agent-program/kiana-completion/program.json`

- [ ] **Step 1: Assign one Baseline Curator and set Builder capacity to zero**

Run:

```bash
jq -e '
  .status == "blocked_on_w0_baseline" and
  .execution.next_ready_task == "E01" and
  .counts.maximum_active_builders == 16
' docs/agent-program/kiana-completion/program.json
```

Expected: exit `0`. The configured post-W0 cap remains 16, while `dags/gates.json` enforces effective W0 Builder capacity `0`.

- [ ] **Step 2: Execute every checkbox in the W0 plan in order**

Run the commands exactly as recorded in `2026-07-13-kiana-w0-baseline-intake-implementation-plan.md`.

Expected: E01 capture is immutable; E02 pauses at the explicit user approval checkpoint before baseline commits; E03 starts only from the approved clean baseline; E04 accepts only an acyclic 170-task catalog.

- [ ] **Step 3: Record the accepted W0 exit state**

Run:

```bash
jq -e '
  .schema == "kiana.baseline-manifest.v1" and
  .approval.status == "approved" and
  .git.source_materialization_commit != null and
  .git.source_materialization_tree != null
' docs/agent-program/kiana-completion/baseline/baseline-manifest.json
git status --porcelain=v1
```

Expected: `jq` exits `0`; `git status` prints nothing after the approved baseline materialization.

### Task 3: Execute W1 Contracts And External Control Plane

**Files:**
- Task Cards: `docs/agent-program/kiana-completion/tasks/E.json`, `D01.json`, `D02.json`, `D03.json`, `D05.json`, `D06.json`
- Create: `tools/agent-program/`
- Create: `docs/agent-program/kiana-completion/control-plane/`
- Modify only through packets: contract paths owned by `P01`, `P11`, `P12`, `P13`, `P14`, `P25`, `P26`, `P28`, `P29`

- [ ] **Step 1: Select the contract-first frontier**

Run:

```bash
jq -r '.tasks[] | select(.task_id == "E19" or .task_id == "E20" or .task_id == "E21" or .task_id == "E22") | [.task_id, .title, (.depends_on | join(","))] | @tsv' \
  docs/agent-program/kiana-completion/tasks/E.json
```

Expected: exactly four rows, E19 through E22. Each remains blocked until the accepted W0 commit contains E02 and E04 evidence.

- [ ] **Step 2: Produce one task-level implementation plan and immutable WorkPacket per contract**

For each of `E19`, `E20`, `E21`, `E22`, the Domain Lead must derive an implementation plan from its exact Task Card, then the Program Compiler must bind the accepted baseline commit, relevant ownership locks, contract hashes, verification commands and required approval. Do not combine two contract IDs in one WorkPacket.

Expected: four packet hashes, four non-overlapping leased worktrees, and no consumer packet based on a pre-contract commit.

- [ ] **Step 3: Integrate contracts serially**

Each contract packet follows `G0 -> independent review -> G1 -> G2/G3 single-package train verification -> canonical_accepted`. After each acceptance, rebase or recreate consumer WorkPackets against the new canonical commit.

Expected: the canonical branch has one writer and all four contract hashes are recorded in the control-plane database.

- [ ] **Step 4: Execute the seven W1 domain contract cards**

Run:

```bash
jq -r '.tasks[] | select(.wave == "W1") | [.task_id, .title, (.depends_on | join(","))] | @tsv' \
  docs/agent-program/kiana-completion/tasks/D01.json \
  docs/agent-program/kiana-completion/tasks/D02.json \
  docs/agent-program/kiana-completion/tasks/D03.json \
  docs/agent-program/kiana-completion/tasks/D05.json \
  docs/agent-program/kiana-completion/tasks/D06.json
```

Expected: exactly `D01-01`, `D01-02`, `D02-01`, `D03-01`, `D05-01`, `D06-01`, `D06-03`. Dispatch each only after its E19-E22 and same-wave dependencies are canonical; integrate public contract cards as single-package trains.

- [ ] **Step 5: Implement E23 before E24**

Run the implementation and verification commands from the cards:

```bash
python3 -m unittest discover -s tools/agent-program/tests -p 'test_*lease*.py'
python3 -m unittest discover -s tools/agent-program/tests -p 'test_*queue*.py'
```

Expected after E23/E24 acceptance: both commands exit `0`; concurrent claim has one winner; stale fencing tokens cannot enqueue; queue records bind packet/result/review/verification/base hashes; Cargo, workspace and release semaphores are cross-process safe.

- [ ] **Step 6: Freeze CLI status and report output contracts**

E23 must create `docs/agent-program/kiana-completion/control-plane/schemas/agent-program-cli-status.v1.schema.json`. E24 must create `docs/agent-program/kiana-completion/control-plane/schemas/agent-program-cli-report.v1.schema.json`. The status schema defines `scheduler.active_epoch`, all `limits`, wave `counts`, and `progress`; the report schema defines `capabilities`, `domains`, `tasks`, evidence freshness and blocker counts used below.

Expected: schema validation passes before any master-plan `jq` assertion consumes these fields.

- [ ] **Step 7: Verify the control plane before raising concurrency**

Run:

```bash
python3 tools/agent-program/cli.py validate \
  --program docs/agent-program/kiana-completion/program.json
python3 tools/agent-program/cli.py status --format json | jq -e '
  .scheduler.active_epoch != null and
  .limits.canonical_writers == 1 and
  .limits.workspace_test_slots == 1 and
  .limits.release_gate_slots == 1
'
```

Expected: both commands exit `0`. These commands are unavailable before E23/E24 and therefore cannot be used as evidence for those packets themselves.

### Task 4: Execute W2 Behavior-Preserving Hotspot Decomposition

**Files:**
- Task Cards: `docs/agent-program/kiana-completion/tasks/E.json` (`E05` through `E18`)
- Modify: only each packet's bound candidate paths

- [ ] **Step 1: Compile W2 Ready tasks after their contract dependencies are canonical**

Run:

```bash
python3 tools/agent-program/cli.py compile --wave W2 --ready-only
python3 tools/agent-program/cli.py status --wave W2 --format json | jq -e '
  .counts.total == 14 and .limits.builders >= 8 and .limits.builders <= 12
'
```

Expected: total W2 count is 14; only dependency-satisfied packets become Ready.

- [ ] **Step 2: Dispatch by conflict-graph color**

Run:

```bash
python3 tools/agent-program/cli.py dispatch --wave W2 --one-color --max-builders 12
```

Expected: no two active leases overlap a physical path or semantic lock; `Cargo.lock`, registries, schema index and release workflow remain exclusive.

- [ ] **Step 3: Enforce behavior parity**

Every W2 packet must first capture its named baseline golden, perform only move/extract/re-export changes, then rerun the exact focused command. Any changed route, schema, public export, ordering, error, or gate becomes a new semantic-fix child task rather than being hidden in the refactor.

Expected: all 14 W2 tasks reach `canonical_accepted`; no semantic-fix diff is mixed into a decomposition packet.

### Task 5: Execute W3 Core Product Domains

**Files:**
- Task Cards: `tasks/D01.json` through `D06.json`, plus W3 cards in `D07.json`, `D09.json`, `D11.json`, `D16.json`

- [ ] **Step 1: Compile the 44-card W3 frontier**

Run:

```bash
python3 tools/agent-program/cli.py compile --wave W3 --ready-only
python3 tools/agent-program/cli.py status --wave W3 --format json | jq -e '.counts.total == 44'
```

Expected: total W3 inventory is 44; Ready is a dependency-filtered subset.

- [ ] **Step 2: Preserve contract-first ordering inside each domain**

Run only contract cards before their consumers. Domain Integrators may maintain parallel staging branches, but RuntimeEvent, Session, Tool, Workflow, TaskCard, WorkPacket and Evidence contracts integrate as single-package trains.

Expected: no consumer WorkPacket references an unaccepted contract hash.

- [ ] **Step 3: Require recovery and stress evidence**

Every domain exit must include focused G0, crate/domain G1, and applicable repeated/concurrency/recovery G2 evidence. A flaky test is rerun with fixed seed three times; continued instability blocks the domain.

Expected: D01-D06 core acceptance packets remain blocked until normal, error, crash, replay, permission and integration paths are all evidenced.

### Task 6: Execute W4 Provider And Product-Surface Domains

**Files:**
- Task Cards: W4 cards in `D07.json` through `D16.json`

- [ ] **Step 1: Compile and color the 47-card W4 graph**

Run:

```bash
python3 tools/agent-program/cli.py compile --wave W4 --ready-only
python3 tools/agent-program/cli.py status --wave W4 --format json | jq -e '.counts.total == 47'
python3 tools/agent-program/cli.py dispatch --wave W4 --one-color --max-builders 16
```

Expected: W4 total is 47, active Builder count never exceeds 16, and shared provider/client/retrieval locks serialize conflicting work.

- [ ] **Step 2: Reject API-only completeness**

Provider, MCP, Remote, RAG, Memory, Notebook, Web and IDE cards require production path, normal and failure tests, a consumer/user entry, fresh verification and reference-difference evidence. Schema, test name or route presence alone cannot advance capability state to `implemented`.

Expected: each accepted W4 domain has a real end-to-end acceptance artifact.

### Task 7: Execute W5 Cross-Domain Integration

**Files:**
- Task Cards: all W5 cards in `D01.json` through `D18.json`

- [ ] **Step 1: Compile the 40-card W5 graph**

Run:

```bash
python3 tools/agent-program/cli.py compile --wave W5 --ready-only
python3 tools/agent-program/cli.py status --wave W5 --format json | jq -e '.counts.total == 40'
```

Expected: total W5 inventory is 40; blocked domain exits are not dispatched.

- [ ] **Step 2: Build 4-to-8 packet merge trains**

Run:

```bash
python3 tools/agent-program/cli.py integrate --wave W5 --train-size-min 4 --train-size-max 8
```

Expected: only conflict-independent ordinary packets share a train. Contract/schema, `Cargo.lock`, auth/security, release workflow, migration and cross-platform sandbox packets each use a single-package train.

- [ ] **Step 3: Run the repository gate in its single slot**

Run the G3 commands declared in `acceptance/verification-gates.json` from a fresh detached worktree at the train commit.

Expected: workspace check/test, schema and local-RC gates all bind the same commit hash; failure triggers train bisection and requeue, not partial canonical merge.

### Task 8: Execute W6 Commercial Acceptance

**Files:**
- Task Cards: `docs/agent-program/kiana-completion/tasks/D18.json` (`D18-03` through `D18-10`)
- Verify: release scripts, signed artifacts, platform and customer evidence

- [ ] **Step 1: Confirm all W6 dependencies and external authorities**

Run:

```bash
python3 tools/agent-program/cli.py compile --wave W6 --ready-only
python3 tools/agent-program/cli.py status --wave W6 --format json | jq -e '.counts.total == 8'
```

Expected: total W6 inventory is 8. Missing signing identity, notarization, live service, channel or customer evidence remains a typed blocker and cannot be replaced by local fixtures.

- [ ] **Step 2: Execute G4 only through the Release Controller**

Run platform, live-service, signing, channel and customer commands exactly as bound in `verification-gates.json` and the accepted WorkPackets, one release slot at a time.

Expected: every proof binds issuer, artifact digest, source revision, platform, timestamp/freshness and acceptance authority.

- [ ] **Step 3: Require zero commercial blockers**

Run:

```bash
bash scripts/commercial-release-blockers-report.sh --fail-on-blockers --json
```

Expected: exit `0` with `local_blocking=0`, `external_blocking=0`, and `blocking=0`. Any external blocker keeps `D18-10` unaccepted.

### Task 9: Shadow-Migrate The Control Plane Into Kiana

**Files:**
- Modify through new accepted packets: `kiana-types`, `kiana-tasks`, `kiana-tools`, `kiana-commands`, `kiana-entrypoints`
- Preserve: external SQLite schema and packet/evidence contracts

- [ ] **Step 1: Reach the migration entry threshold**

Run:

```bash
python3 tools/agent-program/cli.py status --format json | jq -e '
  .progress.completed_waves >= 3 and
  .progress.accepted_packets >= 50
'
```

Expected: exit `0` before Kiana is allowed to shadow scheduling decisions.

- [ ] **Step 2: Run Kiana read-only against the same state**

For each decision, record external and Kiana Ready set, lease choice, retry action and integration ordering without allowing Kiana to mutate scheduler state.

Expected: both engines reference the same program revision, database snapshot, canonical base and scheduler epoch.

- [ ] **Step 3: Require 20 consecutive matching packet decisions**

Run the decision-comparison command introduced by the Kiana control-plane migration packet.

Expected: 20 consecutive exact matches and zero dual-writer intervals. A mismatch resets the streak and creates an Incident Record.

- [ ] **Step 4: Cut over one scheduler epoch**

Deactivate the external active epoch, atomically create the Kiana epoch, then verify no old fencing token can claim, heartbeat or enqueue.

Expected: exactly one active scheduler epoch; external tooling remains recovery-compatible and read-only by default.

### Task 10: Final Program Acceptance

**Files:**
- Verify: complete capability ledger, domain evidence, release evidence and canonical source tree

- [ ] **Step 1: Verify capability and domain closure**

Run:

```bash
python3 tools/agent-program/cli.py report --format json | jq -e '
  .capabilities.references_classified == 38 and
  .capabilities.unclassified == 0 and
  .domains.total == 18 and
  .domains.done == 18 and
  .tasks.total == 170 and
  .tasks.unexplained_terminal_failures == 0
'
```

Expected: exit `0`.

- [ ] **Step 2: Verify the canonical source and evidence chain**

Run fresh G3 and G4 from the exact release source commit. Check that every accepted Task Card resolves to WorkPacket, Attempt, Result, independent Review, fresh Verification, canonical commit and capability evidence hashes.

Expected: no stale evidence, no scope deviation, no missing approval and no unverifiable Agent-only completion claim.

- [ ] **Step 3: Sign the final acceptance record**

The Product Owner and Release Controller approve the final report only after `commercial-release-blockers-report.sh --fail-on-blockers --json` exits `0` and Linux, macOS, Windows, real provider, Remote, MCP, Web/IDE, signing, channel and customer evidence all remain fresh.

Expected: `D18-10` reaches `canonical_accepted`, the program reaches `commercial_proven`, and the acceptance record binds the release artifact digests and source revision.

## Stop-The-Line Rules

Immediately stop new dispatch when any of these occurs: baseline drift, unknown ownership, lease/fencing violation, contract hash drift, evidence corruption, secret exposure, non-controller canonical write, verification backlog beyond two micro-waves, the same failure twice, or unauthenticated commercial proof.

Resume only after an Incident Record contains the exact failed packet/attempt, preserved logs, root cause or external blocker, corrected DAG/packet/policy revision, and approval from the responsible controller.
