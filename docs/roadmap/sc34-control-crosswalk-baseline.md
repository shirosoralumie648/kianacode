# SC-34 security control registry and control crosswalk baseline

> Snapshot date: 2026-09-28. This slice owns the *decision* that a security control item is
> admissible to be claimed, and that the claim is no stronger than the evidence behind it. Local
> Cargo test/build commands are intentionally not run by the integration owner; GitHub Actions owns
> the fixtures and the workspace gate.
>
> **This is a documentation and source contract. It is not a regulatory certification.**

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-34`](security-compliance.md#step-sc-34) |
| code landing | `kiana-policy/src/security_control_registry.rs`, registered by `kiana-policy/src/lib.rs` |
| fixtures | `kiana-policy/tests/sc34_control_registry.rs` (40 tests, deny-first, success paths last), `kiana-policy/tests/sc34_control_registry_guard.rs` (source guard) |
| documents | [security-control-crosswalk.md](security-control-crosswalk.md) (the crosswalk body), this baseline |
| feature_status | `partial` — the registry can refuse an unsupported claim; nothing loads the crosswalk into it |
| proof_level | `source`; no `local_behavior` / `durable` / `live` / `physical` promotion |
| canonical path | supplied control draft → `SecurityControl::validate` → `SecurityControlRegistry::register` → `registry.validate` re-derivation |

The card's rejected-first column names two failures, and both are refusals about *language*
rather than about runtime behaviour:

- **"把单测或类型宣称为法规认证"** — a unit test or a type described as a regulatory
  certification. Refused by `security_control_proof_ceiling_exceeds_evidence`.
- **"控制与证据没有 owner"** — a control whose evidence has no owner. Refused by
  `security_control_owner_required`.

Neither of them is a runtime hazard, and neither can be demonstrated by running the system. That
is precisely why the refusal had to be modelled as data: a rule you can only check by reading a
paragraph is a rule that gets edited when someone is in a hurry.

## The three things that must never be substituted for each other

```text
framework clause   SEC-09 / GOVERN / LLM01 / INV-S09   -- what the standard says
control item       ctl-*                                -- what Kiana says it does about it
evidence           ControlEvidence { proves: ProofLevel } -- what was actually demonstrated
```

The whole module exists to keep these three apart. The load-bearing type is
[`ProofLevel`](security-compliance.md): `source < local_behavior < durable < live < physical`,
with the names taken verbatim from the `proof_level` vocabulary in `AGENTS.md` so that no
adjacent word like "mostly trusted" can be introduced as a rung.

`demonstrated_proof()` returns `Option<ProofLevel>`, and the `None` case is the design's most
important line. No evidence is `None`, **not** `Some(Source)`. Folding an empty evidence set
down to `Source` would let a control with nothing behind it claim the weakest rung of proof, and
`Source` is a rung — it is the rung that says "you have types and fixtures", which is exactly the
thing the card is worried about being over-claimed.

## Rejection paths, and how each one is refused

### 1. A pass claim with no evidence behind it

Three separate codes, deliberately not collapsed, because an operator needs to know which of the
three gaps they are looking at:

| Situation | Code |
|---|---|
| a `proof_ceiling` was declared and the evidence list is empty | `security_control_evidence_required` |
| `status` claims a pass (`partial`/`effective`) and no evidence exists at all | `security_control_status_not_derivable` |
| `status` claims `effective` but the strongest evidence is weaker than the ceiling | `security_control_proof_ceiling_exceeds_evidence` |

The ordering is the point: emptiness is tested *before* the generic text guard, so "nobody proved
this" and "the owner string is corrupt" stay distinguishable, and the ceiling comparison is
tested only once there is something to compare against.

`demonstrated_proof` takes a **maximum**, never a sum. Three `source` items do not add up to
`local_behavior`, and the fixture
`several_weak_pieces_of_evidence_do_not_add_up_to_a_stronger_claim` pins that.

### 2. Understating is allowed, overstating is not

`honest_status()` derives the verdict that the evidence actually supports: `Unknown` with no
evidence, `Effective` when the evidence reaches the ceiling, `Partial` otherwise. The registry
refuses a `status` that is *stronger* than that derivation.

It deliberately does **not** refuse a `status` that is *weaker*. A control whose evidence fully
backs its ceiling may still be published as `partial` while it waits on a human review; refusing
that would make the registry a place where conservatism is a bug, and conservatism is not a bug.

### 3. Unknown framework, unknown major, unknown clause

`SecurityFramework` is a closed four-value enum (`sec`, `nist`, `owasp`, `internal`); anything
else is `security_control_framework_unknown`. Each framework carries a
`supported_major()` and a `known_clauses()` allow-list:

- SEC-01 … SEC-12 (the constitution),
- GOVERN / MAP / MEASURE / MANAGE (the NIST AI RMF functions),
- LLM01 … LLM10 (OWASP LLM Top 10),
- INV-S01 … INV-S12 and SC-34 (Kiana internal).

`security_control_framework_major_unknown` fires when a record declares a major this build does
not accept; `security_control_framework_clause_unknown` fires when the clause is not on that
framework's list. Cross-framework near-misses are covered explicitly: `LLM01` is a real OWASP
identifier and is refused under `sec`, and `SEC-13` is one digit away from a real clause and is
refused too. `deny_unknown_fields` closes the same hole on the record itself, so a future field
cannot be silently dropped by an older reader.

### 4. Scope only narrows

`ControlScope` is a `BTreeSet<ControlScopeFacet>`, not prose, so "narrower" is computable rather
than a reviewer's impression. `derive_child` computes `parent ∩ requested` and discards anything
the parent does not cover; an empty intersection is
`security_control_scope_intersection_empty` rather than an empty-scope control.

The subset property is re-checked in two places, because a serialized record can be edited and
reloaded without ever passing through `derive_child`:

- `SecurityControlRegistry::register` → `security_control_scope_widened`
- `SecurityControlRegistry::validate` → the same code, so a registry built by any other route is
  still checked

An orphan (`security_control_parent_unknown`) and a self-parent
(`security_control_parent_self_reference`) are both refused, since a control that inherits from
nothing describes no inheritance at all.

### 5. `Unknown` stays `Unknown`

There is no code path from "we did not check" to "this passes". `honest_status` has an explicit
early return of `Unknown` for the empty-evidence case, there is no `unwrap_or_default`, and no
"missing evidence counts as satisfied" branch exists anywhere in the file. The registry-level
refusal for it is `security_control_status_not_derivable`.

### 6. Records cannot republish themselves

Every control carries a `control_digest` over its semantic fields, and the registry carries a
`registry_digest` over the sorted control ids and their digests. Both are recomputed and compared
on every validation, so a status flipped after construction (`security_control_digest_mismatch`)
and a control swapped into the table without recomputing the registry digest
(`security_control_registry_digest_mismatch`) are both caught. `ControlEvidence` is digest-bound
the same way.

### 7. Evidence is a reference, never a payload

`ControlEvidence` is `{evidence_ref, source_digest, proves, evidence_digest}`. The fixture
`an_evidence_reference_that_carries_a_secret_is_refused` shows why: a reference that needs the
value in it to be useful is a second copy of the secret. Text fields run through the same
`kiana_domain` helpers the SC-33 incident record uses — `safe_text` checks
`redact_text(value) != value` first and then `scan_secret_sentinels`.

The two secret fixtures use a URL-with-userinfo trigger (`https://alice:hunter2@...`) rather than
a bearer header on purpose: a bearer header is a *redaction* marker and would be caught by the
first check, yielding `_not_redacted`. Userinfo is sentinel-only, so it reaches the scanner and
proves the second check runs too. Both checks are load-bearing and the fixtures keep them distinct.

