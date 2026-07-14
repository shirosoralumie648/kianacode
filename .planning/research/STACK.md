# Technology Stack Research

**Project:** Kiana
**Domain:** Local-first open-core AI agent platform spanning Coding, Academic Research, Daily Work, cloud, and enterprise self-hosting
**Researched:** 2026-07-14
**Overall confidence:** HIGH for the live Rust baseline and lockfile findings; MEDIUM for additions that are verified in official registries or local reference manifests but are not yet integrated into Kiana

## Executive Decision

Kiana should remain a Rust product with one typed runtime and multiple thin product shells. The existing Rust 2021 workspace, Tokio runtime, `RuntimeEvent`, tool/command registries, append-only workflow EventLog, Axum app server, MCP transports, and `rustls` HTTP stack are the correct baseline. A greenfield rewrite, a second agent runtime for Desktop/Web/IDE, or a Python/Node control core would erase the strongest work already present.

The stack needs four deliberate additions rather than a replacement:

1. Add a durable query/projection layer: SQLx 0.9 with bundled SQLite locally and PostgreSQL in cloud/enterprise deployments. The authenticated EventLog remains the fact source; databases hold indexes, projections, tenancy, leases, and queryable metadata.
2. Add one shared TypeScript client workspace: React 19 + TypeScript 7 + Vite 8, packaged by Tauri 2 for native Linux/macOS, served as the Web client, and reused selectively inside a VS Code webview. All clients consume the existing app-server/runtime schemas.
3. Add standards-based identity, secret storage, and telemetry: OIDC/OAuth 2, OS keychains, secret-value wrappers, and OpenTelemetry over the existing `tracing` instrumentation.
4. Strengthen release engineering: one pinned Rust toolchain, centralized workspace dependency versions, reproducible frontend locks, provenance/attestations, dependency audit/vetting, and real cross-platform client tests.

The most urgent stack correction is dependency governance. The root `[workspace.dependencies]` table is mostly advisory today because only `xcap`, `enigo`, and one `uuid` consumer use `workspace = true`. The lockfile consequently contains three `ratatui` lines, two `crossterm` lines, and two `tokio-tungstenite` lines. The root also declares `similar = "3.1"` while the consuming crate requests `2.6` and resolves `2.7.0`. Centralize actual dependencies before adding another runtime or UI ecosystem.

## Decision Legend

| Label | Meaning |
|---|---|
| **Keep** | Existing technology and boundary are suitable; preserve them. |
| **Strengthen** | Existing choice is sound but its versioning, production behavior, or governance must be hardened. |
| **Add** | Missing category required by the approved product. |
| **Avoid** | Do not introduce this technology or pattern in the stated scope. |

Confidence is attached to each recommendation. **HIGH** means it is demonstrated by Kiana's live manifests/code/docs or corroborated by the lockfile. **MEDIUM** means it is supported by an official registry/current local reference and fits the approved architecture, but Kiana still needs an integration spike and acceptance evidence.

## Keep As-Is

| Technology / boundary | Live version or contract | Purpose | Rationale | Confidence |
|---|---|---|---|---|
| Rust workspace | Rust 1.96.0, edition 2021, resolver 2 | Core runtime and all privileged behavior | It already owns runtime, sessions, providers, tools, policy, workflow, evidence, recovery, CLI/TUI, MCP, and native integration. Rust remains appropriate for a single distributable local core and controlled enterprise workers. Do not move to edition 2024 during the product-expansion milestone; that is a separate compatibility change. | HIGH |
| Tokio | Cargo requirement `1.44`; lock `1.52.3` | Async I/O, streaming, tools, MCP, remote sessions | It is pervasive and compatible with Axum, Reqwest, SQLx, and OpenTelemetry. A second async runtime would complicate cancellation, deadlines, and tests. | HIGH |
| Serde + JSON | `serde 1.0.228`, `serde_json 1.0.150` in `Cargo.lock` | Durable contracts and inter-process payloads | Existing schema-backed JSON and `RuntimeEvent` are already shared by CLI, SDK, MCP, app server, TUI, remote, and bridge. Keep JSON as the public compatibility format. | HIGH |
| Axum + Tower model | `axum 0.8.9`, `tower 0.5.3`; `tower-http 0.6.11` already resolved | Local app server and future cloud services | Axum is already wired to the product shell and shares Tokio/Serde. Promote Tower/Tower HTTP to explicit direct dependencies when production middleware is added; do not introduce a parallel web framework. | HIGH |
| Reqwest with Rustls | `reqwest 0.12.28`; manifests disable default features and enable `rustls-tls` on critical paths | Provider, marketplace, MCP, remote, and Web access | The stack avoids a platform OpenSSL dependency and already has shared SSRF/network-policy semantics. Keep all user-controlled targets behind `kiana-services`. | HIGH |
| Typed `RuntimeEvent` and JSON Schema v1 contracts | Repository contract, not a registry version | Cross-surface event and command protocol | This is Kiana's central interoperability asset. Desktop, Web, IDE, SDK, RPC, MCP, and cloud must consume it instead of defining surface-specific event models. | HIGH |
| Append-only EventLog plus rebuildable projections | Repository contract | Durable workflow truth, integrity, recovery | The approved design explicitly makes EventLog the fact source. Database adoption must not demote or bypass its authenticated append and recovery rules. | HIGH |
| CLI/TUI stack | `clap 4.6.1`; target `ratatui 0.29`; target `crossterm 0.28` | Local interactive and automation surfaces | These surfaces are already meaningful and are the most direct local-first delivery. Keep them first-class even after graphical clients arrive. | HIGH |
| Cargo locked/offline release gates | Cargo 1.96.0; existing shell gates | Deterministic local and release validation | `--locked --offline`, schema smoke, release smoke, and package lifecycle smoke are strong existing controls. Extend them; do not replace them with frontend-only or cloud-only CI. | HIGH |
| Dual open-source license | `MIT OR Apache-2.0` | Open-core personal core and ecosystem adoption | This is permissive and compatible with the current Rust dependency policy. Commercial services and enterprise packaging can remain separately licensed without changing the core license. | HIGH |

