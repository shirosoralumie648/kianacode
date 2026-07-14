# Project Research Summary

**Project:** Kiana
**Domain:** Local-first Open Core AI Agent platform for Coding, Academic Research, Daily Work, official cloud, and enterprise self-hosting
**Researched:** 2026-07-14
**Confidence:** MEDIUM-HIGH overall; HIGH for approved scope, current Rust boundaries, and dependency order; MEDIUM for time-sensitive client versions, proprietary product behavior, and hosted operations

## Executive Summary

Kiana 1.0 is one complete AI Agent product, not a Coding CLI with optional future product lines. The approved boundary includes a local-first `Kiana Core`, Coding, Academic Research, and Daily Work packs, CLI/TUI, Headless SDK/RPC/MCP, IDE, Desktop, Web/App Server, an optional official cloud, and enterprise self-hosting. Coding receives the most development capacity and can mature earlier in Alpha/Beta, but Research, Daily, every product surface, cloud, and enterprise remain 1.0 requirements. Phase order manages dependencies; it does not reduce scope.

The recommended strategy is a brownfield evolution of the current Rust workspace into one ports-and-adapters execution system. Freeze typed contracts first, establish event logs as the only durable authorities, centralize deterministic policy, extract the reusable runtime, and then harden evidence-first workflows, recovery, side-effect safety, and bounded multi-agent execution. Only after those semantics are stable should domain packs, thin product surfaces, remote workers, cloud control planes, and enterprise adapters scale out. Desktop, Web, IDE, cloud, or a pack must never introduce its own model loop, session store, permission semantics, workflow state machine, or completion definition.

The primary product risk is completion theater and state divergence, not lack of feature ideas. Kiana must distinguish source existence, local proof, target-environment proof, and user-value proof; a schema, module, mock, local smoke, or passing test count cannot stand in for a real journey, signed release, hosted tenant boundary, or customer acceptance. The roadmap therefore pairs every capability with evidence, retains `result_unknown` for uncertain external writes, keeps Research claims tied to provenance, and treats the 38-reference `Adopt / Adapt / Reject` ledger as continuous governance rather than a one-time inventory.

## Scope And Evidence Guardrails

### Non-Negotiable 1.0 Boundary

- **Core:** one runtime, session, provider, tool, policy, context, memory, workflow, evidence, recovery, extension, and usage model.
- **Packs:** Coding, Academic Research, and Daily Work all reach formal release quality; Coding priority means more parallel capacity, not exclusive scope.
- **Surfaces:** CLI/TUI, Headless SDK/RPC/MCP, IDE, Desktop, and Web/App Server consume the same commands, IDs, events, state, approvals, and evidence.
- **Delivery forms:** no-account Local Personal, explicitly enabled Official Cloud, and Enterprise Self-hosted all remain required for 1.0.
- **Platforms:** native Linux/macOS and Windows through WSL are the approved first-release boundary; native Windows is post-1.0.
- **Release language:** Alpha/Beta may expose partial vertical slices with an explicit support matrix, but `1.0`, `complete`, and `production-ready` remain blocked until every approved gate passes.

### Proof Ladder

| Proof level | What it proves | Required artifacts | What it cannot prove |
|---|---|---|---|
| **1. Source existence** | A mechanism or contract is present and attributable | Live source path, schema, reference snapshot, license/provenance, owner | That it runs, is safe, or serves a user |
| **2. Local proof** | Behavior works in controlled local conditions | Contract/unit/integration tests, deterministic fixtures, local smoke, replay/recovery evidence | Target OS packaging, real provider/connector behavior, hosted isolation, or customer value |
| **3. Target proof** | The shipped shape works in its intended environment | Installed artifact, real Linux/macOS/WSL journey, live provider/connector receipt, hosted tenant negative tests, enterprise deployment/rollback evidence | That the end-to-end workflow solves the intended user problem reliably |
| **4. User-value proof** | A real user journey meets acceptance with inspectable evidence | Versioned golden task, target-user or owner acceptance, quality/cost/reliability result, supportable release artifact | Future upstream parity or untested adjacent journeys |

