# CAP-30 dynamic tool discovery and controlled extension admission baseline (broker boundary)

> Snapshot date: 2026-09-28. This slice adds the *promotion* half of CAP-30 at the capability
> broker boundary. Local Cargo test/build/smoke commands are intentionally not run; GitHub Actions
> owns the fixtures and the workspace gate. `cargo check`, `cargo fmt` and `cargo clippy` for
> `kiana-capability-broker` were run and pass.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`CAP-30`](capability.md#step-cap-30) |
| code landing | `kiana-capability-broker/src/discovery.rs`, registered by `kiana-capability-broker/src/lib.rs` |
| fixtures | `kiana-capability-broker/tests/cap30_dynamic_discovery.rs` (22 tests, deny-first, both success paths last), `kiana-capability-broker/tests/cap30_dynamic_discovery_guard.rs` (8 source guards) |
| feature_status | `partial` for the source-level promotion reducer |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | search result -> `DynamicDiscoveryReport::evaluate` -> `promotable()` -> caller builds a `CapabilityRequest` through the existing ControlPlane path |

## Why this is a second landing and not a duplicate of the catalog reducer

CAP-30 already has a landed half: [`cap30-tool-extension-baseline.md`](cap30-tool-extension-baseline.md)
covers `kiana-domain/src/tool_discovery.rs`, which decides whether a descriptor may become
**model-visible in a search result at all**. The search half itself predates both
(`search_tool_schemas` / `search_tool_catalog` in `kiana-domain/src/tool_catalog.rs`, wired to the
`tool.search` executor in `kiana-daemon/src/execution_control.rs`).

Neither of those answers the question this slice owns. A descriptor that was legitimately visible in
a catalog thirty seconds ago is still just a string. Something has to decide, at the moment a
discovery result is about to be turned into work, whether **this** result may be promoted under
**this** grant. That is a different question, it needs different inputs (a live grant, a revocation
epoch, a current effect ceiling), and putting it in the catalog reducer would have meant threading
grant state into a module that deliberately knows nothing about grants. The two are complementary,
and the vocabulary is kept disjoint on purpose:

| | catalog reducer (`kiana-domain`) | this slice (`kiana-capability-broker`) |
|---|---|---|
| question | may this descriptor be *searchable*? | may this *result* be promoted, now? |
| inputs | catalog, role binding, trust, health | grant, authority epoch, effect ceiling, revocation, catalog |
| vocabulary | `ToolDiscovery*` | `Discovery*` |
| on refusal | the descriptor is not admitted to the catalog | the candidate is not promotable |

## The data flow

```text
  tool.search (already a brokered operation, ControlPlane-authorized like any other)
      |
      v
  server-owned DiscoverySurface            <- the parent authority
      principal / project / authority_epoch
      granted_capabilities                 <- capability keys, may be EMPTY
      ScopeSet{operations, namespaces}     <- the authorization surface
      effect_ceiling                       <- ReadOnly or ReadWrite
      catalog: operation -> RegisteredDiscoveryTool { namespace, schema_digest,
                                                       capability, effect, side_effecting }
      catalog_digest / revoked / expires_at_unix_ms / digest   <- digest is sealed
      |
      v
  DynamicDiscoveryRequest                 <- bound to the surface digest it was issued under
      request_id / query / surface_digest / issued_at / max_results
      |
      v
  DiscoverySearchOutcome                   <- THE UNTRUSTED SIDE
      Completed(Vec<DiscoveredToolCandidate>)   every field is a claim
      |   Failed { reason }                      the search did not happen
      v
  DynamicDiscoveryReport::evaluate(request, surface, evidence, outcome, now)
      |
      |-- 1. request shape?          no -> Denied  discovery_request_invalid
      |-- 2. surface + evidence ok?  no -> Denied  discovery_surface_invalid
      |-- 3. surface_digest bound?   no -> Denied  discovery_authorization_stale
      |-- 4. revoked?                     -> Denied  discovery_authorization_revoked
      |-- 5. now >= expires?               -> Denied  discovery_authorization_expired
      |-- 6. outcome?
      |        Failed                 -> UNKNOWN  discovery_search_failed, retry_required
      |        Completed(candidates)   -> per candidate, first failing check wins:
      |             validate -> seen/dup -> catalog lookup -> namespace -> schema
      |             -> evidence -> ProjectTrust -> provenance -> manifest digest
      |             -> adapter status -> project -> capability claim -> capability granted
      |             -> operation in scope -> permission union -> effect claim -> effect ceiling
      v
  DynamicDiscoveryReport { status, admitted, denials, retry_required, digest }
      |
      v
  promotable()   -- empty unless status == Admitted; the ONLY way out
      |
      v
  caller builds CapabilityRequest -> ControlPlane::handle_command -> policy/gates/approval
      -> CapabilityBrokerPort::execute -> ExtensionAdmission -> handler
```

The last two arrows are **not** in this slice and are not reachable from it. The module builds no
`CapabilityRequest` and holds no `CapabilityBrokerPort`; the guard fixture asserts both absences by
source marker. Promotion is a candidate, and a candidate still has to walk the ordinary authorized
path.

## What the card rejects, and how

| Rejected | How | Code |
|---|---|---|
| dynamic search bypassing static authorization | the surface's `granted_capabilities` + `ScopeSet` are consulted per candidate; a tool the search found but the grant does not carry is refused | `discovery_capability_not_granted`, `discovery_operation_out_of_scope`, `discovery_unknown_tool` |
| a cached result outliving its grant | the request is bound to `surface_digest`; revocation, narrowing, an epoch bump or a new catalog all move the digest | `discovery_authorization_stale` |
| a revoked grant readmitted by a re-sealed digest | `revoked` is checked separately from the digest, so re-sealing around a revocation does not help | `discovery_authorization_revoked` |
| extension self-granting more than the parent | manifest scope ∪ descriptor claim, then ∩ grant; a non-empty difference is a **refusal**, never a narrowed grant | `extension_scope_superset_denied` |
| a project-local extension loaded without trust | `project_local` requires `ProjectTrustVerdict::Trusted`; `Unknown` and `Untrusted` are both refused | `discovery_project_trust_unverified` |
| supply-chain drift | the descriptor's `observed_manifest_digest` must equal the registry's `registered_manifest_digest`; drift in *either* direction is the same refusal | `extension_manifest_digest_mismatch` |
| a descriptor that describes its own provenance | only `PublisherSigned` and `RepositoryBundled` are verifiable roots | `discovery_provenance_unverified` |
| a descriptor escalating its own effect | effect is the *weaker* of the registry entry and the manifest; a descriptor claiming more is refused before the surface ceiling is even consulted | `discovery_effect_claim_escalated` |
| a read-only pass admitting a read-write tool | the surface's `effect_ceiling` is checked against the server-owned effect, not the claim | `discovery_effect_ceiling_exceeded` |
| unknown tool name / namespace / unregistered schema | all three fail closed against the registry, never against the descriptor | `discovery_unknown_tool`, `discovery_unknown_namespace`, `discovery_schema_unregistered` |
| search failure degrading to allow-all or deny-all | a failed search is `DiscoveryStatus::Unknown` with `retry_required`, an empty admitted list, and a pinned single denial code | `discovery_search_failed` |
| one result smuggling two descriptors for one operation | the second claim of an `(namespace, operation)` pair is refused; admitted identity comes from the registry, not the claim | `discovery_duplicate_candidate` |
| a report edited after the fact | `validate_against` re-derives the whole decision and re-binds every field | `discovery_report_binding_invalid`, `discovery_report_digest_mismatch` |

## The two invariants that make the rest structural rather than advisory

**A search result is a candidate, not an authorization.** `DynamicDiscoveryReport::promotable` is
the only accessor for admitted candidates and returns `&[]` unless the status is `Admitted`. An
`Unknown` report therefore cannot be read as "allow everything" and a `Denied` report cannot be read
as "deny everything, carry on" — both yield the same empty set, and the `Unknown` report
additionally sets `retry_required` so the caller knows a re-run is mandatory. An empty
*successful* search is a different, final answer (`Denied`, no retry demanded), and
`discovery_request_invalid` is what an oversized result produces rather than a silent truncation.

**Intersection, never union.** `intersect_extension_scope(requested, granted)` is the only way a
capability set enters an admitted record. The guard asserts the module contains no
`granted.union`, no `granted_capabilities.union` and no `.extend(`, and asserts the reference rule
in `kiana-policy/src/grant_scope.rs` still uses `intersect` / `contains` with
`grant_scope_capability_intersection_empty`. A superset request is refused whole: a descriptor that
asked for too much is not trusted with a smaller grant on a retry.

## Decision order is part of the contract

`DiscoveryDenial::precedence` is a total order over all 23 codes, and
`every_denial_code_is_stable_snake_case_and_precedence_is_a_total_order` pins it. The groups run:
input cannot be answered -> the answer is stale before it is a permission question -> the search did
not happen -> the descriptor is not a real registered tool -> its supply chain is not trustworthy ->
it asks for something this grant does not have. A candidate that violates several rules reports the
*earliest* one, so a descriptor that both escalates its effect and claims an ungranted capability
is reported as the effect escalation it is rather than softened into a permission problem, and an
untrusted descriptor that is also out of scope is reported as untrusted rather than as a scope
question.

## One deliberate asymmetry, and why

`DiscoverySurface::allows_namespace` treats an absent namespace allow-list as **deny all**, while
`ScopeSet::allows_operation` treats `NotApplicable` as unrestricted. This is intentional and
documented in the method: an unrestricted *namespace* list is precisely the bypass this card
forbids, and a discovery surface must name the namespaces it offers. A grant that never names a
namespace offers no dynamic tool discovery, which is the safe reading. The fixture
`unknown_tool_namespace_and_schema_all_fail_closed` covers both directions.

## Failure-first fixture matrix

`kiana-capability-broker/tests/cap30_dynamic_discovery.rs` — one test per rejected-first item, in
decision order, with the two success paths last.

| Fixture | Assertion |
|---|---|
| `cached_result_cannot_outlive_the_grant_it_was_issued_under` | a revoked+re-sealed surface and a narrowed grant both refuse the cached result as stale, name no candidate, and demand no retry |
| `revoked_grant_denies_even_when_the_caller_resealed_the_digest` | the explicit `revoked` flag carries the refusal on its own when the request is bound to the revoked surface |
| `expired_grant_is_refused_at_the_expiry_instant` | one millisecond before expiry admits; at the expiry instant the same inputs are refused |
| `failed_search_is_unknown_and_promotes_nothing` | a failed search is `Unknown` + `retry_required` + zero candidates, an empty successful search is `Denied` + no retry, and the two are not the same answer |
| `malformed_request_and_corrupt_inputs_are_refused_not_answered` | bad surface digest, blank query, zero `max_results`, a re-sealed surface and a corrupt evidence record are all refused rather than answered |
| `unknown_tool_namespace_and_schema_all_fail_closed` | unregistered operation, unlisted namespace, namespace that disagrees with the registry, drifted schema, and a surface that names no namespaces at all |
| `a_candidate_outside_the_authorization_surface_is_refused` | the same candidate is admitted on a surface that offers it and refused on one that does not, which is what makes the refusal about the grant rather than the tool |
| `a_capability_the_grant_does_not_carry_is_refused` | a renamed capability is a claim mismatch; a registry that agrees still meets a grant that lacks it |
| `an_operation_outside_the_grant_scope_is_refused` | registry and granted capability agree while the scope dimension does not list the operation |
| `untrusted_provenance_and_untrusted_project_are_refused_before_the_manifest_is_compared` | `Unknown`/`Untrusted` project trust, self-asserted and absent provenance; the same candidate is admitted once both facts are in order |
| `manifest_digest_drift_is_supply_chain_tampering_not_a_cache_miss` | drift on the descriptor side and on the registry side produce the same refusal |
| `missing_evidence_and_a_non_available_adapter_are_refused` | unknown component, `RequiresApproval` / `NotSupported` / `Denied` statuses, and evidence issued for another project |
| `an_extension_asking_for_more_than_the_parent_grant_is_refused_not_narrowed` | a greedy manifest and a greedy descriptor are both refused whole, and the intersection helper is asserted non-amplifying directly |
| `a_descriptor_cannot_escalate_its_own_effect` | claim escalation, a loud manifest against a read-only registry, the read-only ceiling, and the same read-write extension admitted under a read-write ceiling |
| `duplicate_operations_are_refused_so_one_result_cannot_smuggle_two_descriptors` | the second claim is refused and exactly one descriptor survives; when the first is also refused the report is a flat `Denied` |
| `an_oversized_result_is_refused_rather_than_truncated` | 200 candidates over the 128 bound are refused, not silently cut |
| `decision_order_is_fixed_and_a_tampered_report_is_refused` | a triply-violating descriptor reports the earliest rule; trust outranks authorization; an unsealed edit, a re-sealed forgery, a smuggled candidate, a re-flagged retry and a relabelled `Unknown` are each refused with their own code |
| `every_denial_code_is_stable_snake_case_and_precedence_is_a_total_order` | 23 unique snake_case codes, 23 unique precedences spanning 0..=22, and only `discovery_search_failed` is `Unknown` |
| `namespaces_are_derived_and_capability_keys_are_canonical` | namespaces come from the operation, `Other(..)` cannot collide with a built-in capability key, evidence keys are well formed |
| `an_honest_read_only_extension_within_the_grant_is_admitted_as_a_candidate_only` | identity comes from the registry, the carried scope is a subset of the grant, and the report binds surface, catalog and epoch |
| `several_honest_candidates_are_admitted_together_and_a_bad_one_does_not_poison_them` | a drifted entry between two honest ones is refused while both neighbours are admitted |
| `the_five_tool_model_surface_is_untouched_by_any_of_this` | no admitted candidate is named `shell`, `apply_patch`, `mcp`, `memory.search` or `memory.write` |

`kiana-capability-broker/tests/cap30_dynamic_discovery_guard.rs` — shape, not decisions.

| Fixture | Assertion |
|---|---|
| `the_discovery_contract_is_registered_and_reachable_from_the_crate_root` | `mod discovery;` and `pub use discovery::*;` exist, and exactly those two lines mention the module in the crate root, so the slice cannot be wired into dispatch |
| `the_refusal_vocabulary_is_present_in_source` | all 23 codes are present in `discovery.rs` |
| `the_non_amplifying_primitives_are_present` | the intersection, the surface-digest comparison, the `Unknown` status, the fail-closed namespace arm, the registry-sourced effect and the re-derivation |
| `the_search_result_is_a_candidate_and_not_an_authorization` | `promotable` is status-gated; the module names no `CapabilityRequest` and no `execute` |
| `the_slice_has_no_side_effect_tokens` | 22 effect tokens and 6 control-flow tokens are absent from the **code** (comments are stripped first, so prose is neither rewarded nor punished) |
| `the_frozen_five_tool_model_surface_is_untouched` | the five names and the existing fail-closed fixture are still in `kiana-runner/src/tools.rs`; `discovery.rs` names no tool-minting token and no legacy registry crate |
| `the_permission_union_rule_stays_an_intersection_not_a_union` | `kiana-policy/src/grant_scope.rs` still uses `intersect`/`contains`; `discovery.rs` contains no widening expression |
| `the_module_is_self_describing_about_its_proof_ceiling` | the doc still says "read-only contract", "candidate, not an authorization", "adds no sixth model-visible tool" and "It is a decision over values a caller" |

## Honest limitations

**This does not prove that any extension exists, is signed, is trusted, or is bound.** Every value
the reducer judges is supplied by a caller. If a caller builds a `DiscoverySurface` that lists an
unsigned package as `PublisherSigned` with a matching digest, this module will admit its candidates.
The trust verdict is only as good as the admission path that produced the
`ExtensionAdmissionEvidence`, and that path is `kiana-daemon/src/extensions.rs`
signature/package verification plus the existing `ExtensionAdmission::check` recheck at dispatch —
neither of which this slice touches. The module is a decision, not a verifier.

**No search is implemented or exercised.** `DiscoverySearchOutcome` is an input. The `tool.search`
executor in `kiana-daemon/src/execution_control.rs` and the ranking in
`kiana-domain/src/tool_catalog.rs` are unchanged; BM25 and tag ranking remain unimplemented, exactly
as the sibling baseline records. No real MCP server, no real plugin installation and no real package
were involved.

**No live extension, and no five-tool cassette regression.** The card asks for a legitimate read-only
extension first and an approved side-effecting extension second, end to end. Both are decision
fixtures over supplied values. The legacy five-tool profile/cassette set is unchanged and was not
re-run here.

**Two capability vocabularies are not reconciled.** `ExtensionAdmissionEvidence::required_capabilities`
and the surface's `granted_capabilities` are both expressed as `capability_key` strings
(`query`, `process`, `other.<name>`). Manifest-side capability naming
(`mcp.declarative.stdio`, `capability.adapter`, from `ExtensionComponentKind::capability_name`) is a
*different* vocabulary, and nothing in this slice maps between the two. A caller that mixes them
would get a false `extension_scope_superset_denied`. Recorded as follow-up.

**`DiscoveryProvenance` and `ProjectTrustVerdict` are not the project's own enums.** `ProjectTrust`
lives in `kiana-types`, which is on the compatibility boundary and is not a dependency of
`kiana-capability-broker`; `kiana-domain` exposes no equivalent. The two local enums mirror the
three-state trust and the verifiable-root distinction the rest of the system uses, but the mapping
is asserted by no test in either direction. The four `ExtensionVisibility*` enums in
`kiana-domain/src/extension_visibility.rs` and the `ToolDiscovery*` enums in
`kiana-domain/src/tool_discovery.rs` are likewise not reconciled with these.

**The promotion step itself is unwired.** Nothing in the workspace calls
`DynamicDiscoveryReport::evaluate` yet. A caller must invoke it, read `promotable()`, and then build
a `CapabilityRequest` that goes through `ControlPlane::handle_command` like any other. Wiring that
is a separate slice and needs its own decision about where in the command surface it belongs.

**`ExtensionAdapterStatus::RequiresApproval` is refused, not deferred.** A component in that state
gets `discovery_adapter_not_available` here. That is the fail-closed reading — a search result must
not wave through something that needs approval — but it means an approved side-effecting extension
needs a follow-up decision about how approval and discovery compose. It is not a claim that
approval is impossible.

## CI

GitHub Actions runs the unified `.github/workflows/ci.yml` gate, which auto-discovers
`kiana-capability-broker/tests/*.rs`, and also runs `cargo fmt --all --check` and
`cargo clippy --workspace --all-targets --locked`. No separate workflow was added, per the CI
consolidation rule recorded in `scripts/ci/doc-reference-exemptions.txt`.

Commands run locally for this slice (all exit 0):

```text
cargo check  -p kiana-capability-broker --all-targets --locked --offline
cargo fmt    -p kiana-capability-broker -- --check
cargo clippy -p kiana-capability-broker --all-targets --locked --offline
```

`cargo test` was **not** run, by instruction. The fixtures are therefore unproven at
`local_behavior`; they are `source` claims about what the code says, and only CI can promote them.