## Strengthen Existing Stack

### Toolchain And Dependency Governance

| Recommendation | Version policy | Why | Acceptance condition | Confidence |
|---|---|---|---|---|
| Add `rust-toolchain.toml` and pin the release toolchain | Exactly `1.96.0` for this milestone | `rust-version = "1.96"` is declared, but CI currently installs floating `stable`. A floating compiler makes reproducibility and MSRV claims unverifiable. | Local, CI, release, and enterprise offline builds resolve the same compiler/components; an intentional toolchain update is reviewed as its own change. | HIGH |
| Make root workspace dependencies real | Convert shared crate dependencies to `{ workspace = true }` and remove unused root entries | Current manifests independently request `tokio`, `serde`, `clap`, terminal crates, and WebSocket crates. The root table does not govern most of them. | `cargo metadata` shows one intended direct requirement per shared package and `cargo tree -d` has documented exceptions only. | HIGH |
| Unify terminal versions | `ratatui 0.29.0`, `crossterm 0.28.1` initially | The lock currently resolves Ratatui 0.26.3, 0.28.1, and 0.29.0 plus Crossterm 0.27.0 and 0.28.1. Unification reduces binary size and incompatible widget/event types. | `kiana-components`, `kiana-ink`, and `kiana-screens` use one pair; render and input regression tests pass. | HIGH |
| Unify WebSocket transport | Direct line `tokio-tungstenite 0.28.0` initially, then one reviewed upgrade | Kiana asks for 0.28 while the graph also resolves 0.29.0. A single reviewed transport version reduces TLS and frame-behavior variance. | All stdio/HTTP/SSE/WS MCP and remote fixtures pass with one resolved line. | HIGH |
| Correct `similar` ownership | Use the actually consumed `similar 2.7.0`, or migrate consumers in a dedicated change | Root declares unused `3.1`; `kiana-color-diff` requests 2.6 and locks 2.7.0. Describing 3.1 as the stack is currently false. | Root and consuming manifest agree; diff golden tests pass. | HIGH |
| Pin CI actions by full commit SHA | Keep human-readable version comments | Tag-only actions such as `actions/checkout@v4` and `dtolnay/rust-toolchain@stable` are mutable supply-chain inputs. | Release workflows use reviewed SHAs; update automation opens auditable PRs. | HIGH |

### Runtime And Contract Layer

| Recommendation | Version | Why | Boundary | Confidence |
|---|---|---|---|---|
| Promote Tower middleware to direct dependencies | Start with locked `tower 0.5.3` and `tower-http 0.6.11` | Cloud/local servers need request IDs, trace propagation, body limits, timeouts, CORS policy, compression policy, and concurrency limits. Existing transitive versions are the lowest-risk starting point. | Middleware cannot contain business/runtime state; it wraps the same Axum handlers and policy gates. | HIGH |
| Add schema derivation and drift checks incrementally | `schemars 1.2.1` verified in crates.io on 2026-07-14 | Hundreds of hand-maintained JSON schemas and Rust emitters create drift risk. Derive schemas for new contracts and compare generated output to committed golden files. | Existing published v1 schemas remain compatibility artifacts; generation is not allowed to rewrite them silently. | MEDIUM |
| Generate TypeScript types from committed JSON Schema | `json-schema-to-typescript 15.0.4`, `ajv 8.20.0` verified in npm on 2026-07-14 | Web, Desktop, and IDE clients need exact event/report types and runtime validation without duplicating Rust enums by hand. | `docs/schemas` remains the wire-contract source for clients; generated files are reproducible and checked in CI. | MEDIUM |
| Keep JSON/HTTP/WebSocket as the public headless protocol | Existing app-server and MCP contracts | It works in browsers, IDE extension hosts, shell automation, and remote bridges. | gRPC/Protobuf may be introduced only for a measured internal service need, behind adapters; it must not replace public RuntimeEvent JSON in this milestone. | HIGH |

## Add: Persistence And Retrieval

### Recommended Data Stack