Every requirement and reference capability records its highest achieved proof level and the missing next level. No lower level may be reported as a higher level. `local_blocking=0` is not equivalent to `external_blocking=0`, and neither alone establishes user value.

### 38-Reference Governance

The feature research contains a live 38/38 reference inventory. Roadmap work must preserve one row per immediate `reference/` directory with snapshot/source path, license, mechanism, `Adopt / Adapt / Behavior-only / Reject`, Kiana owner, contract, test, risk, and evidence. A new or changed reference triggers a live re-audit. Proprietary, non-commercial, unclear-license, archived, maintenance-mode, mock-heavy, or restored-source projects may inform observable behavior but cannot silently contribute production code or completion claims.

## Key Findings

### Recommended Stack

Keep the current Rust 2021 modular core and extend it deliberately. The strongest existing assets are Tokio, Serde/JSON, Axum, Reqwest with Rustls, typed `RuntimeEvent` contracts, append-only workflow EventLogs, CLI/TUI delivery, and the existing release-proof scripts. The first stack work should make the declared dependency policy real: pin Rust 1.96.0 for this milestone, centralize workspace dependencies, remove unexplained duplicate terminal/WebSocket lines, correct version declarations, and pin CI/release tools.

Add storage, client, identity, observability, and hosted infrastructure behind stable ports rather than replacing the core. SQLx with bundled SQLite is the recommended local projection/query layer; PostgreSQL plus object storage supports hosted tenancy and leases while EventLogs remain authoritative. One generated TypeScript client workspace should serve React/Vite Web UI, Tauri Desktop, and IDE clients. OIDC/OAuth, OS Keychain or server vault, OpenTelemetry with redaction, pinned SBOM/provenance tools, and real platform packaging tests complete the production stack.

**Core technology decisions:**

- **Rust 1.96.0, edition 2021, Tokio:** retain the privileged runtime and all safety-critical semantics in the existing workspace.
- **Serde JSON plus committed JSON Schema:** preserve browser-, MCP-, SDK-, and migration-friendly public contracts; generate client types and detect drift.
- **Axum/Tower and Reqwest/Rustls:** use one local/hosted service foundation and one governed network boundary.
- **Append-only EventLogs plus rebuildable projections:** keep session/workflow facts separate from query indexes and UI stores.
- **SQLx + bundled SQLite / PostgreSQL:** provide local projections and hosted tenancy without becoming a second workflow authority.
- **React + Vite + TypeScript, Tauri 2:** provide shared operational UI and native Linux/macOS packaging while keeping privileged behavior in Rust.
- **OIDC/OAuth, Keychain/vault, envelope encryption:** avoid custom authentication, plaintext fallbacks, and custom cryptography.
- **OpenTelemetry plus existing `tracing`:** expose redacted operational signals, never workflow truth or raw user content.

**Version confidence limit:** frontend and client versions in [STACK.md](STACK.md), including Node, pnpm, TypeScript, React, Vite, Tauri, Playwright, and related packages, were registry-checked on 2026-07-14 but are time-sensitive and not yet integrated. Revalidate the compatible set during the first client spike and pin exact versions and lockfiles. PostgreSQL, object-store service, KMS, Kubernetes, and signing-tool deployment versions remain intentionally unselected until their deployment phases can test upgrade, rollback, backup, and offline behavior.

### Expected Features

**Must have for 1.0:**

- **Core reliability:** typed events and IDs, durable session/workflow state, provider capability negotiation, one tool/connector registry, deterministic config and policy, provenance-aware context/memory, evidence/verifier, crash recovery, idempotency, `result_unknown`, bounded multi-agent, extension lifecycle, usage/audit, local data lifecycle, and release proof.
- **Coding:** a dated Claude Code public-behavior ledger; repo exploration; controlled context; safe edit/diff/undo; shell, Git, test/debug/review; MCP/extensions; agents/teams; browser/computer-use/notebook/LSP; Headless/CI; background/remote; voice fallback; and cross-surface Coding continuity.
- **Academic Research:** research question and protocol; literature/data/code discovery; legal acquisition; PDF/OCR and citation management; evidence graph; synthesis; versioned datasets and experiments; statistics and chart provenance; paper workspace; reproducibility/submission package; EDA/hardware/robotics extensions; and a Research-specific integrity verifier.
- **Daily Work:** local information objects; calendar, mail, messaging, office artifacts, and meetings; connector center; browser/desktop automation; project workflows; approval inbox; external receipts and reconciliation; workspace search; reporting; and team collaboration.
- **Surfaces:** terminal, automation protocols, VS Code and JetBrains IDE experiences, Desktop workspace, Web/App Server, consistent degraded/error/accessibility states, platform behavior, diagnostics, and cross-surface continuity.
- **Commercial delivery:** opt-in encrypted cloud sync, remote workers, teams, billing and operations; enterprise install/offline bundle, SSO/RBAC, managed policy, vault/KMS, audit, restricted-network operation, retention/DR, observability, licensing, documentation, and support acceptance.

