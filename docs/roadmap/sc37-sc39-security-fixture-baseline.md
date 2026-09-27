# SC-37/SC-38/SC-39 security fixture suite baseline

## Scope

Three verification cards from the security programme's Wave G, delivered as one source slice.
They are FIXTURE suites: they compile and are exercised by GitHub CI. They do not add runtime
enforcement, do not dispatch a capability, and do not promote any card's proof level beyond
`source`.

| Card | What is added | Proof ceiling |
|---|---|---|
| SC-37 | `Sc37DenyCase` / `Sc37DenyMatrix`: a deny-first matrix keyed by refusal family, with an explicit per-surface dispatch budget that must be zero | source (recorded fixture observation) |
| SC-38 | `Sc38PropertyCase` / `Sc38PropertyCorpus`: a seeded, clock-free property/fuzz/serialization/replay corpus with no `Allowed` outcome | source (deterministic offline corpus) |
| SC-39 | `Sc39AttackCase` / `Sc39AttackCorpus`: a red-team/eval attack corpus that stores digests only and never promotes injected text to policy | source (offline attack corpus) |

The core insight all three share: a denial is only a denial if nothing downstream ran. Every
contract therefore carries a dispatch budget (`handler`/`broker`/`provider`/`adapter`, or the
SC-37/SC-39 `handler_calls` + `provider_calls` + `effect_count` triple) and a non-zero budget is
rejected by `validate()`. `Sc37DenyCase::with_dispatch` exists so the guard can prove the
contract refuses a "deny" that still dispatched; it must never be used to describe a passing case.

## Source slice

- `kiana-domain/src/security_fixture_contracts.rs` defines all three corpora plus
  `ScFixtureCorrelation`, the request/run/operation/cursor binding they share. Correlation is
  identity and position only; it confers no admission.
- `kiana-domain/tests/sc37_sc39_security_fixtures.rs` covers each card's "先拒绝" column with one
  named test per rejection, plus the negative tests that prove a forged digest, a non-zero
  dispatch, a duplicate case name, an uncovered family, a seed mismatch, a replay divergence or
  an injected-as-product section is refused.
- `kiana-core/tests/sc37_deny_matrix.rs` drives the real deny paths (`SecurityContext::from_server`,
  `AuthorityFence::validate_current`/`successor`, `plan_deletion`, `EntrypointParityMatrix`,
  `scan_secret_sentinels`) and records the reason each one produced.
- `kiana-core/tests/sc38_property_replay.rs` generates payloads with a fixed SplitMix64 stream
  derived from an explicit seed — no clock, no unseeded RNG — and asserts refusal, dedup,
  cursor ordering and unknown-major rejection.
- `kiana-core/tests/sc39_red_team_corpus.rs` runs synthetic attacks through `PromptBundle`,
  `SecurityContext`, `CapabilityCatalog::from_manifests`, `RouteAttestation::verify`,
  `plan_deletion` and `EntrypointParityMatrix`.
- `kiana-core/tests/sc37_sc39_security_fixture_guard.rs` pins the source markers and greps the
  registration in `kiana-domain/src/lib.rs`.

## Failure-first fixture matrix

### SC-37 — 每类越权、重放、泄露、TOCTOU、删除、入口绕过

