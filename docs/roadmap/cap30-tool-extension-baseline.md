# CAP-30 dynamic tool search and controlled extension admission baseline

> Snapshot date: 2026-09-28. This slice adds the *admission decision* half of CAP-30. Local Cargo
> test/build/check/clippy/smoke commands are intentionally not run; GitHub Actions owns fixtures
> and affected-target checks.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`CAP-30`](capability.md#step-cap-30) |
| source snapshot | master plus this CAP-30 discovery-admission slice |
| feature_status | `partial` for the source-level admission reducer |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |
| canonical path | server-owned candidate -> fixed-order admission -> admitted schemas only |

CAP-30 asked for a filter-then-search pipeline: filter by owner/role/project/trust/health/version,
then search by exact/BM25/tag ranking, then expand only the selected schemas under a context-token
budget. The search half already existed (`search_tool_schemas` / `search_tool_catalog` in
`kiana-domain/src/tool_catalog.rs`, wired to the `tool.search` executor in
`kiana-daemon/src/execution_control.rs`). What did not exist was the admission decision over an
extension- or server-supplied descriptor, and that is what this slice adds in
`kiana-domain/src/tool_discovery.rs`.

The reducer is a pure function of a `ToolDiscoveryPlan` and a list of `ToolDiscoveryCandidate`
values. It creates no catalog, loads no package, verifies no signature, starts no broker, calls no
port and appends no event. Every claim in the report is a claim about supplied facts.

## What the card rejects, and how

| Rejected | How |
|---|---|
| an invisible or untrusted descriptor reaching the model | `ToolDiscoveryTrust` must be `Verified`; `Unverified` and `Denied` produce `SourceUntrusted` and the descriptor is never admitted, not merely field-filtered |
| a descriptor from a degraded or stale source | `ToolDiscoveryHealth` must be `Healthy`; `Degraded` and `Stale` produce `SourceUnhealthy`, because a partial catalog served during an upgrade is indistinguishable from a deliberate removal |
| a descriptor registered against another catalog version | `ToolDiscoveryPlan::new` seals the current `ToolCatalogSnapshot`, and a candidate whose `catalog_version` differs produces `CatalogVersionStale` |
| a tool bound to a role the server did not bind it for | `bound_roles` is server-set; a plan whose `role_id` is absent produces `RoleNotBound` |
| an extension self-granting a capability | the reducer never appends to a role allow-list. `shadow_builtin_descriptor` refuses any candidate that names an existing `ToolSpec` or claims an operation a built-in tool already owns (`BuiltinBindingConflict`) |
| an extension replacing a built-in binding | the same rule applies to a renamed tool claiming a built-in operation, and to a second candidate claiming an already-admitted operation (`DuplicateToolName`) |
| a read-only extension acquiring a write tool | `ExtensionEffect::ReadOnly` with any of the five effect-bearing capabilities produces `ReadOnlyWriteDenied`; the descriptor is the untrusted side of this boundary, so its own claim is the evidence |
| a signed manifest understating its own scope | `check_manifest_effect` refuses a `ReadOnly` manifest whose `required_capabilities` contain a write capability (`tool_discovery_manifest_read_only_write_scope`), so the descriptor never exists to be filtered |
| a read-only plan admitting a read-write contract | `effect_ceiling` is a ceiling on the contract, not only on the tool; a read-write candidate under a read-only plan produces `EffectInsufficient` |
| a report presented as another pass's answer | `validate_against` re-derives the whole decision and re-binds the plan digest, so an edited status or admitted list is refused (`tool_discovery_report_binding_invalid`) |

The decision order is fixed so the same facts always produce the same report: plan validity,
then structural collisions (`BuiltinBindingConflict`, `DuplicateToolName`), then trust and
identity (`SourceUntrusted`, `CatalogVersionStale`, `SourceUnhealthy`, `ProjectMismatch`,
`RoleNotBound`), then the effect ceiling. Structural collisions outrank trust deliberately: a
descriptor that both shadows a built-in binding and claims to be read-only is reported as the
binding takeover it is, not softened into a permission problem.

## Failure-first fixture matrix

`kiana-domain/tests/cap30_tool_discovery.rs` — one test per rejected-first item, plus a
decision-order test.

| Fixture | Assertion |
|---|---|
| `tool_search_never_returns_invisible_or_untrusted_descriptors` | untrusted, unbound, degraded, stale and wrong-catalog-version descriptors are all denied and never admitted; the existing search executor still returns a versioned, bounded, non-granting response |
| `extension_cannot_self_grant_or_replace_builtin_binding` | a built-in name, a renamed tool claiming a built-in operation, an unbound role and a duplicate operation are each denied; a read-only manifest declaring write scope is refused |
| `read_only_extension_write_is_denied_at_executor` | all five effect-bearing capabilities and their prefixed spellings are denied under a read-only contract; a read-only plan refuses a read-write contract; an honest read-only candidate is admitted and still grants nothing |
| `decision_order_is_fixed_and_a_tampered_report_is_rejected` | a doubly-violating candidate reports the structural conflict; a tampered or reflip report fails `validate_against`; a budget too small to fit the admitted set refuses admission |

## CI and limitations

GitHub Actions runs the unified `.github/workflows/ci.yml` workspace gate. No separate workflow.

**This does not prove that an extension is present, signed, trusted or bound.** The reducer is a
decision over values a caller supplies; it cannot tell a truthful adapter from a lying one. If the
daemon constructs a `ToolDiscoveryCandidate` with `trust: Verified` for an unverified package, the
reducer will admit it. The trust verdict is only as good as the admission path that produced it,
and that path is the existing `kiana-daemon/src/extensions.rs` signature/package verification,
not this module.

Three card items are explicitly not established here. First, **BM25 and tag ranking are not
implemented**: `search_tool_schemas` still uses the exact/prefix/token/description scoring it
always used, and the `tags` field on a candidate is carried and validated but not scored. Second,
**extension descriptor indexing** is not implemented: candidates are supplied as a list rather
than drawn from a persistent index, and the reducer is O(n) over that list. Third, **no live
read-only extension and approved side-effect extension are exercised end to end**; the fixtures
are decision fixtures, and the five-tool legacy profile / cassette regression the card asks for is
unchanged and not re-run here.

The context-token accounting is a `bytes / 4` heuristic over the selected schemas, matching
`search_tool_catalog`. It is not a provider usage receipt and is not a context-window measurement.

The four `ExtensionVisibility*` enums in `kiana-domain/src/extension_visibility.rs` already model
trust, status, risk and kind for the extension registry. This module does not replace them: it adds
`ToolDiscoveryTrust`/`ToolDiscoveryHealth`/`ToolDiscoveryOrigin` because a *descriptor* arriving at
the search boundary has different admission questions from a *registry entry* arriving at an
operator listing. The mapping between the two vocabularies is not asserted by any test in this
slice and is recorded as follow-up.

## Registration

`kiana-domain/src/lib.rs` must gain four lines for this module to be part of the crate:

```text
mod backend_selection;        # not needed for CAP-30; see the CAP-31/32 baseline
```

For CAP-30 specifically:

```text
mod tool_discovery;                 # after `mod text_normalization;`
pub use tool_discovery::*;          # after `pub use tool_catalog::*;`
```