**Competitive requirements that are also 1.0 gates:**

- Evidence-first completion and inspectable acceptance.
- Durable recovery across crashes, disconnects, workers, and surfaces.
- Safe reconciliation instead of replaying uncertain external side effects.
- Bounded multi-agent execution with isolation and integration proof.
- Full local product use without an account.
- One state model across three packs and every surface.
- Visible provider capability routing/degradation.
- Provenance-aware context, memory, and Research claims.
- User-selectable autonomy inside immutable hard boundaries.
- Audited reference coverage and transparent release readiness.

**Defer beyond 1.0 only:** native Windows, native mobile clients, multi-region active-active and extreme-scale fleets, P2P sync, marketplace revenue sharing, user model training, ambient always-on voice/video, additional high-risk professional packs, custom EDA solvers/automated manufacturing orders, unbounded agent societies, decorative 3D workspaces, and approval-free high-risk actions. None of these deferrals permits moving an approved pack, surface, cloud, or enterprise capability out of 1.0.

### Architecture Approach

Use a ports-and-adapters modular monolith around a single `RuntimeHost` and append-only state model. Extract `kiana-state`, `kiana-policy`, and `kiana-runtime` at proven seams; retain `kiana-types`, `kiana-tasks`, `kiana-services`, `kiana-tools`, `kiana-query`, and `kiana-skills` as the owning low-level components. Domain packs contribute tools, workflow templates, object schemas, verifiers, evaluation fixtures, and presentation hints through a stable manifest. Cloud and enterprise are composition roots and adapters over the same events, commands, policy, evidence, and recovery semantics.

**Major components:**

1. **`kiana-types` contract layer** - typed IDs, events, statuses, commands, errors, provider capabilities, policy inputs/decisions, and pack manifests.
2. **`kiana-state`** - session repositories, generic event-store ports, local compatibility adapters, projections, indexes, migrations, and concurrency control.
3. **`kiana-policy`** - pure deterministic merge/evaluation for trust, autonomy, path, network, side-effect, workspace, and organization policy.
4. **`kiana-runtime` / `RuntimeHost`** - turn lifecycle, provider plan, context snapshot, tool loop, workflow attachment, event publication, retry budget, cancellation, and local/daemon/remote host selection.
5. **`kiana-tasks`** - workflow DAG, WorkPacket, evidence, verifier, integrity, review, recovery, lease, and side-effect facts.
6. **Capability layer** - tools, providers/services, query/context, skills/plugins/hooks/MCP/connectors, each behind governed contracts.
7. **Domain packs** - Coding adapter plus Research and Daily packs that cannot fork core state or policy.
8. **Thin surfaces and hosted composition** - terminal, automation, IDE, Desktop, Web, remote workers, cloud control plane, and enterprise adapters.

### Dependency Chain

```text
live baseline + proof ledger + pinned toolchain
  -> typed/versioned contracts
  -> authoritative session/workflow state + rebuildable projections
  -> deterministic policy, secrets, trust, and local data boundaries
  -> reusable runtime + explicit provider capability plans
  -> reliable workflow/evidence/recovery/result_unknown
  -> bounded multi-agent + extension/domain-pack contracts
  -> Coding / Research / Daily vertical slices
  -> terminal/headless/IDE/Desktop/Web continuity
  -> official cloud identity/sync/workers/teams/billing/ops
  -> enterprise deployment/SSO/RBAC/vault/audit/DR/ops
  -> 38-reference, platform, security, supply-chain, and user-value proof
```