| Technology | Version | Role | Why this choice | Constraints | Confidence |
|---|---|---|---|---|---|
| SQLx | `0.9.0` | Shared Rust database driver and migrations | It matches Tokio, supports both bundled SQLite and PostgreSQL, and is also pinned by the local `reference/codex/codex-rs` state stack. One reviewed database abstraction is preferable to adding separate local and cloud ORMs. | Use repository interfaces; do not share one physical schema between local personal storage and multi-tenant cloud storage. Disable unused SQLx features. | MEDIUM |
| SQLite via SQLx `sqlite-bundled` | SQLite version supplied by the reviewed SQLx/libsqlite build | Local session indexes, workflow projections, memory metadata, FTS5, artifact metadata, sync queue | A bundled engine preserves no-account/offline operation and avoids a system database prerequisite on Linux/macOS/WSL. SQLite should index and project existing JSONL/events rather than replace them. | WAL, busy timeout, migrations, bounded transactions, backup/restore, corruption recovery, and lock-contention tests are release requirements. No runtime-loaded extensions by default. | MEDIUM |
| PostgreSQL via SQLx | Server major not yet pinned in this repository | Cloud/enterprise tenants, users, RBAC, workflow leases, sync cursors, billing references, audit indexes | PostgreSQL provides transactions, row-level security, mature backup tooling, and the concurrency model needed by remote workers and teams. | Pin one upstream-supported major in deployment manifests before implementation; require tenant ID and RLS tests on every tenant-owned table. | MEDIUM |
| PostgreSQL transactional outbox and leased job rows | Schema/protocol version owned by Kiana | Reliable control-plane commands and worker dispatch for 1.0 | It keeps workflow state changes and dispatch publication atomic without making Redis or a new broker a second truth source. | Use idempotency keys, `FOR UPDATE SKIP LOCKED`, lease expiry, attempt budgets, and result-unknown semantics. Add a broker only after measured throughput requires it. | MEDIUM |
| `object_store` | `0.14.0` | S3-compatible/cloud-neutral artifact blobs | Large evidence, bundles, PDFs, datasets, screenshots, and release artifacts should not live in PostgreSQL. The crate offers a provider-neutral Rust boundary and is MIT/Apache-2.0. | Store content hashes and tenant metadata transactionally in PostgreSQL; use envelope encryption, retention policy, and immutable-object tests. Enterprise may supply its own compatible service. | MEDIUM |
| SQLite FTS5 first | Bundled SQLite capability | Local lexical search | It is sufficient for deterministic local text search and avoids introducing a search daemon. | Keep the existing deterministic search/vector contracts. Index only approved content and retain provenance/hash links. | MEDIUM |
| PostgreSQL vector extension only when semantic retrieval is productionized | Extension/server version not yet pinned | Cloud semantic retrieval | This avoids a separate vector service for the first complete release while preserving a migration path. | Do not claim production embeddings from the current deterministic hash-vector fixture. Pin extension/server versions and provide recall/latency/eval evidence before enabling. | MEDIUM |

### Persistence Ownership Rules

- `kiana-tasks` remains the authority for Workflow/EventLog/evidence transitions. A new low-level persistence crate may expose migrations, transactions, and projections, but command and UI code must not write workflow tables directly. **Confidence: HIGH.**
- Local SQLite is a rebuildable projection for event/session search plus durable non-EventLog records such as sync cursors. Every projected row must retain source event identity/schema/hash. **Confidence: HIGH.**
- Cloud PostgreSQL owns tenant/account/control-plane facts that do not exist in local mode, while synchronized task/event payloads retain the same Kiana contracts. **Confidence: HIGH.**
- Large blobs are content-addressed in object storage; PostgreSQL stores digest, size, media type, encryption key reference, retention, and authorization metadata. **Confidence: MEDIUM.**
- Do not load arbitrary SQLite extensions supplied by projects/plugins. If a vector extension is later selected, statically package and audit it per target. **Confidence: MEDIUM.**

## Add: Desktop, Web, And IDE

There is no JavaScript manifest in the live Kiana product today. Add one client workspace only after the app-server/RuntimeEvent contract is frozen for the first client slice. Exact versions below were verified from the official npm/crates registries on 2026-07-14 and should be committed as exact versions with a lockfile, not copied as floating ranges.

### Shared Client Workspace

| Technology | Initial exact version | Purpose | Why | Confidence |
|---|---|---|---|---|
| Node.js LTS | `24.18.0` (`Krypton`) | Reproducible frontend/extension build runtime | It is the latest LTS entry returned by the official Node distribution index during research and satisfies current Vite/pnpm engine constraints. Pin it in CI and a version file. | MEDIUM |
| pnpm | `11.13.0` | One lockfile/workspace for Web, Desktop UI, shared client, and IDE | It gives strict, space-efficient dependency management and an explicit `packageManager` field. Do not add npm, Yarn, Bun, and pnpm locks together. | MEDIUM |
| TypeScript | `7.0.2` | Client and extension language | Strong generated contract types are important across hundreds of runtime/report schemas. This is a new major, so validate VS Code/Tauri tooling compatibility in the first spike before broad adoption. | MEDIUM |
| React / React DOM | `19.2.7` | Shared operational UI | React is supported by the strongest local Web/IDE references and lets Kiana share transcript, approval, diff, artifact, and status components without sharing runtime state. | MEDIUM |
| Vite | `8.1.4` | Webview/Web/Tauri frontend build | A static client build fits an authenticated app-server product; server rendering adds little value for an operational agent UI. | MEDIUM |
| React Query | `5.101.2` | Snapshot/request cache around typed app-server calls | It provides explicit invalidation and mutation state while RuntimeEvent WebSocket reducers remain Kiana-owned. It must not become a second workflow state store. | MEDIUM |
| Lucide React | `1.24.0` | Shared icon system | It satisfies consistent Desktop/Web/IDE controls without shipping hand-maintained SVG sets. | MEDIUM |
| Vitest | `4.1.10` | Client unit/component tests | It matches the Vite toolchain and is used by current local Web/IDE references. | MEDIUM |
| Playwright | `1.61.1` | Browser user journeys and visual/state regression | It covers Web and VS Code webview journeys against the real app server. Native packaging needs separate launch/install smoke. | MEDIUM |

### Surface Choices