| Card rejection | Named test | Observed reason | Dispatch budget |
|---|---|---|---|
| 越权 forged role | `sc37_privilege_escalation_forged_role_is_denied_with_zero_dispatch` | `AUTH_ROLE_MISMATCH` | 0/0/0/0 |
| 越权 untrusted project | `sc37_privilege_escalation_untrusted_project_is_denied_with_zero_dispatch` | `AUTH_CALLER_UNTRUSTED` | 0/0/0/0 |
| 重放 unbound permit | `sc37_replay_unbound_permit_is_denied_with_zero_dispatch` | `dispatch_permit_decode_failed` → `POLICY_APPROVAL_BINDING_MISMATCH` | 0 |
| 重放 stale fence | `sc37_replay_stale_authority_fence_is_denied_with_zero_dispatch` | `POLICY_AUTHORITY_EPOCH_ROLLBACK` | 0 |
| 泄露 secret in receipt | `sc37_leakage_secret_sentinel_in_receipt_text_is_denied_with_zero_dispatch` | `SECRET_REDACTION_FAILED` | 0 |
| 泄露 secret in prompt | `sc37_leakage_secret_in_prompt_channel_is_denied_with_zero_dispatch` | `SECRET_REDACTION_FAILED` | 0 |
| TOCTOU fence drift | `sc37_toctou_fence_scope_drift_is_denied_with_zero_dispatch` | `AUTH_SESSION_GENERATION_STALE` | 0 |
| TOCTOU expired fence | `sc37_toctou_expired_fence_is_denied_with_zero_dispatch` | `UNKNOWN_FENCE_EXPIRED` | 0 |
| 删除 legal hold | `sc37_deletion_legal_hold_is_denied_with_zero_dispatch` | `deletion_legal_hold_active` | 0 |
| 删除 retention unknown | `sc37_deletion_retention_unknown_is_denied_with_zero_dispatch` | `deletion_retention_unknown` | 0 |
| 删除 stale epoch | `sc37_deletion_stale_data_epoch_is_denied_with_zero_dispatch` | `deletion_data_epoch_stale` | 0 |
| 入口绕过 direct route | `sc37_entrypoint_bypass_direct_route_and_cross_entrypoint_replay_are_rejected` | decode refused | 0 |
| 入口绕过 denied+handler call | `sc37_entrypoint_bypass_denied_command_with_handler_call_is_rejected` | `entrypoint_parity_denied_handler_effect` | refused, not sealed |

### SC-38 — 随机 payload、截断事件、重复 frame、乱序 cursor、unknown major

| Card rejection | Named test | Observed reason |
|---|---|---|
| 随机 payload | `sc38_random_payloads_never_decode_into_a_dispatchable_value` | decode refused, `UNKNOWN_UNCLASSIFIED` |
| 截断事件 | `sc38_truncated_frames_never_partially_accept` | decode refused, `FACT_SCHEMA_UNKNOWN_MAJOR` |
| 重复 frame | `sc38_duplicate_frames_deduplicate_instead_of_re_effecting` | `POLICY_APPROVAL_BINDING_MISMATCH`, outcome `deduplicated` |
| 乱序 cursor | `sc38_out_of_order_and_gapped_cursors_are_refused` | `FACT_FENCE_MISMATCH` / `authority_fence_sequence_rollback` |
| unknown major | `sc38_unknown_major_versions_are_refused_not_upgraded` | `incompatible schema version ... unknown major must fail-closed` |

Determinism is itself a tested property: `sc38_corpus_is_deterministic_across_rebuilds_and_order_replays`
rebuilds the corpus from the same seed and case order and asserts a byte-identical digest, then
replays it reversed and asserts the reversed corpus is still internally consistent.

### SC-39 — prompt injection、间接注入、secret exfil、恶意插件/MCP 描述

| Card rejection | Named test | Observed reason | Outcome |
|---|---|---|---|
| prompt injection | `sc39_prompt_injection_cannot_become_product_authority` | injection stays a Context section | `denied` |
| prompt injection role forgery | `sc39_prompt_injection_role_forgery_is_denied_at_the_control_plane` | `AUTH_ROLE_MISMATCH` | `denied` |
| 间接注入 | `sc39_indirect_injection_from_repository_text_cannot_widen_a_grant` | `extension_read_only_write_denied` | `denied` |
| 间接注入 delete recipe | `sc39_indirect_injection_through_a_deletion_recipe_is_denied` | `deletion_legal_hold_active` | `denied` |
| secret exfil | `sc39_secret_exfiltration_cannot_leave_the_broker` | all 9 scan channels refuse | `denied` |
| secret exfil userinfo | `sc39_secret_exfiltration_via_a_url_userinfo_is_also_masked` | userinfo masked before scan | `denied` |
| 恶意插件 | `sc39_malicious_plugin_manifest_cannot_self_authorize` | `capability_intersection_missing:…` | `denied` |
| 恶意 MCP 描述 | `sc39_malicious_mcp_description_is_denied_before_dispatch` | `route_attestation_prompt_pack_untrusted` | `denied` |
| 恶意 MCP 描述 unverified | `sc39_malicious_mcp_description_with_unverified_provider_is_unknown_not_allowed` | `route_attestation_provider_unverified` | `unknown` |
| 恶意 MCP 描述 scope widening | `sc39_malicious_mcp_description_cannot_approve_its_own_scope_widening` | handler count stays 0 | `requires_approval` |