### Critical Pitfalls

1. **Multiple writable authorities** - session, task, workflow, memory, UI, and cloud state diverge. Prevent this by making event append the commit point and every other representation rebuildable.
2. **Contract drift across Rust, schemas, transports, and clients** - freeze versioned typed contracts, generate client types, retain compatibility adapters, and run golden replay at every surface phase.
3. **Runtime and policy logic continuing to grow in entrypoint mega-files** - extract one tested seam at a time; do not combine runtime rewrite, packs, UI, and cloud in one change.
4. **Provider equivalence theater** - record dated capabilities and route, visibly degrade, name emulation, or reject; never silently claim unsupported tool, vision, structure, reasoning, or context behavior.
5. **Blind retry of external writes** - persist attempt, target, idempotency key, approval, receipt, and verification query; uncertain dispatch becomes `result_unknown` until reconciled.
6. **Autonomy, extensions, remote workers, or tenant layers bypassing policy** - use one fail-closed evaluator, monotonic organization denies, external ProjectTrust storage, short-lived scoped worker credentials, and negative bypass tests.
7. **Unbounded or weakly isolated multi-agent execution** - persist WorkPackets before launch, enforce leases/scopes/locks, quarantine late results, and rerun whole-workflow verification.
8. **Evidence theater** - track source, local, target, and user-value proof separately; withdraw completion language when a higher proof level is absent.
9. **Research evidence contamination** - bind claims to source/artifact/hash and experiments to data split, environment, seed, code revision, and raw result; unverified downstream claims become blocked.
10. **Desktop/browser/connectors becoming privileged backdoors** - require capability manifests, origin/target binding, narrow native commands, approval, receipts, redaction, and prompt-injection tests.
11. **Cloud eroding local-first or tenant checks stopping at handlers** - keep Local Personal complete, derive tenant scope from authenticated server context, and enforce it in repositories, rows, objects, queues, vaults, workers, and streams.
12. **Reference, platform, and supply-chain proof drifting** - re-audit live sources/licenses and test actual signed install/upgrade/rollback artifacts on Linux, macOS, and WSL.

## Implications For Roadmap

The following structure matches `granularity=fine`. Phase numbers express dependency gates. Coding, Research, and Daily workstreams may run in parallel after Phase 9, with the most capacity assigned to Coding, but all three must converge before final surface and release acceptance.