| Surface | Stack | Responsibility | Cross-platform and licensing notes | Confidence |
|---|---|---|---|---|
| Desktop | Tauri `2.11.5`; `@tauri-apps/cli 2.11.4`; `@tauri-apps/api 2.11.1`; shared React/Vite client | Native Linux/macOS shell, keychain integration, local daemon lifecycle, deep links, notifications, updater, and OS file dialogs | Tauri is MIT/Apache-2.0 and reuses the Rust core. Audit each plugin permission/capability. Do not expose a generic privileged invoke bridge to third-party web content. | MEDIUM |
| Web | Static React/Vite client + existing Axum app server | Remote/team workspaces, workflow views, approvals, admin/operations | Serve versioned static assets from the service or CDN. The Web client never owns policy, secrets, or workflow truth. | MEDIUM |
| Windows v1 | WSL-hosted Kiana core + browser-based Web client from Windows | Approved first-release Windows experience | This respects the explicit WSL boundary. Document loopback/auth/file-path behavior and test WSL1/WSL2 as applicable. Native Windows Tauri/worker support remains a later milestone. | HIGH |
| IDE v1 | VS Code extension host in TypeScript, `@types/vscode 1.125.0`, `@vscode/test-electron 3.0.0`, optional React webview | Editor selection/context, diagnostics, diff/review, approvals, task state | Use the app-server/headless client over authenticated loopback/remote transport. The extension must not embed a second model loop, session store, provider registry, or permission engine. VS Code API and extension dependencies are MIT-compatible; marketplace terms require separate release review. | MEDIUM |
| Headless SDK | Generated TypeScript client plus existing Rust interfaces and JSON/RPC/MCP | CI, automation, integrations | Generate request/event/report types from committed schemas and expose bounded async streams. Keep wire compatibility independent from UI release cadence. | MEDIUM |

The frontend workspace should contain packages such as `client-contracts`, `client-runtime`, `ui`, `web`, `desktop`, and `vscode`, but only `client-contracts` and carefully selected presentation components should be shared. Do not share mutable global stores between browser, Tauri, and extension hosts. **Confidence: MEDIUM.**

## Add: Cloud Data Plane And Control Plane

| Plane | Recommended stack | Role and rationale | Deployment rule | Confidence |
|---|---|---|---|---|
| Shared service foundation | Existing Rust 1.96/Tokio/Axum/Serde/Tower/Reqwest-Rustls | Reuse the same contracts, policy decisions, provider adapters, and RuntimeEvent semantics. A Rust service layer avoids a second implementation of safety-critical behavior. | Extract service crates below entrypoints; do not run CLI command parsing inside HTTP handlers. | HIGH |
| Control plane | Axum + PostgreSQL/SQLx + OIDC + transactional outbox | Accounts, organizations, subscriptions, tenant policy, worker registration, workflow placement, audit index, retention, and admin APIs | Keep commercial/account concerns outside the open local core. Every write has tenant scope, idempotency, audit actor, and policy decision. | MEDIUM |
| Data plane | Signed Kiana worker artifact or OCI image + object storage + leased jobs | Executes workflow packets in isolated workspaces and streams typed events/evidence | Workers receive short-lived scoped credentials and immutable inputs. A worker cannot grant itself broader tenant policy or publish completion without verifier evidence. | MEDIUM |
| Sandbox | Existing local `bwrap` path for Linux; rootless OCI container/VM provider for managed workers | Stronger isolation for untrusted remote jobs | Define a sandbox-provider trait and capability report. Container availability is explicit; host-shell fallback must fail closed when isolation is required. | MEDIUM |
| Enterprise self-host | OCI images, Helm chart for Kubernetes, plus a documented single-node Compose-style topology | Repeatable online/offline installation | Kubernetes is supported, not mandatory. Enterprises may provide PostgreSQL, object storage, OIDC, KMS, ingress, and observability endpoints. Ship checksums, SBOM, provenance, migrations, backup/restore, and rollback tooling. | MEDIUM |
| Event fan-out at scale | PostgreSQL outbox first; optional NATS JetStream adapter after measurement | Avoid premature broker sprawl while preserving a durable upgrade path | Do not use Redis Pub/Sub or WebSocket delivery as durable workflow truth. A broker adapter must carry event IDs and support idempotent replay. | MEDIUM |

Do not couple the local personal binary to control-plane availability. Cloud login, sync, remote execution, billing, team space, and enterprise governance are optional adapters selected explicitly. **Confidence: HIGH.**

## Add: Authentication, Secrets, And Cryptography

| Technology / pattern | Version | Purpose | Rationale and constraints | Confidence |
|---|---|---|---|---|
| `keyring` | `4.1.4` | Local API keys and refresh tokens in macOS Keychain/Linux Secret Service | It gives a Rust credential-store boundary and is MIT/Apache-2.0. Implement capability/status reporting because headless Linux and WSL may lack a usable Secret Service. | MEDIUM |
| `secrecy` + existing `zeroize` | `secrecy 0.10.3`, `zeroize 1.9.0` | Reduce accidental display/serialization and clear supported secret buffers | These wrappers supplement, not replace, secure storage. Secret values never implement ordinary debug/JSON paths. | MEDIUM |
| OIDC | `openidconnect 4.0.1` | Cloud/enterprise interactive identity and token validation | Standards-based federation supports external IdPs without Kiana owning passwords. Require issuer/audience/nonce/PKCE/state checks and JWKS rotation. | MEDIUM |
| OAuth 2 | `oauth2 5.0.0` | Provider login, CLI device/loopback flows, scoped connectors | Use Authorization Code + PKCE for Desktop/Web and device authorization where the provider supports it. Tokens remain outside model context and ordinary logs. | MEDIUM |
| Server secret-store trait | Product contract; backend version not yet pinned | Cloud KMS/Vault/Kubernetes secret integration | Enterprise customers need pluggable secret ownership. Store only secret references and fingerprints in PostgreSQL. | MEDIUM |
| Envelope encryption | Product contract; cryptographic implementation requires a dedicated review | Synced blobs, object-store artifacts, and backups | Use per-tenant/per-object data keys wrapped by KMS-held keys; record algorithm/key version and support rotation. Do not invent a custom cipher or key derivation format. | MEDIUM |