The corpus stores only `attack_text_digest`, so the fixture itself carries no working payload and
no secret; the attack strings are literal synthetic markers, not exploits.

## CI

- `kiana-core/tests/sc37_sc39_security_fixture_guard.rs` and the three fixture files are compiled
  and run by the workspace test targets. Formatting is checked with `rustfmt --edition 2021` on the
  touched files only; `cargo fmt --all` is not run because several agents share this worktree.
- The guard asserts the module and re-export lines exist in `kiana-domain/src/lib.rs`, so a
  registered-but-dead slice fails the guard rather than passing silently.

## Limitations — what this slice does NOT prove

- **Not runtime enforcement.** The dispatch budgets are contract fields on recorded fixtures. They
  are not measurements taken from a live Broker, provider or adapter. A real zero-effect proof
  needs a counting adapter in the composition root, which this slice does not add.
- **Not a live red-team evaluation.** Every attack is synthetic and offline. No model, provider,
  plugin, MCP server, network or filesystem was executed. Adaptive attacks, multi-turn attacks and
  tool-result poisoning chains are out of scope.
- **Not a fuzzing campaign.** SC-38 uses a fixed SplitMix64 stream over bounded lengths. There is
  no libFuzzer/AFL harness, no coverage feedback and no crash corpus triage. It is a deterministic
  property fixture, not continuous fuzzing.
- **Not durable or cross-process.** The corpora are immutable values with digests. Nothing here
  proves EventLog append-only behaviour across a restart, nor replay determinism of a real ledger.
- **Not an external upcaster.** SC-38 asserts the registered `SchemaVersion` compatibility rule
  (`major` must match) and that unknown majors fail closed. It does not implement or test a
  version migration/upcaster.
- **Not compliance certification.** A passing deny-first matrix is a source-level control
  statement, not GDPR/HIPAA/SOC 2/ISO 27001 evidence.
- **No authorization granted by correlation.** `ScFixtureCorrelation` names a request, run,
  operation and cursor. It is never consulted by admission and confers no authority.
- **Deliberately no `Allowed` outcome.** `Sc38Outcome` has no allow variant and
  `Sc39ExpectedOutcome` has no allow variant. This encodes deny-first, but it also means these
  corpora cannot express a legitimate allow path, so a future allow-path regression would not be
  caught here.

```text
feature_status: implemented (fixture suites)
proof_level: source
evidence: kiana-domain/tests/sc37_sc39_security_fixtures.rs,
          kiana-core/tests/sc37_deny_matrix.rs,
          kiana-core/tests/sc38_property_replay.rs,
          kiana-core/tests/sc39_red_team_corpus.rs,
          kiana-core/tests/sc37_sc39_security_fixture_guard.rs
local_execution: not run — the task forbids local `cargo test`; compile/lint only
reviewer: unassigned — no local runtime test reviewer
```

## Registration owed by the integration owner

`kiana-domain/src/lib.rs`:

```text
mod security_fixture_contracts;          // after `mod security_contracts;` (line 263)

pub use security_fixture_contracts::*;   // after `pub use security_contracts::*;` (line 558)
```

No `SchemaContract` entry is required: the three corpora are fixture/evidence schemas over
already-registered domain types (`SecurityReasonCode`, `SchemaVersion`), not new wire or runtime
event schemas. If the integration owner wants them version-tracked anyway, these are the entries:

```text
SchemaContract {
    name: "kiana.sc37-deny-matrix.v1",
    version: SchemaVersion::new(1, 0),
    layer: SchemaLayer::Projection,
    owner_crate: "kiana-domain",
    compatibility: CompatibilityPolicy::BackwardCompatible,
    allow_unknown_fields: false,
},
SchemaContract {
    name: "kiana.sc38-property-corpus.v1",
    version: SchemaVersion::new(1, 0),
    layer: SchemaLayer::Projection,
    owner_crate: "kiana-domain",
    compatibility: CompatibilityPolicy::BackwardCompatible,
    allow_unknown_fields: false,
},
SchemaContract {
    name: "kiana.sc39-attack-corpus.v1",
    version: SchemaVersion::new(1, 0),
    layer: SchemaLayer::Projection,
    owner_crate: "kiana-domain",
    compatibility: CompatibilityPolicy::BackwardCompatible,
    allow_unknown_fields: false,
},
```