| Phase | Focus and rationale | Concrete delivery and exit evidence | Feature families / risks addressed | Research mode |
|---|---|---|---|---|
| **1. Live Baseline And Proof Governance** | Establish trustworthy scope before moving behavior | Snapshot dirty/live baseline; create requirement-to-proof ledger; preserve 38/38 reference rows; freeze dated Claude public-behavior inventory method; define proof-level reporting | COD-01, DIF-11/12, AF-01/02/03; prevents reference and evidence theater | **Deep** for dynamic proprietary baseline |
| **2. Reproducible Toolchain And Dependency Convergence** | Make every later migration reproducible | Pin Rust/CI/release tools; centralize workspace dependencies; remove or justify duplicates; lock release inputs; retain serial gate while adding isolated process tests | CORE-16; supply-chain and cross-platform drift | Standard repository work |
| **3. Contract And Schema Baseline** | All downstream state and clients depend on stable semantics | Golden tests for RuntimeEvent, session, workflow, evidence, policy, provider, MCP, remote/bridge, CLI JSON, and app-server schemas; typed IDs/statuses/errors/durability/idempotency; compatibility adapters | CORE-01/03/04; wire drift | Standard, repo-grounded |
| **4. State Authority And Projection Recovery** | Remove split-brain before new writers appear | Add `kiana-state`; move session repository behavior behind ports; preserve legacy read/migration; make workflow/session logs authoritative; prove projection deletion/rebuild, migration, export, rollback; add SQLite only behind replay tests | CORE-02/08/15; multiple authorities, DB projection drift | Targeted SQLx migration spike |
| **5. Policy, Trust, Secrets, And Local Data Boundary** | Side effects and extensions cannot expand safely without one decision model | Add pure `kiana-policy`; typed execution/policy snapshots; autonomy profiles; ProjectTrust; deny precedence; path/network/sandbox/side-effect decisions; Keychain/vault capability states; local backup/export/delete | CORE-05/06/15; policy bypass, plaintext fallback | Security review plus platform capability research |
| **6. Runtime Extraction And RuntimeHost** | Create the reusable execution core before packs or clients depend on entrypoints | Extract provider/tool loop incrementally from `runner.rs`; introduce `InProcessHost`; retain CLI/SDK wrappers; prove event, cancellation, retry, context snapshot, and behavior parity after every slice | CORE-01-04/07; mega-file and runtime-per-surface risks | Standard, repo-grounded |
| **7. Provider Capability And Adapter Closure** | A stable runtime must make multi-provider differences explicit | Version capability snapshots and `CapabilityPlan`; first-class Anthropic, OpenAI, Gemini, OpenRouter, OpenAI-compatible, and local adapters; visible route/degrade/reject; standard contract suites; model-pinned live eval lane | CORE-03/14, DIF-07, AF-14; silent fallback | **Deep** for proprietary/live behavior |
| **8. Reliable Workflow, Evidence, And Side-Effect Semantics** | Build the differentiating completion loop before domain breadth | Link durable turns to WorkflowRun; acceptance, evidence, verifier statuses, idempotent command acknowledgements, durable approvals, receipts, `result_unknown`, compensation/reconciliation, crash and corrupt-projection recovery | CORE-09-11, DIF-01-03; blind retry and false completion | Standard patterns plus connector fault fixtures |
| **9. Bounded Multi-Agent, Extensions, And Pack Contract** | Parallel/domain work needs enforceable scope and lifecycle | WorkPacket/ResultPacket, lease, late-result quarantine, path/resource isolation, whole-run integration gate; signed/provenanced skill/plugin/hook/MCP/connector lifecycle; `DomainPackManifest` and registries | CORE-12/13, DIF-04/09; unbounded agents and extension bypass | Standard architecture, targeted signature review |
| **10. Coding Public Baseline And Repository Journey** | Coding is the highest-investment first pack and validates core depth | Dated parity ledger; init/project resources; repo map/search/context; safe edit/diff/checkpoint/undo; shell; format/lint/build/test/debug/review on versioned real repositories | COD-01-09/16; checklist parity and Git data loss | **Deep** for Claude baseline and multi-language golden set |
| **11. Coding Ecosystem, Automation, And Remote Journey** | Close public Coding breadth on the reliable core | MCP/extensions, agents/teams, browser/computer-use/notebook/LSP, Git/PR/CI receipts, Headless/SDK, background/remote, voice fallback, and IDE/Desktop/Web Coding acceptance | COD-10-15, DIF-04/06; real service and platform gaps | **Deep** for public behavior, remote services, and voice/platform behavior |
| **12. Research Sources, Citations, And Evidence Graph** | Research writing is unsafe until source identity and claims are trustworthy | Research protocol; legal search/download outcomes; PDF/Web/OCR parsing; DOI/arXiv/metadata validation; citation import/export; claim/counter-evidence graph; provenance-aware synthesis | RES-01-06, DIF-08/10, AF-09/11 | **Deep** for scholarly connectors, licensing, parser quality, and integrity attacks |
| **13. Research Experiments, Paper, Reproduction, And Domain Packs** | Claims need reproducible data and experiment evidence before publication artifacts | Dataset/code intake; isolated notebook/interpreter; experiment manifests; statistical checks; baseline/ablation/error analysis; paper workspace; reproduction/submission bundle; EDA/hardware/robotics pack contracts and human sign-off | RES-07-14, AF-17; experiment and statistical contamination | **Deep** with research-domain experts and adversarial evals |
| **14. Daily Objects, Connectors, And Approval Control** | Daily actions require identity/scope/preview before automation | Notes/tasks/files; calendar, mail, messaging, office artifacts, and meeting schemas; connector center with OAuth scopes/health/revoke; approval inbox; cross-source ACL/freshness | DAY-01-05/08-10; secrets, privacy, authorization | **Deep** for connector APIs, OAuth, office formats, and privacy |
| **15. Daily Automation, Workflow, Receipts, And Team Handoff** | External mutation is valid only with recovery and target proof | Browser/Desktop RPA with target identity, checkpoint, compensation, timeout/cancel; mixed-pack project workflows; connector-specific verifier; receipt and unknown-result journeys; shared handoff objects | DAY-06/07/11/12, DIF-03/06; privileged automation and duplicate writes | **Deep** for Desktop/RPA behavior and prompt-injection testing |
| **16. Terminal, Headless, And MCP Product Closure** | Existing surfaces are the earliest full consumers of the stable core | Complete CLI/REPL/TUI for three packs; versioned SDK/RPC streams, backpressure/reconnect/approval; MCP tools/resources/templates/prompts and transport interoperability; doctor/readiness | SURF-01-03/10; surface-owned state and protocol drift | Targeted external interoperability research |
| **17. IDE Clients** | Coding journeys need editor-native context without a second runtime | Ship VS Code and JetBrains clients for context, chat, diff, diagnostics, review, workflow state, approvals, and resume over generated contracts; prove version mismatch and workspace-trust behavior | SURF-04/07/08; IDE-only session/policy forks | **Deep** for current IDE APIs, packaging, and marketplace rules |
| **18. Desktop And Web/App Server** | The approved workspace and remote/team surfaces share one client contract but different privilege boundaries | Validate and pin the TypeScript/React/Vite/Tauri stack; Desktop conversation/workspace/artifacts/connectors/computer-use/approvals; Web workflow/team/admin/ops; bounded pagination/reconnect; narrow Tauri commands | SURF-05/06/08/10, DAY-09/10; privileged bridge and UI store authority | **Deep** for client versions, Tauri security, accessibility, and browser support |
| **19. Cross-Surface Local Continuity And Platform Delivery** | Multiple shells become one product only when state and recovery agree | Optional local daemon; CLI-create/IDE-diff/Desktop-approve/Web-observe journey; cursor resync and conflict behavior; Linux/macOS native and Windows/WSL path, sandbox, secret, browser, install/update/rollback tests | SURF-07-09, COD-14, DIF-06; disconnect, platform, and packaging drift | **Deep** for macOS/WSL target behavior and native packaging |
| **20. Official Cloud Identity, Sync, And Tenant Data Foundation** | Remote value depends on identity and safe data movement first | Account/MFA/device/recovery/delete; opt-in encrypted sync, scopes, conflicts, tombstones, export/delete; PostgreSQL tenant model, RLS/repository negatives, object storage, key rotation, backup/restore | CLOUD-01/02/05/07; local-first erosion and tenant leakage | **Deep** for threat model, identity, residency, and storage operations |
| **21. Official Cloud Workers, Teams, Billing, And Operations** | Commercial cloud becomes usable only when execution and operations share the verified semantics | Signed scoped worker leases, isolated execution, reconnect/cancel/budget, verifier return; team spaces and approvals; usage/entitlement/invoice ledger; SLO, queue, incident, abuse, support, and restore evidence | CLOUD-03/04/06/08, COD-14; remote second authority and operational gaps | **Deep** for provider economics, billing, abuse, capacity, and production evidence |
| **22. Enterprise Deployment, Identity, And Central Governance** | Enterprise data features have no meaning before install and actor boundaries exist | Online/air-gapped bundle, preflight, capacity, migration, upgrade/rollback; OIDC/SAML, MFA, break-glass; RBAC; monotonic managed policy; vault/HSM/KMS and restricted-network adapters | ENT-01-04/06/09; handler-only policy and floating infrastructure | **Deep** for target environments, IdPs, licensing, and offline dependencies |
| **23. Enterprise Audit, Data Lifecycle, DR, Operations, And Support** | Production self-hosting requires recoverability and an owned support contract | Integrity-protected audit/search/export/legal hold; retention/residency; backup/restore/DR exercises; telemetry and support bundle redaction; admin/user/API/security docs; signed customer-owner acceptance | ENT-05/07/08/10, DIF-12; observability leaks and unverified operations | **Deep** with enterprise operators and target customers |
| **24. Full 1.0 Convergence And Release Proof** | Only combined evidence can authorize 1.0 language | Refresh 38/38 governance and Claude ledger; three-pack golden journeys; cross-surface and cross-tenant tests; fault injection; signed/checksummed/SBOM/provenance artifacts; Linux/macOS/WSL lifecycle; cloud/enterprise recovery; P0/P1 closure; local/external blockers zero with target evidence; user/owner acceptance | CORE-16, all DIF gates, all release gates; completion theater | **Deep** for release channels, signing/notarization, external environments, and acceptance |