WSL must have an explicit secret-storage state: secure backend available, environment/permission-checked token file explicitly configured, or blocked. Never silently fall back to plaintext `config.toml`. **Confidence: HIGH.** Local personal use remains account-free; OIDC is not a prerequisite for local Core. **Confidence: HIGH.**

## Add: Observability

| Technology | Version | Purpose | Rules | Confidence |
|---|---|---|---|---|
| Existing `tracing` | `0.1.44` | Structured application spans/events | Make request/workflow/session/turn/tool IDs fields, not interpolated message text. | HIGH |
| `tracing-subscriber` | `0.3.23` | Env filtering, JSON output, redaction layer | Local logs default to bounded local files or stderr. User content and secrets are excluded unless an explicit diagnostic export includes reviewed redaction. | MEDIUM |
| OpenTelemetry family | `opentelemetry 0.32.0`, `opentelemetry-otlp 0.32.0`, `tracing-opentelemetry 0.33.0` | Vendor-neutral traces and metrics export | Pin the family together; test OTLP-disabled behavior. Local telemetry is off by default. Cloud/enterprise export is policy-controlled and tenant-aware. | MEDIUM |
| Prometheus-compatible metrics endpoint | Protocol version, no client library pinned yet | Self-hosted operational metrics | Expose aggregate queue, latency, error, retry, token/cost, worker, and storage signals without prompt/file contents or high-cardinality IDs. | MEDIUM |

Use OpenTelemetry as the interoperability layer, not as the source of workflow truth. Evidence, audit events, and recovery state stay in Kiana's authenticated domain stores. **Confidence: HIGH.**

## Packaging And Distribution

### Preserve And Extend

| Recommendation | Current / target tooling | Why | Confidence |
|---|---|---|---|
| Keep Kiana's release scripts as the canonical proof orchestrator | Existing `release-smoke`, package lifecycle, compliance, signature, blocker, and proof scripts | They encode product-specific schema, acceptance, signing, and recovery evidence that generic packagers do not understand. | HIGH |
| Add a versioned release-tool manifest | `cargo-audit 0.22.2`, `cargo-deny 0.20.2`, `cargo-vet 0.10.2`, `cargo-cyclonedx 0.5.9` verified in crates.io | `cargo install --locked` without an explicit version changes the audit toolchain over time. Pin versions and verify downloaded binaries/checksums in offline bundles. | MEDIUM |
| Strengthen SBOM generation | CycloneDX; start with `cargo-cyclonedx 0.5.9` or make the current generator dependency-complete | The current generator inventories components but should also preserve the resolved dependency graph, target/features, tool identity, and schema validation. | MEDIUM |
| Add Cargo Vet policy | `cargo-vet 0.10.2` | RustSec finds known vulnerabilities; Cargo Vet adds review/audit provenance for new dependency imports. | MEDIUM |
| Add build provenance | SLSA/in-toto statement; signing tool version not yet pinned | Bind Git commit/tree state, Cargo.lock, toolchains, target, CI identity, SBOM, artifacts, and hashes. Current detached-signature hooks can remain, but provenance must be machine-verifiable. | MEDIUM |
| Sign each delivery format using platform-appropriate mechanisms | Existing external signature interface; Apple Developer ID/notarization for macOS; OCI signing for worker images | Archive signatures alone do not establish native app trust or container provenance. Keep signer identities external to the repository. | MEDIUM |
| Continue Homebrew and signed archives for CLI/Core | Existing scripts/manifests | Fits Linux/macOS and WSL without adding a language runtime. | HIGH |
| Add Tauri bundles for Desktop | Tauri 2 bundler outputs, exact formats selected per target | Desktop needs separate install/upgrade/uninstall/notarization proof from the CLI tarball. | MEDIUM |
| Treat native Windows packages as post-1.0 scope | WSL consumes the Linux Core package | The approved platform boundary explicitly excludes a native Windows client from the first complete release. Existing Windows packaging code is future evidence, not a reason to expand scope. | HIGH |

`cargo-dist` may be evaluated later as a builder for standard archives/installers, but it must not replace Kiana's proof manifests, lifecycle smoke, signing policy, or enterprise offline bundle logic wholesale. **Confidence: HIGH.**

## Testing And Evaluation Stack

