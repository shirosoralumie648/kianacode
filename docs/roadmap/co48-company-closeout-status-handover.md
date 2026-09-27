# CO-48 real-model closeout status and handover

> Snapshot date: 2026-09-28. **This is a documentation-only step.** It changes no code, runs no
> test, calls no provider, confirms no delivery and makes no release. Its job is to say, per
> shipped CompanyOS slice, what is actually proven at which proof level, what is explicitly NOT
> proven, and what the next agent must do first.
>
> Claims here are cross-checked against `CURRENT_STATUS.md` evidence blocks, not against roadmap
> intentions. Where the two disagree, §5 names the disagreement rather than resolving it in the
> ledger's favour.

## 1. What this card asks for, and what this document can deliver

The CO-48 card ([`companyos.md` step-co-48](companyos.md#step-co-48)) asks for a **real-model
business closed loop**: run actual Planner/Builder/Reviewer/Closer against a configured, authorized
live provider on a temporary controlled project, independently verify the generated code, record
provider/model/revision, role differences, failures and cost, then backfill `USER.md`,
`module-map.md`, the status ledger and the roadmap.

**None of that live work happened, and this step cannot do it.** There is no credential, no
authorized live provider, no bounded budget and no operator approval in this checkout. The card's
own rejected-first scenario,
`live_company_run_stops_on_invalid_proposal_budget_limit_and_unapproved_action`, exists only as a
line of text in `companyos.md`; it names no test, fixture, cassette or run anywhere in the
repository, and nothing in this worktree produces a real-model result to retain. The card also
forbids substituting a cassette for a failed real model and still labelling the result `live`; that
prohibition is what makes a documentation-only closeout the honest maximum here.

So this document delivers the **handover half** of CO-48 and explicitly does not deliver the
**real-model half**. `feature_status` for CO-48 stays `partial`; `proof_level` stays `source`. The
roadmap row stays `⏳` and the card stays `⏳`.

## 2. Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`CO-48`](companyos.md#step-co-48) |
| document type | status/handover only; **no code change, no test run, no build, no clippy** |
| feature_status | `partial` — source-level live-evidence contract and fake-model closeout path exist; the real-model loop does not |
| proof_level | `source` — no `local_behavior`, `durable`, `live` or `physical` promotion |
| repository product proof ceiling | `local_behavior`: trusted local repositories plus cassette/fake-script execution |
| real model / account / credential used | **none** |
| reviewer of this document | Codex CO-48 documentation review; no independent runtime or live reviewer |

### Proof vocabulary used below

These are the five levels defined in [`CURRENT_STATUS.md` §1](../../CURRENT_STATUS.md) and restated
in [`dep41-capability-proof-matrix.md`](dep41-capability-proof-matrix.md):

- `source` — source, guard and static-compile evidence only.
- `local_behavior` — a reproducible local behavior receipt (trusted local repo + cassette or
  fake-script execution).
- `durable` — persisted facts plus restart/replay proof.
- `live` — an independent external receipt.
- `physical` — target-environment effect and cleanup proof.

Code existing is not `implemented`. A passing narrow test is not `local_behavior`. `local_behavior`
is not `durable`/`live`/`physical`. This document never promotes a level; it only records one.

## 3. What the card rejects, and how the current code answers it

| Rejected by the card | How the source answers it today | What is still missing |
|---|---|---|
| A fake model labelled as a real model | `CompanyLiveMode::FakeCassette` forbids `proof_level: live`, an `operator_approval_ref` and any `live_provider_evidence_digest`; it also cannot reach `status: Verified` (`company_live_fake_cannot_claim_live`, `company_live_fake_cannot_verify`) | Nothing at the contract level. This fence is source-only: it is a struct invariant, not an attestation about a run that happened. |
| A `live` claim without explicit operator authorization | `CompanyLiveMode::LiveOptIn` requires a trimmed `operator_approval_ref` beginning `approval:`, a non-`fake` provider id, a non-`none` provider revision, and a `live_provider_evidence_digest` | An actual approval record, a named operator, and a real provider revision from a real run. |
| `result_unknown` reported as success | `result_unknown` with `status: Verified` is rejected (`company_live_unknown_cannot_verify`); a non-verified status with an empty `limitations` list is also rejected | Execution of the Unknown/reconcile loop against a real failure. |
| `Verified` without a full business chain | `Verified` demands all four role routes (Planner/Builder/Reviewer/Closer), non-zero request counts, attempt receipts, run/review/delivery/closing receipts, artifact and usage digests, independent reviewer, local delivery confirmation and outcome recording | Every one of those artifacts, for a real run. Today they are constructor arguments, not observed receipts. |
| A durable/recovery claim from a source contract | `CompanyRecoverySnapshot::hydrate` (CO-42) rebuilds a read-only snapshot and *defaults the project to paused*; resume is a separately approved plan | A real cross-process restart against a real EventStore. |
| Second execution loop / direct tool execution | Company command routing is asserted through `RequestEnvelope::company_command` → `DaemonHost::handle`; the CO-47/CO-48 guards assert no `CapabilityBroker::new`, no `ModelClient::new`, no `println!` in the governance/business sources | The guards prove absence of a *second loop in the files they read*, not the absence of one anywhere in the workspace. |

## 4. Failure-first fixture matrix (as shipped, per the current source)

These are the failure paths that exist as fixtures/contract tests today. Every one is source-level.
None is a real-model observation.

| Fixture / contract test | Assertion | Where |
|---|---|---|
| `fake_model_coding_project_produces_closing_receipt` | full fake Company path through `DaemonHost::handle` reaches a `ClosingReceipt`; real `OUTPUT.txt` checked | `kiana-daemon/tests/p3_i06_company_golden.rs` |
| `denied_company_commands_record_rejection_without_runtime_effect` | role denial, missing-project close and idempotency payload drift each record a rejected event with no runtime effect and no duplicate business fact | `kiana-daemon/tests/company_lifecycle.rs` |
| `company_live_fake_cannot_claim_live` | `FakeCassette` cannot claim `live` or carry approval/provider evidence | `kiana-domain/src/company_live_evidence.rs` |
| `company_live_opt_in_approval_ref_invalid` | untrimmed or non-`approval:`-prefixed approval reference is refused | same |
| `company_live_opt_in_provider_identity_missing` | a `fake`/untrimmed provider or `none` revision is refused for `LiveOptIn` | same |
| `company_live_unknown_cannot_verify` | `result_unknown` cannot be `Verified` | same |
| `company_live_verified_evidence_incomplete` | a partial chain cannot be `Verified` | same |
| `company_live_evidence_digest_mismatch` | a mutated field breaks the sealed digest | same |
| `CompanyRecoverySnapshot::hydrate` | rejects sequence gaps, cursor regressions, foreign projects, unknown schema versions, corrupt facts, missing evidence | `kiana-domain/src/company_recovery.rs` |
| `company_lifecycle_fixture_keeps_deny_and_effect_boundaries_explicit` | source guard over the CO-47 fixture/cassette/golden/smoke/baseline | `kiana-core/tests/co47_company_lifecycle_guard.rs` |
| `company_closeout_keeps_fake_golden_and_real_model_limits_explicit` | source guard over the Company closeout sources and this baseline | `kiana-core/tests/co48_company_closeout_guard.rs` |

## 5. Where the ledger and the intentions disagree

The step instruction is to cross-check claims against `CURRENT_STATUS.md` evidence blocks and say so
where they disagree. Three disagreements matter to a reader deciding what to do next.

### 5.1 The CompanyOS baselines name CI workflow files that do not exist

Every CompanyOS baseline from CO-02 onward states a CI-only evidence section naming a file under
`.github/workflows/`. **None of those files exists.** Concretely: 54 distinct workflow paths are
named across `docs/roadmap/co*.md`; the only one that resolves is the shared `.github/workflows/ci.yml`
that several baselines cite. The same pattern holds repo-wide: 273 distinct workflow paths are named
across `docs/roadmap/*.md` and 15 exist.

What actually runs CompanyOS today is the single unified `.github/workflows/ci.yml`, whose
`rust` job runs `cargo test --workspace --locked --no-fail-fast`. Because that is a workspace-wide
run, the CO integration tests under `kiana-domain/tests/`, `kiana-core/tests/`,
`kiana-daemon/tests/` and `kiana-entrypoints/tests/` **are** discovered and **are** compiled and
executed by the unified gate. The per-card workflow files are not needed for that to happen.

So the practical consequence is narrower than it first looks, and the honesty point is: the
per-baseline "CI-only evidence" prose describes a *dedicated* per-card CI lane that was never
created. The evidence is real but arrives through `ci.yml`, not through the named file. A reader
who greps for `co42-company-recovery.yml` and finds nothing should conclude the prose is stale,
not that the tests are missing. This document is not the place to rewrite those baselines; it is the
place to record the discrepancy so the next agent fixes the prose or adds the workflows as a
deliberate choice.

Two workflows are named specifically in the `CURRENT_STATUS.md` CO-47 and CO-48 evidence blocks
and are both absent: `.github/workflows/co47-company-lifecycle.yml` and
`.github/workflows/co48-company-closeout.yml`. `scripts/company-os-business-smoke.sh` exists and is
referenced only from `CURRENT_STATUS.md`, `companyos.md` and the CO-47 guard — **no workflow invokes
it**, so the remote-only smoke lane described in that evidence block does not run anywhere today.

The CO-47 baseline has since been amended to say this itself: "not invoked by a dedicated workflow,
so the deny/recovery/effect-accounting path runs as part of the unified `ci.yml` workspace test
target rather than as its own lane." That is the correct characterisation, and it reaches the same
conclusion this document does. The remaining gap is that 41 pre-existing CompanyOS baselines still
promise a dedicated `.github/workflows/co*.yml` lane — 43 such paths are named across them and not
one exists — and the `CURRENT_STATUS.md` CO-47/CO-48 evidence blocks still name two workflows that
do not exist.

### 5.2 A CO-47 source guard asserts a marker the document does not contain

`kiana-core/tests/co47_company_lifecycle_guard.rs` ends with:

```rust
assert!(baseline.contains("feature_status=implemented"));
assert!(baseline.contains("proof_level=source"));
assert!(baseline.contains("limitations"));
```

`docs/roadmap/co47-company-lifecycle-baseline.md` contains `feature_status=implemented` and
`proof_level=source`, so the first two assertions hold. The third does not: the file has a
`## Limitations` **heading**, but Rust's `str::contains` is case-sensitive, and the lowercase
substring `limitations` appears nowhere in the document. Verified by literal case-sensitive
substring search.

Since `cargo test --workspace` in `ci.yml` compiles and runs `kiana-core`'s integration tests,
this assertion should fail the unified CI gate. I have not run the test — local test execution is
forbidden for this step — so this is a static finding, not an observed failure.

This is a defect in the guard, not in the CompanyOS runtime path, and the baseline it reads is
otherwise sound: the `## Limitations` section it now carries is accurate. The fix is a one-word
change in either direction (`"Limitations"`, or a lowercase word in the body). Both files belong to
the CO-47 owner; neither is this step's to edit.

Worth noting as a pattern rather than a one-off: this guard asserts on prose rather than on
behaviour, so it fails for reasons unrelated to the CompanyOS code. A guard that can only break
when a human edits a markdown heading is a weak gate.

### 5.3 The CompanyOS business chain is proven end-to-end at about two fixtures wide

The card family requires that cross-module behaviour be exercised "通过 `DaemonHost::handle` 或其
真实协议入口，不能只调用配置 helper" — that is, through the real protocol entry, not a
configuration helper. Checking every CO test target: the only ones that name `DaemonHost` at all
are `kiana-entrypoints/tests/co40_company_user_flow.rs` and
`kiana-entrypoints/tests/co41_company_surface_parity.rs`, and in both the token is a **string
literal inside a source-text assertion** (`"DaemonHost::local"`, `"DaemonHost"`), not a call.
Neither constructs a host nor calls `.handle()`.

The only tests that genuinely construct a `DaemonHost` and route a real envelope through
`host.handle(...)` are the two CompanyOS integration tests added by CO-47:
`kiana-daemon/tests/p3_i06_company_golden.rs` and `kiana-daemon/tests/company_lifecycle.rs`
(both via `DaemonHost::with_harness_and_project_authority` and `RequestEnvelope::company_command`).

Everything else is a contract-level fixture. Of the 38 cards CO-09..CO-46, 36 have a
`kiana-domain/tests/` fixture plus a `kiana-core/tests/` source guard; CO-40 and CO-41 are
entrypoint tests. All 38 exercise contract types directly, never the real protocol entry. That
is a legitimate and useful unit-test layer, and the per-card baselines say so; but the aggregate
effect is that **the CompanyOS business chain is source-covered end-to-end at roughly two fixtures
wide**, with the remainder proven at the type level. A reader must not take 38 baseline documents
as 38 end-to-end proofs.

Note also that both real-routing tests drive `kiana_runner::ScriptedModel` — a cassette. They are
`local_behavior`-class evidence for a fake model, not `live`.

## 6. Per-slice status

Statuses are taken from each card's own baseline and the matching `CURRENT_STATUS.md` evidence
block. Proof levels are the ledger's; this document does not raise any of them.

| Slice | feature_status | proof_level | What is proven | What is NOT proven |
|---|---|---|---|---|
| CO-01..CO-08 (scope, assignment, role catalog, policy, artifact, receipts, replay) | `implemented` (✅) | `source` | Typed contracts exist; CO-01/CO-08 have baselines. | Behavior on the product spine; the ✅ rows are bound to their own snapshots only. |
| CO-09..CO-46 (intake through template registry) | `partial` (🔄) | `source` | 38 domain contract slices, each with a baseline and a domain fixture. All 38 baselines literally state `proof_level: source`. | Runtime behavior. Per §5.3, none of the 38 reaches the real protocol entry; no restart, no live model, no external delivery. |
| CO-47 (fake-model lifecycle + fault matrix) | `implemented` | `source` | Two real `DaemonHost`-routed tests: golden closeout with a checked `OUTPUT.txt`, plus role/missing-project/idempotency-deny accounting. | A real model; a live provider; the smoke lane named in its evidence block (§5.1); its own guard currently fails (§5.2). |
| CO-48 (real-model closeout + handover) | `partial` (⏳) | `source` | `CompanyLiveCloseoutEvidence` binds provider/model/config/budget revisions, four role routes and receipts, artifact/usage/outcome, independent review, local delivery and `result_unknown`; a CI-indexing guard exists. | **Everything the card asks for.** No real model, account, credential, budget or approval. No external delivery confirmation, no outcome measurement, no cross-process projection, no physical/live handoff. |

The repo's own live evidence is real but narrow, and belongs to a different card: the
`CURRENT_STATUS.md` 2026-09-09 block records one DeepSeek run through its Anthropic-compatible
endpoint, one model, `read-only` sandbox, with `proof-level change: live for this exercised path`.
That is a `kiana run` receipt. **It is not a CompanyOS business loop**: it has no Objective, no
Charter, no Reviewer, no Delivery, no ClosingReceipt, and it is not a `CompanyLiveCloseoutEvidence`.
The card forbids using it as a substitute, and it does not substitute.

## 7. What the next agent must do first

Ordered. Each item names the honest ceiling it may claim if it completes.

1. **Unblock CI on the CO-47 guard (§5.2).** This is the only thing here that can turn a gate red
   today. It is a one-line fix in either the guard or the baseline.
2. **Decide the per-card CI workflow question (§5.1).** Either write the named workflows (each
   `workflow_dispatch`-only, registered in `scripts/ci/validate-workflows.sh`'s allowlist rules) or
   correct the 41 affected CompanyOS baselines to say the evidence arrives through unified
   `ci.yml`. Pick one and make the documents match. Do not leave the prose implying a lane that
   does not exist.
3. **Widen real `DaemonHost` routing (§5.3).** CO-47 proved two fixtures route correctly. The
   highest-value next fixture is the CO-48 card's own rejected-first scenario, run as a real
   `DaemonHost` test: an invalid proposal, a budget limit and an unapproved action must each stop
   the run with a structured refusal and zero broker/model effect. That converts a named-but-absent
   scenario into a real assertion, and it needs no live provider. It would be `local_behavior`.
4. **Only then pursue the live loop, and only with explicit authorization.** That needs: an
   operator approval reference matching the `approval:` prefix the contract requires, a real
   provider identity and concrete revision, a bounded budget, a temporary controlled project, and
   independent review of the generated code. Retain the raw result of the rejected-first scenario
   whichever way it goes. If a real model fails, **do not** substitute a cassette and keep the
   `live` label — record the failure.
5. **Do not promote any proof level without its receipt.** `durable` needs a real cross-process
   restart. `live` needs an independent external receipt. `physical` needs target-environment
   effect and cleanup proof. A green CI run, a transcript, a model self-report or this document
   supply none of these.

## 8. CI and limitations

### CI

No CI run was performed, awaited or claimed by this step, and no workflow file was added or
modified. `CURRENT_STATUS.md`, `docs/roadmap.md` and `docs/roadmap/step-execution-goal.md` were read
only; this step edits no product code. The next CI-relevant change is the §7.1 guard fix, and that
belongs to the CO-47 owner.

For completeness, and because a moving worktree is itself evidence about this step: a
`cargo check --workspace --tests --offline --keep-going` run during this step did **not** complete
cleanly — it reported 16 errors. Every one was in a file belonging to another agent working in
this same worktree at the same time (`kiana-core/src/bq26_fault_harness.rs`,
`kiana-domain/tests/bq25_telemetry_separation.rs`,
`kiana-eventlog/tests/pd30_storage_fault_matrix.rs`, `kiana-query/src/audit_projector.rs`), and the
count moved between runs as those files were edited. No error is in a file this step created or
modified, because this step created exactly one file and it is a markdown document. That is a
statement about attribution, not a green build: the workspace did not compile cleanly at the time of
observation, and this document asserts nothing that depends on it compiling.

### What this document does NOT prove

Stated plainly, because a handover that overstates its own evidence is worse than none:

- **It does not prove any runtime behavior.** It is prose over other people's source. Every claim
  it makes is a claim about what the source *says*, cross-referenced against evidence blocks.
- **It does not prove the CompanyOS business chain works.** No test, build, clippy, smoke or
  runtime command was executed for this step. The §5.2 and §5.3 findings are static source
  findings, not observed failures or observed passes.
- **It does not prove any real-model, live-provider, live-streaming, enterprise, installer,
  signed-package or dsh-Web-clone capability.** None of those exists in this checkout and none is
  claimed. The repository's product proof ceiling is `local_behavior`: trusted local repositories
  plus cassette/fake-script execution.
- **It does not promote any proof level.** Every CompanyOS slice remains at `source`. The
  `live` DeepSeek block belongs to a different card and a different evidence snapshot.
- **It does not certify the health of the rest of the roadmap.** The §5.1 workflow discrepancy is
  repo-wide (258 named-but-absent workflow paths), not CompanyOS-specific, and this document makes
  no claim about slices it did not read.
- **It cannot substitute for the live gate.** The CO-48 card's exit condition is a proven live
  business closed loop over a finite, authorized provider/model/task range. Until that exists with
  retained raw results, CO-48 stays `⏳` and this document stands as its handover, not its closure.

### Handover fields

Per [`dep41-operator-runbook.md` §4](dep41-operator-runbook.md):

```text
source_snapshot: CO-48 documentation-only step on the 2026-09-28 shared worktree; HEAD not recorded by
  this step (no git command was run per its constraints); all source claims above were read from the
  working tree, not from a pinned commit
worktree_status: docs/roadmap/co48-company-closeout-status-handover.md (new, this step); the pre-existing
  docs/roadmap/co48-company-closeout-baseline.md is unchanged; no code, manifest, workflow or frozen
  document was edited
command_argv: literal substring/grep/read inspection of CURRENT_STATUS.md, docs/roadmap.md,
  docs/roadmap/companyos.md, docs/roadmap/co*.md, kiana-domain/src/company*.rs,
  kiana-{domain,core,daemon,entrypoints}/tests/co*.rs, .github/workflows/, scripts/company-os-business-smoke.sh
  cargo check --workspace --tests --offline --keep-going
cwd·environment: repository root; Linux; no cargo test, no clippy, no fmt, no git mutation, no provider
  call, no network access, no smoke script
fixture·cassette: none executed; the fixtures in §4 are cited from source, not run
exit_code: no test command was run, so no test exit code exists. The workspace check exited 101 with
  16 errors, all attributed to concurrent agents' files; see §8 "CI" for the full attribution and
  the caveat that this is not a clean-build claim
status change: none. CO-48 remains partial/⏳; no roadmap, ledger or card status was edited
proof-level change: none. No slice was promoted; every CompanyOS slice remains proof_level=source
limitations: the five bullets in §8 above; the §5.2 guard finding is static and unconfirmed by a
  test run; the §5.3 routing finding counts source text and was not verified by execution
reviewer: Codex CO-48 documentation review; no independent runtime reviewer, no live reviewer, and
  no operator has reviewed or authorized any live action
```