### Phase Ordering Rationale

- Phases 1-3 freeze what the system means before moving ownership or generating clients.
- Phases 4-7 establish state, policy, runtime, and provider behavior before any new pack or shell can create a competing implementation.
- Phases 8-9 establish reliable execution, side-effect safety, multi-agent isolation, and extension/pack boundaries before expanding user-facing breadth.
- Phases 10-15 are three parallel pack workstreams after the common gates. Coding receives the highest capacity, but all six phases are required for 1.0.
- Phases 16-19 turn existing and new shells into thin consumers and prove cross-surface continuity before hosted scale adds network and tenancy failures.
- Phases 20-23 add official cloud and enterprise semantics as optional adapters, preserving no-account Local Personal operation.
- Phase 24 is evidence convergence, not a documentation-only wrap-up; it cannot manufacture target or user-value proof missing from earlier phases.

### Research Flags

Phases requiring deeper research during planning:

- **Phase 1:** freeze the dynamic Claude Code/Claude Desktop public baseline from dated official observable behavior; distinguish clean-room behavior from non-reusable proprietary source.
- **Phase 7:** run model-pinned provider research for Anthropic, OpenAI, Gemini, OpenRouter, OpenAI-compatible, and local models; document capability drift, retry, usage, and data-handling behavior.
- **Phases 10-11:** refresh Coding parity and build representative multi-language, remote, Git/CI, browser, voice, and IDE acceptance sets.
- **Phases 12-13:** research scholarly APIs/licensing, citation validation, OCR/table quality, statistics, experimental integrity, paper/reproduction formats, and EDA/hardware responsibility boundaries.
- **Phases 14-15:** research live connector semantics, OAuth scopes, office formats, meeting privacy, browser/Desktop automation, prompt injection, idempotency, compensation, and target-state verification.
- **Phases 17-19:** revalidate current VS Code, JetBrains, Tauri, Web, macOS, and WSL APIs, packaging rules, accessibility expectations, and security boundaries.
- **Phases 20-21:** produce cloud threat, tenancy, sync, billing, abuse, SLO, capacity, backup, and incident designs against real infrastructure.
- **Phases 22-23:** validate enterprise deployment topologies, SSO/RBAC, customer vault/KMS, offline dependencies, data governance, DR, observability, support, and license operations with target operators.
- **Phase 24:** research and collect current signing/notarization/channel requirements and external acceptance evidence; local substitutes are not acceptable.