| Layer | Technology / version | Required coverage | Why | Confidence |
|---|---|---|---|---|
| Rust correctness | Existing `cargo test`; add `cargo-nextest 0.9.140` | Unit, integration, process, target-specific suites | Nextest offers isolation, retries, partitioning, and CI reports. Keep the serial release gate until global cwd/env tests are made independent; do not mask races with retries. | MEDIUM |
| Contract snapshots | `insta 1.48.0` plus existing JSON Schema smoke | RuntimeEvent, MCP, app-server, CLI JSON, TUI reducer, migration golden files | Reviewable snapshots catch accidental compatibility drift. Normalize timestamps/paths; never snapshot secrets. | MEDIUM |
| Property/fuzz-style invariants | `proptest 1.11.0` | EventLog prefix recovery, DAG transitions, IDs, path containment, schema/version parsing, redaction | Integrity and recovery code has combinatorial inputs that example-only tests miss. | MEDIUM |
| HTTP/provider fixtures | `wiremock 0.6.5` | Provider adapters, redirects/SSRF, OAuth/OIDC, object store, retries/idempotency | Deterministic local fixtures keep default CI offline and credential-free. Existing MCP transport fixtures remain. | MEDIUM |
| CLI/process tests | `assert_cmd 2.2.2`, `tempfile 3.27.0` | Installed binary, environment isolation, WSL/path behavior, migration/rollback | Replaces ad hoc global cwd/env mutation with process boundaries and temporary roots. | MEDIUM |
| Performance | `criterion 0.8.2` plus scenario load tests | Event replay/projection, repo indexing, FTS, serialization, WebSocket fan-out, worker scheduling | Establish budgets before selecting a broker/vector service or changing storage. | MEDIUM |
| Client unit/component | `vitest 4.1.10` | Reducers, generated types, approval logic, diff/artifact views, accessibility states | Fits the selected Vite/React stack. | MEDIUM |
| Web journeys | `@playwright/test 1.61.1` | Login/no-login boundaries, session resume, streaming, approval, disconnect/recovery, admin/RBAC, mobile-width Web views | Uses the real Axum server and seeded deterministic provider. Visual checks supplement, not replace, state assertions. | MEDIUM |
| VS Code extension | `@vscode/test-electron 3.0.0` + Playwright where appropriate | Activation, workspace trust, file scopes, diff, diagnostics, reconnect, app-server version mismatch | Proves the extension is a shell over Core rather than a divergent runtime. | MEDIUM |
| Desktop packaging | Tauri build/install/launch smoke on Linux and macOS release runners | Keychain capability, deep link, updater rollback, local daemon handshake, permissions, notarization | Browser tests cannot prove native bundle behavior. | MEDIUM |
| Agent evaluation | Existing strict offline RuntimeEvent suite + deterministic fake providers + versioned Coding/Research/Daily golden tasks | Completion evidence, policy, recovery, citations, external receipts, regressions, cost/latency budgets | Keep default evaluation deterministic/offline. Live provider suites are opt-in, dated, model-pinned, and never the sole correctness gate. | HIGH |
| Fault injection | Process kill, truncated writes, DB busy/corruption, network partitions, expired leases, duplicate webhooks, object-store failures | Result-unknown and recovery semantics across local/cloud | Reliability is Kiana's product differentiator and requires failures at storage/process/network boundaries, not only mock success paths. | HIGH |

## Supply Chain And Licensing Rules

| Rule | Required implementation | Rationale | Confidence |
|---|---|---|---|
| Preserve the permissive core dependency policy | `cargo-deny` allow list, npm license inventory, binary notices, target/feature-specific review | Rust and client dependencies must remain compatible with `MIT OR Apache-2.0`; frontend and native plugin transitive licenses need the same gate. | HIGH |
| Audit optional/native features separately | Default Core, native computer use, Desktop, IDE, cloud worker, enterprise image graphs | The current lock includes advisories reachable only through optional capture dependencies. One aggregate lock scan cannot prove each shipped artifact's exposure. | HIGH |
| Pin all ecosystems | Cargo.lock, `pnpm-lock.yaml`, exact `packageManager`, Node/Rust versions, OCI base-image digests, action SHAs | Kiana cannot claim reproducibility while any build input floats. | HIGH |
| Review Tauri plugins and IDE dependencies as privileged code | Explicit allowlist, capability manifest, provenance, least privilege | These packages execute at sensitive desktop/editor boundaries and must not be treated as ordinary presentation libraries. | MEDIUM |
| Do not bundle copyleft infrastructure casually | Customer-provided S3-compatible services and external IdPs by interface; legal review before redistribution | Some popular self-hosted storage/auth/vector products use AGPL, SSPL, BSL, or source-available terms that can conflict with commercial redistribution/support obligations. | HIGH |
| Keep reference code behind an Adopt/Adapt/Reject license record | Reference snapshot/version, source path, license, owner, tests, evidence | The approved product requires clean-room behavior work for proprietary references and license-aware reuse for open source. | HIGH |

## Technologies And Patterns To Avoid