## Reason code inventory

Thirty-eight literal codes, all `security_control_*`, plus the `{field}_invalid` /
`{field}_not_redacted` / `{field}_secret_detected` triple produced per text field and per digest
field. The naming follows the crate's existing convention (`policy_*`, `project_trust_*`,
`security_incident_*`): stable snake_case, specific per defect, and never collapsed into a
generic `invalid` where an operator would lose the distinction between two different problems.

Every code is a `String` error, matching `kiana-policy`'s existing modules. That is a real
weakness — see the limitations — but changing the crate's error type is a shared-manifest-adjacent
decision that belongs to the integration owner, not to this slice.

## Why this module has no side effects

The module writes no file, opens no network connection, calls no port or adapter, appends no
event and starts no execution loop. It cannot sign an audit record, run a scanner, contact an
assessor, or make a compliance claim true by doing anything.

That constraint is the *point*, and it is what the source guard
`kiana-policy/tests/sc34_control_registry_guard.rs` pins: a control registry that could write or
dispatch would be able to manufacture the very evidence it exists to audit, and then "our ceiling
is `source`" would stop being true. The guard asserts both the presence of the contract markers
and the absence of `std::fs`, `std::net`, `std::process`, `Command`, `TcpStream`, `reqwest`,
`tokio`, `EventStore`, `append_event`, `ControlPlane`, `handle_command`, the five model-visible
tool names and the runner/broker types.