Phases with strong repository-backed patterns that generally do not need a separate broad research pass are Phases 2, 3, 4, 6, 8, and 9. They still require focused spikes or security review where the phase table says so.

## Confidence Assessment

| Area | Confidence | Notes |
|---|---|---|
| Approved 1.0 scope | **HIGH** | `.planning/PROJECT.md` and the approved complete-product design agree on all packs, surfaces, local/cloud/enterprise forms, safety, and release gates. |
| Existing stack and repository boundaries | **HIGH** | Live manifests, lockfile findings, codebase maps, current crate ownership, schemas, and release scripts support the brownfield path. |
| New persistence/client/identity/observability stack | **MEDIUM** | Official registries and local references support the choices, but Kiana has not integrated or target-tested them. |
| Feature taxonomy and dependency order | **HIGH** | The four research outputs converge on contracts/state/policy/runtime/reliability before packs/surfaces/hosted layers. |
| Claude public feature completeness | **MEDIUM** | The local inventory is broad but proprietary behavior is dynamic and was not refreshed through live official documentation in this research run. |
| Provider behavior | **MEDIUM-LOW** | Adapter boundaries are clear, but model capabilities and proprietary service behavior change and require dated live contract suites. |
| Architecture extraction seams | **HIGH** | Runtime, state, and policy responsibilities are concentrated in identifiable current files and can be moved incrementally behind compatibility wrappers. |
| Research and Daily product details | **MEDIUM** | Approved scope is clear; scholarly, statistical, connector, office-format, and automation target behavior needs domain-specific validation. |
| Desktop/IDE/Web technology choices | **MEDIUM** | The direction is coherent, but exact versions, API compatibility, native security, packaging, and marketplace behavior are time-sensitive. |
| Cloud and enterprise design | **MEDIUM** | Standard patterns are identified, but no target tenant, billing, SSO, vault, DR, or customer acceptance proof exists yet. |
| Pitfall assessment | **HIGH** for repository risks; **MEDIUM** for hosted scale | The intentional fallback is grounded in current Kiana concerns, tests, release evidence, and the completed research; future operating risks still require live validation. |
| 38-reference governance | **MEDIUM-HIGH** | All 38 immediate directories have current mappings, but upstream changes, licenses, stubs, and proprietary boundaries require continuous re-audit. |