| Avoid | Why it is wrong for Kiana | Use instead | Confidence |
|---|---|---|---|
| Greenfield rewrite or a second Python/Node agent core | Duplicates policy, sessions, events, providers, tools, evidence, and recovery; invalidates brownfield proofs | Extend the existing Rust crates and typed contracts. | HIGH |
| Electron for Desktop | Ships a full browser runtime, duplicates the Rust process boundary, increases patch and memory burden, and offers no necessary advantage for this local Rust core | Tauri 2 + shared React/Vite client. | MEDIUM |
| Next.js or SSR as the primary product shell | Kiana is an authenticated operational application, not a content site; SSR adds Node server state and another deployment runtime | Static Vite client backed by Axum APIs/WebSockets. | MEDIUM |
| Native Windows client in the first complete release | Contradicts the approved WSL boundary and multiplies sandbox, updater, signing, filesystem, and input work | WSL Core + browser Web client; schedule native Windows separately. | HIGH |
| Mandatory account/cloud dependency | Violates the local-first, no-account personal core requirement | Local SQLite/files/EventLog; cloud adapters explicitly enabled. | HIGH |
| UI-specific session stores or event shapes | Breaks cross-entrypoint resume, approval, evidence, and recovery parity | Generated clients over `RuntimeEvent` and app-server schemas. | HIGH |
| Making SQLite/PostgreSQL the workflow fact source | Bypasses authenticated append/recovery semantics and creates split-brain with existing EventLog artifacts | EventLog authority plus rebuildable DB projection. | HIGH |
| Redis as durable workflow state or the first job queue | Adds another operational dependency without transactional coupling to tenant/workflow state | PostgreSQL outbox and leased jobs; add a broker only from measured need. | MEDIUM |
| A vector database before real retrieval evaluation | Current vector behavior is a deterministic fixture, not evidence that a new service improves task success | SQLite FTS/local scoring first; PostgreSQL vector extension only after evals. | HIGH |
| Runtime-loaded native SQLite extensions from plugins/projects | Expands code execution and ABI/supply-chain risk at a privileged persistence boundary | Statically reviewed features or pure-Rust scoring behind a capability gate. | MEDIUM |
| Custom password authentication, custom token format, or custom cryptography | High security and enterprise integration risk | OIDC/OAuth 2, reviewed JOSE libraries, KMS-backed envelope encryption. | HIGH |
| Plaintext secret fallback | WSL/headless convenience would become credential leakage | Capability-aware keyring, permission-checked explicit token files for automation, or blocked setup. | HIGH |
| Raw prompts, files, tokens, or secrets in telemetry | Violates local-first privacy and enterprise isolation | Redacted structured metadata, opt-in local export, tenant-aware OTLP. | HIGH |
| Kubernetes as a local or universal enterprise prerequisite | Makes personal and smaller self-host deployments unnecessarily fragile | Local binary/Tauri; single-node topology; optional Helm for Kubernetes operators. | MEDIUM |
| Wholesale replacement of release scripts with `cargo-dist` | Loses Kiana-specific schema, acceptance, evidence, signing, and enterprise handoff gates | Keep scripts canonical; optionally delegate narrow artifact-building steps. | HIGH |
| Floating `stable`, unpinned Actions, `npx --yes`, or unversioned `cargo install` in release CI | Executes mutable tools and defeats provenance/offline reproducibility | Exact toolchains, SHAs, package versions, locks, checksums, offline bundle. | HIGH |
| Copying OpenHands/AutoGen/MetaGPT/Cline implementation stacks wholesale | Their Python/React/Node service shapes and licenses do not match Kiana's existing Rust boundaries; some are archived, maintenance-mode, or WIP | Adapt observable contracts and product patterns through Kiana-owned Rust/client interfaces. | HIGH |

## Alternatives Considered

| Category | Recommended | Alternative | Why the alternative is not the default | Confidence |
|---|---|---|---|---|
| Local database | SQLx 0.9 + bundled SQLite | `rusqlite 0.40.1` | Rusqlite is a strong embedded option, but using SQLx for SQLite/PostgreSQL matches Kiana's Tokio stack and the local Codex reference while reducing driver/migration duplication. Reconsider only if measured async/SQLx overhead or required SQLite APIs block the projection layer. | MEDIUM |
| Desktop | Tauri 2 | Electron | Electron's bundled Chromium and Node privilege surface are unnecessary when the core is already Rust and a Web client is shared. | MEDIUM |
| Web frontend | React + Vite static app | Next.js/SSR | Operational authenticated screens do not need SSR; Axum already owns the server and contracts. | MEDIUM |
| Cloud database | PostgreSQL | Distributed SQL database | PostgreSQL is simpler to operate and sufficient until measured tenant/region scale proves otherwise. Preserve exportable contracts rather than pre-optimizing storage. | MEDIUM |
| Job dispatch | PostgreSQL outbox/leases | Redis queue or NATS JetStream immediately | Transactional coupling and fewer services are more important at 1.0. NATS is a valid later adapter for measured fan-out/throughput. | MEDIUM |
| Blob API | `object_store` abstraction | Direct AWS SDK throughout business code | A narrow provider-neutral boundary supports official cloud and enterprise-owned S3-compatible storage without spreading provider types. | MEDIUM |
| Identity | External OIDC providers | Bundled custom identity/password system | Standards reduce credential liability and satisfy enterprise federation. Self-host packages should integrate the customer's IdP. | HIGH |
| Contract source | Existing JSON Schema + generated clients | Protobuf/gRPC rewrite | JSON is already public and browser/MCP friendly. Protobuf would create a migration with no demonstrated need. | HIGH |
| Release automation | Existing proof scripts + pinned specialized tools | Full `cargo-dist` takeover | Generic distribution is useful, but Kiana's release definition includes evidence, acceptance, platform security, and enterprise offline proofs. | HIGH |

## Version Compatibility And Pinning

