# CO-46 Company template registry and research-report baseline

## Scope

CO-46 adds a versioned CompanyOS registry for process templates, role-pack metadata and the
existing `PolicyProfile` authority reference. A template version freezes role order, required
artifacts, required gates, output contract and policy-profile digest. Coding and research-report
templates therefore share the same governance shape while keeping different output contracts; the
research path is analyst/reviewer/closer and ends in a local report delivery rather than Builder
source changes.

The registry permits one initial active install. Every later version is a configuration proposal
with a current-version/hash snapshot, explicit approval and acceptance references, and an optional
explicit migration requirement. Active processes keep their original `ActiveTemplatePin` until a
separate migration call. Rollback changes only the active default pointer and records a rollback;
it does not delete template history, proposals, process pins or business facts.

Prompt `allowed-tools` annotations, untrusted packages and hidden arbitrary scripts are rejected as
authority sources. The domain adapter does not load packages, run scripts, call a model, dispatch a
capability or replace ControlPlane/Broker policy.

## Implemented source slice

- `CompanyTemplateVersion`, `CompanyRolePackVersion`, `CompanyTemplateConfigProposal` and
  `CompanyTemplateRollbackRecord` use strict serde, canonical digests and fail-closed references.
- `CompanyTemplateRegistry` keeps role packs, policy profiles, template history, active pointers,
  process pins, proposals and rollback records. Upgrade approval refuses active-process authority
  changes without explicit migration and requires both approval and acceptance references.
- `coding_v1` and `research_report_v1` demonstrate the shared governance contract and distinct
  output contracts; research has no Builder role and requires independent check/local report gates.
- Core exposes only a pure validation/recording adapter. Existing CompanyProcess, PolicyProfile,
  ControlPlane and Broker paths remain the authority and effect boundaries.

## CI-only evidence

`.github/workflows/co46-company-template-registry.yml` runs formatting, the domain version/upgrade/
migration/rollback/research fixtures, the Core source guard and domain/core test-target compilation
on GitHub Actions. Local Cargo tests, builds, checks, clippy and smoke scripts were not run and CI
is not awaited.

```text
feature_status: partial
proof_level: source
```

## Failure-first fixture matrix

| Fixture | Assertion |
|---|---|
| `template_upgrade_requires_acceptance_and_explicit_migration_for_active_processes` | stale authority is rejected; approval without migration/acceptance fails; pin changes only after explicit migration |
| `coding_and_research_templates_share_policy_but_keep_distinct_outputs` | both templates validate under one policy digest; research has no Builder and requires independent-check/local-report contracts |
| `prompt_tools_untrusted_packages_and_hidden_scripts_cannot_form_authority` | prompt annotations and untrusted/hidden sources cannot become execution authority |
| `direct_second_template_install_requires_a_config_proposal` | an installed template cannot be silently replaced by a second version |

## Limitations and handoff

- This is a pure registry and contract slice; durable EventStore materialization, organization
  config storage, scheduler migration execution and live research evidence remain later work.
- Role-pack metadata is not a mutable permission grant; the registry does not hot-load prompts or
  provider packages and does not claim that a report was researched, checked or delivered.
- CI results are intentionally not awaited; proof level remains `source`.