**Overall confidence:** MEDIUM-HIGH for roadmap direction and phase dependencies; MEDIUM for implementation details that depend on dynamic external products, live providers, target platforms, or customer environments.

### Gaps To Address

- **Dynamic proprietary baselines:** freeze official observable versions during Phase 1 and retain a dated delta ledger; do not claim perpetual parity.
- **Time-sensitive versions:** re-query and compatibility-test frontend, Tauri, IDE, auth, telemetry, and release-tool versions before adoption; pin the accepted set.
- **Provider truth:** use deterministic offline suites for correctness plus opt-in dated live suites for provider behavior; never make live provider availability the only correctness gate.
- **Research integrity:** establish adversarial corpora and expert-reviewed citation, statistics, experiment, and reproduction acceptance before allowing final-paper completion.
- **Desktop and external automation:** prove origin/target identity, native privilege isolation, prompt-injection resistance, receipt/reconciliation, and actual macOS/WSL behavior.
- **Hosted and enterprise proof:** select concrete infrastructure versions only with deployment manifests and upgrade/rollback/backup tests; validate tenancy and operations in target environments.
- **Release evidence:** collect real signing, notarization, channel, installation, recovery, support, cloud-owner, enterprise-owner, and target-user evidence. Existing local schemas and smoke tests are foundations, not completion.

## Sources

### Primary Repository Evidence (HIGH Confidence)

- [PROJECT.md](../PROJECT.md) - approved product scope, requirements, constraints, and business boundary.
- `docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md` - approved complete-product architecture, workflows, safety, surfaces, commercial delivery, and 1.0 gate.
- [STACK.md](STACK.md) - live stack, version, integration, release, and adoption recommendations.
- [FEATURES.md](FEATURES.md) - table stakes, differentiators, post-1.0 boundary, anti-features, dependencies, golden journeys, and 38-reference mapping.
- [ARCHITECTURE.md](ARCHITECTURE.md) - target boundaries, state ownership, event/port contracts, migration seams, failure semantics, and build order.
- [PITFALLS.md](PITFALLS.md) - current-repository risks, hosted risks, detection/recovery guidance, and phase warnings.
- `.planning/codebase/`, `docs/reference-migration-roadmap.md`, `docs/reference-feature-matrix.md`, current Cargo manifests/lockfile, schemas, runtime/task/evidence source, and release scripts referenced by the four research outputs.

### External And Reference Corroboration (MEDIUM Confidence)

- Official crates.io, npm, and Node registry metadata checked on 2026-07-14 for proposed database, client, identity, observability, testing, and release tools; versions remain subject to integration validation.
- The 38 local `reference/` repositories and their representative README, architecture, source-layout, manifest, and license evidence recorded in [FEATURES.md](FEATURES.md); governance decisions range from Adopt to Behavior-only/Reject.
- Proprietary or restored Claude materials are behavior and boundary references only; they do not authorize source reuse or prove current official behavior.

---
*Research completed: 2026-07-14*
*Ready for roadmap: yes, subject to the explicit confidence limits and research flags above*