`kiana-policy` also does not depend on `kiana-core`, so the module structurally cannot reach the
control plane even if someone wanted it to.

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` gate, whose
`cargo test --workspace --locked --no-fail-fast` auto-discovers `kiana-policy/tests/*.rs`. No
separate workflow was added, per the CI consolidation rule that a new roadmap step must never
multiply automatic fan-out.

**This slice does not, and cannot:**

1. **Make a compliance claim.** It has no notion of an assessor, a control objective, a test
   period or a finding. It is a schema plus refusals.
2. **Load the crosswalk.** `docs/roadmap/security-control-crosswalk.md` is prose for humans.
   Nothing parses it into the registry, and no release gate reads either. SC-35 and SC-41 own that
   wiring; until they land, this is a reviewable claim set with nothing enforcing it.
3. **Verify the evidence it is handed.** `ControlEvidence` carries an opaque `source_digest`. The
   registry checks the digest's *format* and its *self-consistency*, not that the referenced fact
   exists, was committed, or says what the control claims. Resolving a reference to a committed
   EventLog fact is not implemented.
4. **Promote any proof level.** Every control in the crosswalk sits at `source` because
   `CURRENT_STATUS.md` records `proof_level=source` for the backing slices and the CI runs behind
   them were not awaited. This slice does not change that and cannot.
5. **Cover the framework itself.** The clause allow-lists are a transcription of SEC-01…SEC-12,
   the four NIST AI RMF functions, the OWASP LLM Top 10 and INV-S01…INV-S12 as they are written in
   this repository's own documents. They are **not** checked against the upstream standards, and
   they are not an authoritative rendering of them. A clause that the real NIST or OWASP renumbers
   will not be noticed here.
6. **Model control inheritance beyond one level.** `parent` is a single optional link and
   `register` requires the parent to be registered first, so a child of a child needs an explicit
   order. A full DAG with cycle detection across three or more levels is not implemented; only the
   self-reference case is caught.

## Known weaknesses in this slice, stated rather than hidden

- **`String` error codes.** A typo in a reason code is a compile-time-silent behaviour change. The
  crate's other modules share this shape, so the fix belongs to the whole crate, not here.
- **No `PartialEq`/`Ord` on the registry's iteration beyond `BTreeMap` determinism.** Fine today;
  worth revisiting if controls ever need a stable public ordering distinct from id ordering.
- **The clause lists are a transcription.** See limitation 5. This is the weakest link in the
  chain, because everything downstream trusts the list.
- **`honest_status` is advisory.** It is a method anyone can call and disagree with; the
  enforcement lives in `validate`, and a future caller could use the method as if it were the
  gate. The fixtures pin `validate`, not the method's callers.

## Evidence block

```text
source_snapshot: 8b1086a2 ("step: add the SC-33 incident, vulnerability and reconcile workflow
  contract") plus the SC-34 source slice; kiana-policy/src/security_control_registry.rs;
  kiana-policy/src/lib.rs; kiana-policy/tests/sc34_control_registry.rs;
  kiana-policy/tests/sc34_control_registry_guard.rs;
  docs/roadmap/security-control-crosswalk.md; docs/roadmap/sc34-control-crosswalk-baseline.md
worktree_status: isolated branch wt/sc34 in /home/shirosora/kiana-wt/sc34; the only tracked
  modification is the two-line module registration in kiana-policy/src/lib.rs; no other file in
  the repository is touched, and no pre-existing stash or unrelated WIP was read or altered
command_argv: cargo fmt -p kiana-policy (exit 0); cargo fmt -p kiana-policy --check (exit 0);
  cargo check -p kiana-policy --all-targets --locked --offline (exit 0);
  cargo clippy -p kiana-policy --all-targets --locked --offline (exit 0, no finding in the new
  files); bash scripts/ci/validate-doc-references.sh (exit 1, two PRE-EXISTING unresolved
  references in docs/roadmap.md and docs/roadmap/persistence-data-layer.md caused by reference/
  not being checked out in this worktree — the new documents resolved). No test, build, smoke or
  fixture was executed, per the explicit instruction that CI owns all test execution
cwd·environment: /home/shirosora/kiana-wt/sc34; Linux x86_64; offline cargo with --locked;
  no local runtime test reviewer
fixture·cassette: kiana-policy/tests/sc34_control_registry.rs holds 40 CI-only tests in
  deny-first order with the three success paths last: ownerless / punctuation-free owner,
  malformed owner, no-owner record forced through the registry, source evidence registered as
  durable, a one-step gap, evidence stacking, a ceiling with no evidence, an unevidenced pass,
  an unevidenced control deriving Unknown, weak evidence deriving Partial, a status field used as
  a conclusion, unknown framework, unknown major, cross-framework identifier, invented clause,
  SEC-13 near-miss, missing mapping, duplicate mapping, unknown record field, unknown status word,
  unknown proof word, empty scope, intersection derivation, disjoint child, hand-widened child,
  reloaded widened child, orphan parent, self-parent, unknown facet, tampered control digest,
  tampered registry digest, swapped control, duplicate id, duplicate evidence, secret-bearing
  evidence reference, self-republishing digest, and finally the three success paths (register plus
  derive, JSON round-trip, and registering an honestly-Unknown placeholder). The guard holds 4
  tests over markers, forbidden tokens and the lib.rs registration. No fixture was executed locally
exit_code: static checks only, as listed under command_argv; the remote fixture result and the
  workspace gate are unobserved
status change: the SC-34 source slice, its two fixtures, the crosswalk and this baseline are
  added on wt/sc34. docs/roadmap.md, CURRENT_STATUS.md, .github/workflows/*, Cargo.toml,
  Cargo.lock and package.json are deliberately untouched — the ledger row, the roadmap card marker
  and the status evidence block are the integration owner's to write
proof-level change: feature_status=partial (source-level admission, ownership, clause allow-list,
  proof-ceiling, scope-narrowing, unknown-preservation and digest-binding contracts);
  proof_level=source; no local_behavior, durable, live or physical promotion
limitations: this is a read-only contract over supplied control drafts. It appends no event,
  persists no control, resolves no evidence reference to a committed fact, contacts no assessor,
  enforces no release gate, loads no document, and promotes no proof level. The four framework
  clause lists are a transcription of this repository's own documents and are not checked against
  the upstream standards. Control inheritance is one level deep with no cycle detection beyond
  self-reference. Every control in the crosswalk sits at proof ceiling `source` because no backing
  slice has been observed above `source`, and this is a documentation and source contract — not a
  certification, attestation or audit result of any kind
reviewer: implementation review of the module's data model, the 38-code inventory, the deny-first
  ordering in `validate`, the max-not-sum evidence rule, the two-place scope-subset check, the
  redaction-versus-sentinel distinction, and the absence of any side effect or second execution
  path. Four fixtures with wrong expected codes were found and corrected by the author before
  delivery: a punctuation-only owner (not "missing"), a bearer header (redaction, not sentinel),
  a `ceiling > evidence` control asserted as honestly Partial (that shape is refused outright), and
  an `understated` expectation for a check that was deliberately not implemented. No local runtime
  or CI test reviewer
```