| Set | Compatibility decision | Required check | Confidence |
|---|---|---|---|
| Rust/Cargo | Pin 1.96.0; keep edition 2021 | Full workspace, all features, clippy/fmt/test/build, target release runners | HIGH |
| Axum/Tower | Start from locked Axum 0.8.9 + Tower 0.5.3 + Tower HTTP 0.6.11 | HTTP/SSE/WS/MCP fixtures, body/timeout/trace middleware, schema snapshots | HIGH |
| Terminal | Converge on Ratatui 0.29.0 + Crossterm 0.28.1 | Headless render, PTY/input, resume/history, approval panel | HIGH |
| SQLx | Pin 0.9.0 with minimal SQLite/PostgreSQL/migrate/Tokio-Rustls features | Offline metadata/migrations, bundled SQLite per target, WAL recovery, PostgreSQL integration | MEDIUM |
| Tauri | Keep Rust crate/CLI/API on the compatible 2.11 release line selected by the lockfiles | Linux/macOS build, plugin permissions, updater, deep links, signing/notarization | MEDIUM |
| Client | Node 24.18.0 + pnpm 11.13.0 + React 19.2.7 + Vite 8.1.4 + TypeScript 7.0.2 | Clean frozen install, browser support matrix, VS Code extension build, Tauri build | MEDIUM |
| OpenTelemetry | Upgrade `opentelemetry`, SDK/exporter, semantic conventions, and tracing bridge as one set | No-export mode, OTLP fixture, trace context propagation, redaction | MEDIUM |
| Auth | `openidconnect 4.0.1` + `oauth2 5.0.0` | PKCE/state/nonce, issuer/audience, JWKS rotation, device/loopback flows, clock skew | MEDIUM |
| Release tools | Pin every CLI version in a checked manifest | Offline install/bundle, checksum, tool `--version`, SBOM/provenance records | MEDIUM |

The infrastructure rows without an exact version are intentional. Kiana currently has no deployment manifests for PostgreSQL, Kubernetes, an object-store service, or a KMS; selecting a number here without testing the eventual distribution would be false precision. Their exact supported versions must be pinned in the phase that adds deployment manifests and exercised through upgrade/rollback/backup tests.

## Recommended Adoption Order

1. **Dependency/toolchain convergence:** pin Rust, centralize workspace dependencies, remove duplicate terminal/WebSocket lines, correct the `similar` declaration, and pin CI/release tools.
2. **Storage and contracts:** introduce SQLx migrations and SQLite projections behind repository interfaces; add schema generation/golden checks and keyring capability reporting; preserve EventLog authority.
3. **Headless production hardening:** explicit Tower middleware, generated TypeScript client, OpenTelemetry redaction/opt-in controls, and contract/version negotiation.
4. **Shared client workspace:** React/Vite UI against the real local app server, with Vitest/Playwright and no direct privileged filesystem/provider access.
5. **Desktop and IDE:** Tauri Linux/macOS packaging and a VS Code shell over the same client/contracts; Windows remains WSL + Web.
6. **Cloud control/data plane:** PostgreSQL tenancy/outbox, object storage, OIDC, remote worker sandbox, backup/restore, and audit/evidence flows.
7. **Enterprise/release closure:** OCI/Helm plus single-node topology, KMS/secret adapters, provenance/attestation, platform signing, offline bundles, fault injection, and target-customer acceptance.

This order follows the repository's existing core-loop-first rule: storage and wire contracts stabilize before clients; clients stabilize before cloud scale; commercial packaging proves the same behavior rather than introducing a parallel product.

## Sources

### Primary Repository Evidence (HIGH)

- `.planning/PROJECT.md` - approved product, platform, local-first, safety, licensing, and delivery constraints.
- `docs/superpowers/specs/2026-07-14-kiana-complete-ai-agent-product-design.md` - approved Core/packs/surfaces/cloud/enterprise design and release gates.
- `.planning/codebase/STACK.md`, `.planning/codebase/ARCHITECTURE.md`, `.planning/codebase/INTEGRATIONS.md` - live Rust layering, dependencies, transports, persistence, and integration boundaries.
- `Cargo.toml`, all workspace crate manifests, and `Cargo.lock` - declared and resolved versions on 2026-07-14.
- `docs/reference-migration-roadmap.md` and `docs/reference-feature-matrix.md` - implemented contracts, open gaps, reference decisions, and proof status.
- `deny.toml`, `scripts/release-smoke.sh`, `scripts/package-release.sh`, `scripts/compliance-audit.sh`, and `.github/workflows/release.yml` - current dependency, packaging, audit, and release behavior.
- `reference/codex/codex-rs/Cargo.toml` - current local reference evidence for SQLx 0.9 bundled SQLite, schema generation, keyring, snapshots, and OpenTelemetry.
- `reference/cline`, `reference/continue`, and `reference/OpenHands/openhands-ui` manifests - local reference evidence for React/Vite/Vitest/Playwright/VS Code product shells; used as corroboration, not copied as implementation instructions.

### Official Registry And Upstream Metadata (MEDIUM)

- Rust crates metadata queried from `https://crates.io/api/v1/crates/<crate>` on 2026-07-14 for SQLx, keyring, secrecy, OpenTelemetry, object_store, OIDC/OAuth, schema/testing, and release tools.
- npm package metadata queried from `https://registry.npmjs.org/<package>/latest` on 2026-07-14 for React, TypeScript, Vite, pnpm, Tauri JS packages, Vitest, Playwright, VS Code test/types, AJV, generated TypeScript contracts, icons, and React Query.
- Node release index queried from `https://nodejs.org/dist/index.json` on 2026-07-14; the first LTS entry was Node 24.18.0 (`Krypton`).
- Tauri crate metadata queried from crates.io; stable crate version observed was 2.11.5 with `Apache-2.0 OR MIT` licensing.

Context7, Brave, Firecrawl, and Exa were unavailable in this research session. Time-sensitive package versions above were therefore verified against official package registries and local manifests; no unverified current-version claim is used. Infrastructure products whose deployable versions were not verified are explicitly left unpinned.

---
*Stack research for the Kiana complete-product milestone; brownfield evolution, not a rewrite.*
