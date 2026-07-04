# Kiana Commercial Release Readiness

Date: 2026-07-04

## Verified Local Release Candidate Gates

The local source-build release candidate now has a passing Windows/Git Bash gate:

```bash
CARGO_REGISTRIES_CRATES_IO_PROTOCOL=git \
CARGO_NET_GIT_FETCH_WITH_CLI=true \
./scripts/release-smoke.sh
```

The gate covers:

- `cargo fmt --all --check`
- `cargo test --workspace --locked --offline --no-fail-fast`
- `cargo build --release --locked --offline -p kiana-entrypoints --bin kiana`
- Release binary version, doctor, model smoke, model catalog, and license status smoke.
- Help, auth, completion, plugin marketplace, MCP config, and project MCP smoke.
- Temporary source install into an isolated `INSTALL_DIR`.
- Installed binary version, doctor, help, auth, plugin, and MCP smoke.
- Commercial full preflight also invokes opt-in live provider and remote smoke wrappers when production-like credentials or daemons are available; local RC mode keeps those checks external.

The package script also produced a local tarball and checksums in a temporary `DIST_DIR`:

```bash
DIST_DIR="$(mktemp -d)" ./scripts/package-release.sh
```

## Completed This Pass

- Windows release-smoke now resolves `kiana.exe` and preserves a usable Git Bash tool PATH for child Windows processes.
- Install smoke uses the actual Cargo and Rustup homes instead of assuming `HOME/.cargo`.
- Doctor warnings in a clean environment are verified by content instead of treated as hard install failures.
- Temporary install smoke runs offline after the main build gate, avoiding accidental registry access.
- Added `scripts/package-release.sh` for tar.gz plus SHA256 artifacts.
- Added `.github/workflows/release.yml` for tag/manual cross-platform artifact packaging, checksum verification, artifact upload, and draft GitHub Release creation.
- Added `SECURITY.md`, `PRIVACY.md`, and `TELEMETRY.md`.
- Added `CHANGELOG.md`, `UPGRADE.md`, `docs/release-checklist.md`, and `docs/distribution-channels.md`.
- Added workspace package metadata inheritance for internal crates and marked them `publish = false`.
- Tightened `.gitignore` so local task state, secrets, logs, build output, release output, and raw reference checkouts do not enter the product repository by default.
- Added `scripts/release-preflight.sh` and `make release-preflight` to make local RC checks and full commercial release blockers executable.
- Added CycloneDX SBOM generation and `scripts/compliance-audit.sh`; release packages now include `SBOM.cdx.json` and `docs/compliance-report.json`.
- Added `deny.toml`; tag/manual release workflow installs `cargo-audit` and `cargo-deny` and runs full compliance mode.
- Added `scripts/install-compliance-tools.sh` so full compliance tooling can be installed under ignored `target/` paths instead of relying on global Cargo state.
- Full local compliance mode now passes with project-local `cargo-audit` and `cargo-deny`; current RustSec output has no vulnerability errors and retains six allowed warning advisories for upstream-only maintenance/unsoundness tracking.
- Direct-connect Unix socket support is implemented for Unix builds through `cc+unix:///path/to/socket` and `kiana server --unix <path>`; Windows builds report the platform boundary explicitly.
- Service-layer API key lookup now reuses the same effective config stack as CLI auth/config commands, including managed settings overrides and empty-value filtering.
- Release smoke now enforces locked/offline Cargo test and release-build gates, and successful smoke subcommands keep their real exit status instead of relying only on output matching.
- Release packaging now emits distribution manifest dry-runs from the same package checksums, including an enterprise offline manifest, a pinned `kiana.enterprise.offline-manifest.v1` schema, and explicit package-channel blockers when a target cannot be published yet.
- Windows release packaging now emits a portable ZIP plus checksum beside the tarball, and combined distribution manifest generation emits winget YAML when that ZIP is present.
- Release package lifecycle smoke now verifies staged tarball checksums, extraction, install, repeated install, rollback restore, uninstall behavior, installed model smoke tool reporting, and enterprise offline manifest artifact/checksum coverage in a temporary install root.
- Added a `commercial` permission profile and doctor-level `commercial_security` readiness report for strict sandbox plus approval posture validation.
- Added `kiana doctor --json` with a pinned `kiana.doctor.v1` schema and release smoke validation for schema-critical readiness fields.
- `kiana doctor --json` now reports platform-aware commercial security posture with `platform`, `isolation`, and `controls`; Linux keeps the strict `bwrap` sandbox gate, while Windows/macOS report the approval plus shell exec-policy model instead of a false `bwrap` blocker.
- `kiana doctor --json` now reports a reference capability matrix for runtime/session, tool/MCP, security, provider, local-coding, product shell, remote release, and knowledge-agent surfaces, including local evidence and the remaining commercial risks that still require external proof.
- OAuth token files can now be used as redacted remote/bridge bearer-token sources and refreshed through their stored refresh token when command-based refresh is not configured.
- OAuth token files that carry `expires_at` are refreshed before remote-session and bridge live calls when the token is expired or within the proactive refresh window.
- OAuth token writes now use same-directory atomic replacement, flush before rename, and owner-only permissions on platforms with native permission bits.
- Remote code-session create, bridge credential fetch, smoke, and hydrate commands retry once after auth failure through the same remote refresh path used by websocket reconnects.
- Code-session auth retry now keys off typed 401/403 HTTP status from remote API errors, with text matching retained only as a compatibility fallback.
- Tag release workflow now enforces `v$(VERSION)` alignment, derives package manifest URLs from the active GitHub repository/tag, runs full release preflight before packaging, and recursively uploads manifest/compliance artifacts into the draft GitHub Release.
- Tag release workflow now runs package lifecycle smoke before signing, preserves signing/notarization proof artifacts, regenerates combined distribution manifests after all matrix artifacts are downloaded, stages accepted/live proof evidence into `dist/proofs/PROOF-MANIFEST.json`, and runs `scripts/verify-commercial-release-artifacts.sh` before drafting a GitHub Release.
- Full release preflight no longer accepts `KIANA_RELEASE_SIGNING_CONFIRMED`; commercial signing, notarization, Windows publishable packaging, and channel status must be represented by artifact proof files and manifests.
- Added `scripts/sign-release-artifacts.sh` and wired it into the tag release workflow; it requires an external `KIANA_SIGNING_COMMAND` and macOS notarization command/proof instead of accepting manual signing confirmation.
- Release signing now also requires a reviewed `KIANA_RELEASE_SIGNER` before generating proof files, so the default `external-release-signer` placeholder is rejected at signing time instead of only by the final artifact verifier.
- Commercial artifact signing and verification now require `KIANA_SIGNATURE_VERIFY_COMMAND`; the release signature proof records verified archive and binary-checksum signatures, and `scripts/release-signature-verification-smoke.sh` proves invalid signatures plus blocked channels, missing Windows publishable artifacts, and missing entitlement/product/ops proofs are rejected offline.
- macOS notarization proof now has a strict commercial contract: `kiana.macos-notarization.v1` requires target/archive binding, accepted timestamp, authority, notarization id, and no unrecognized fields.
- `kiana auth status --json` now exposes redacted OAuth token-file status, expiry, expiring/expired state, refreshability, and storage source; service-level OAuth token checks reject expired token files instead of using a placeholder false result.
- `kiana auth status --json` now exposes provider-aware readiness for Anthropic, OpenAI-compatible, Ollama, and fake providers, including redacted API key previews plus endpoint/model settings without exposing secrets.
- `/app/auth/status` exposes pinned `kiana.auth-status.v1` redacted account, OAuth, and provider readiness for Web/IDE clients without shelling out to `kiana auth status --json`.
- `/app/license/status` exposes pinned `kiana.license-status.v1` enterprise license, account, entitlement, and support readiness for Web/IDE clients without shelling out to `kiana license status --json`.
- Startup now records a first-start onboarding sentinel under `KIANA_HOME`, and TUI startup shows a one-time onboarding message when no API key or usable OAuth token is configured.
- TUI `/diff --json` and `/diff --last-assistant --json` now render structured diff previews with file lists, stats, and assistant patch snippets instead of raw JSON transcript output.
- TUI permission requests now carry structured approval panel state with tool, reason, blocked path, input preview, suggestions, and queue context while preserving `/allow` and `/deny` command handling.
- TUI headless render tests now verify the REPL approval panel keeps queued permission context, status shortcuts, and prompt input visible, and the resume picker renders filtered search results plus resume help.
- TUI `/history` now opens a searchable prompt-history picker backed by `KIANA_HOME/tui-history.jsonl`, dedupes newest-first prompt entries, restores the selected prompt as a draft, and has focused headless state/render coverage.
- TUI `/settings` now opens a read-only readiness hub for account/auth, provider/model capability, permissions, MCP, remote, and diagnostics status using the existing command registry, with headless render and runtime coverage; `/app/settings` exposes the same readiness sections for local app clients.
- `scripts/product-shell-smoke.sh` now names and runs the TUI approval, diff, onboarding, resume, prompt-history, and settings-readiness headless gates, and `scripts/release-smoke.sh` invokes it in full release mode.
- Direct-connect server now exposes a bearer-protected local app-server contract under `/app`, with read-only conversations, session event snapshots, settings, redacted secrets, sandbox, auth status, license status, model catalog, git-status, diff, checkpoint creation, checks dry-run/run, review dry-run/run, context-index, repo-map, context-search, and context-pack endpoints covered by `direct_connect_app_contract`.
- `/app/models/catalog` exposes the pinned offline `kiana.model-catalog.v1` provider/model catalog so Web/IDE clients can show model availability and provider status without shelling out to `kiana model catalog --json`.
- `/app/context/repo-map` exposes pinned `kiana.repo-map.v1` structural repository maps so Web/IDE clients can display language, symbol, and token-budget context without shelling out to `kiana context repo-map --json`.
- App-server conversations, session events, settings/readiness sections, secrets, sandbox, plugins, and git-status subresponses now have pinned JSON schemas and release/package gates beside the top-level `/app` contract schema; `/app/plugins` now proves enabled/disabled plugin summaries and component counts, including plugin `app.json` manifests.
- Runtime events now have a pinned `kiana-runtime-event.v1` JSON schema and packaged SDK contract doc; app-server event snapshots validate their embedded events against the same v1 payload variants.
- TUI transcript reduction and CLI text export now have full RuntimeEvent fixtures covering user, assistant, stream, tool call/result, permission request, session, error, and result events with stable roles, content, order, and exported text.
- Added `scripts/product-acceptance-report.sh` and the pinned `kiana.product-acceptance.v1` schema; local RC mode records headless product-shell, app-server, context-search, and context-cache-recovery coverage, while full commercial preflight requires an accepted target-customer workflow signoff file.
- Added `scripts/release-ops-report.sh` and the pinned `kiana.release-ops.v1` schema; local RC mode records missing operational signoff, while full commercial preflight requires an accepted private vulnerability route, credential owner, support contact, retention policy, and credential review proof.
- Added `scripts/platform-security-proof-report.sh` and the pinned `kiana.platform-security-proof.v1` schema; local RC mode records platform posture, while full commercial preflight requires accepted platform security proof from the real release runner.
- Added `scripts/commercial-release-blockers-report.sh` and the pinned `kiana.commercial-release-blockers.v1` schema; release managers can now generate a lightweight JSON/text list of remaining local and external commercial blockers without rerunning Cargo, live network smoke, signing, or publication gates.
- Commercial blocker reports now include owner assignment metadata, acceptance artifacts, verification commands, and handoff notes; `scripts/commercial-release-blockers-report.sh --handoff-md <path>` writes a release-owner Markdown handoff, and `scripts/commercial-release-handoff-smoke.sh` locks the handoff contract independently of the broader schema smoke.
- Commercial blocker reports now include `build.locked-offline-cache`, a local build/test blocker that detects when the runner's Cargo registry source cache is missing crates required by `Cargo.lock` before locked/offline release gates are trusted.
- Local RC product-acceptance preflight can now emit a schema-validated `headless_smoke_partial` report when the host's offline Cargo registry cache is incomplete; this keeps release ownership/reporting gates auditable while preserving strict full commercial acceptance through `scripts/product-acceptance-report.sh full`.
- Added `scripts/local-rc-evidence-report.sh` and the pinned `kiana.local-rc-evidence.v1` schema; release managers can now bundle local Linux RC artifacts, checksums, distribution manifests, generated proof JSON, lifecycle smoke status, and the current commercial blocker summary into `dist/proofs/local-rc-evidence.json` before external release owners take over.
- Added `scripts/stage-commercial-release-proofs.sh` and the pinned `kiana.commercial-proof-manifest.v1` schema; release managers can stage already accepted/live provider, remote, entitlement, product, release-ops, and platform-security proof files into the final `dist/proofs/**` layout with sha256 handoff evidence, without creating or accepting proof content.
- Added non-accepted commercial proof templates under `docs/proof-templates/` for product acceptance, entitlement, release operations, and platform security evidence; preflight verifies those examples remain `status=blocked` and are not mistaken for accepted release proofs.
- Added `scripts/validate-json-schema.py` and `scripts/schema-contract-smoke.sh`; local preflight and release packages now carry a dependency-free schema validation entrypoint for proof templates, the commercial blocker report, and the local RC evidence report.
- OpenAI-compatible provider support is now wired through the provider model profile, runner provider selection, and `kiana model list --json`; Chat Completions text and function-style tool loops are covered by local mock gates.
- Local Ollama provider support is now wired through the provider model profile, runner provider selection, and `kiana model list --json`; `/api/chat` text and `tools`/`tool_calls` loops are covered by local mock gates.
- Built-in provider registry metadata now centralizes provider display name, protocol, auth method, option aliases, env vars, default model/base URL, model source category, streaming mode, and live-smoke requirement for `model list`, `auth status`, and runner provider construction.
- Provider adapter standard tests now cover fake provider metadata, text response, unsupported-tool rejection, tool-call mapping, synthetic stream events, and provider error-code behavior as release-gated expectations for future adapters.
- `kiana model list --json` now distinguishes native provider streaming from synthetic stream-event fallback through `streaming_mode` and `native_streaming`, preventing OpenAI-compatible and Ollama paths from being presented as native streaming implementations.
- `kiana model catalog --json` now emits a pinned `kiana.model-catalog.v1` report that stays offline by default; `--live` opt-in refreshes OpenAI-compatible `/models` and Ollama `/api/tags` against configured endpoints, and release smoke validates the default offline JSON gate.
- `stream-json` init now exposes visible `skills` and `plugins` capability summaries, including plugin skills and plugin slash commands when enabled; disabled plugins remain reported as disabled while their skills/commands disappear from the active capability lists.
- `kiana skills` now has command-level coverage proving disabled plugin skills are hidden from JSON, show, path, and active skill-directory surfaces while plugin status remains auditable elsewhere.
- Stop-hook execution now has direct coverage proving disabled plugin `hooks/hooks.json` entries do not run, keeping plugin status/audit visibility separate from active hook behavior.
- `kiana mcp list/get` now has command-level coverage proving disabled plugin `.mcp.json` servers are hidden from user-facing MCP resource surfaces.
- Local and cached plugin marketplace manifests now expose plugin `interface` and policy metadata through `marketplace list --json`; `not_available` marketplace entries are rejected during install, and disabled plugin agents are hidden by the shared enabled-plugin root resolver.
- Plugin installs now write `.kiana-install-receipt.json` using the pinned `kiana.plugin-install-receipt.v1` schema, including source, marketplace policy, file count, stable content hashes, and a `stable-hash-v1` integrity seal; release smoke verifies both normal and tampered receipt states through the installed file and `plugin show`.
- Managed plugin install policy now supports administrator `allow`, `deny`, `allowMarketplaces`, `denyMarketplaces`, and `denyPathInstalls` rules through `KIANA_MANAGED_PLUGIN_POLICY_FILE` or the shared `KIANA_MANAGED_POLICY_FILE`, with deny rules taking precedence.
- `kiana context index --json`, `kiana context search <query> --json`, and `kiana context pack <query> --json` now provide deterministic local repository indexing, ranked lexical/path-aware search, root-scoped local artifact packs through `--root DIR`, and bounded snippet packs as the first knowledge/RAG foundation without external services; file-path prompts can return useful context even when the path tokens are absent from file contents, and release smoke plus package lifecycle smoke validate the pinned `kiana.context-index.v1`, `kiana.context-search.v1`, and `kiana.context-pack.v1` contracts.
- `kiana context index --json --cache <path>` and `/app/context/index?cache=true` now write persistent local context-index artifacts and report created, updated, or recovered status plus reused, added, changed, and removed file counts; corrupt cache JSON is rewritten with a schema-valid recovered report instead of blocking local knowledge indexing, and app clients can refresh the default workspace cache without shelling out.
- `/app/diff` now exposes the pinned `kiana.diff.v1` dirty-state report from the active workspace, so Web/IDE clients can display changed files plus staged and unstaged stats without shelling out to `kiana diff --json`.
- `/app/checkpoints` now exposes the pinned `kiana.checkpoint.v1` safety checkpoint creation report from the active workspace, so Web/IDE clients can capture staged, unstaged, and untracked edit state before destructive actions without shelling out to `kiana checkpoint --json`.
- `/app/checks/dry-run` now exposes the pinned `kiana.checks.dry_run.v1` local quality-gate plan from the active workspace, so Web/IDE clients can show `rustfmt`, `cargo_check`, `cargo_test`, and release-smoke readiness without shelling out to `kiana checks --dry-run --json`.
- `/app/checks` now exposes the pinned `kiana.checks.run.v1` isolated local quality-gate execution report from the active workspace, so Web/IDE clients can run `rustfmt`, `cargo_check`, `cargo_test`, and release-smoke gates without shelling out to `kiana checks --json`.
- `/app/review/dry-run` now exposes the pinned `kiana.review.dry_run.v1` local review plan from the active workspace, so Web/IDE clients can show dirty files, patch previews, and isolated-review steps without shelling out to `kiana review --dry-run --json`.
- `/app/review` now exposes the pinned `kiana.review.run.v1` local review execution report from the active workspace, so Web/IDE clients can run isolated checks, see pass/fail findings, and keep gate scripts out of the active worktree without shelling out to `kiana review --json`.
- Providers that do not support tools still default to an empty tool set when tools are not explicitly configured, so text prompts do not require a `--tools ""` workaround.
- `kiana model smoke --json` now emits a pinned `kiana.model-smoke.v1` provider smoke report; default release gates require fake provider text smoke to pass and report live provider skip reasons, while real Anthropic/OpenAI-compatible/Ollama smoke remains opt-in through `--live` or `KIANA_PROVIDER_SMOKE_LIVE=1`, and provider tool-call smoke remains opt-in through `--tools` or `KIANA_PROVIDER_SMOKE_TOOLS=1`.
- `scripts/provider-live-smoke.sh --required` now turns opt-in provider reports into commercial evidence by running live catalog plus live text/tool smoke and storing proof JSON under `target/live-smoke/provider/`.
- `scripts/remote-live-smoke.sh --required` now turns `remote-session code-session smoke --json` into a pinned `kiana.remote-code-session-smoke.v1` commercial evidence gate with proof JSON under `target/live-smoke/remote/`.
- The release workflow now writes live smoke, entitlement, product acceptance, release ops, and platform security proof artifacts under `dist/proofs/`, uploads them, stages `PROOF-MANIFEST.json` plus `HANDOFF.md`, and the commercial artifact verifier checks the manifest hashes and proof files before drafting a release.
- `kiana license status --json` now emits a pinned `kiana.license-status.v1` readiness report for offline enterprise license, account, entitlement, support contact, and managed-policy inputs without exposing raw license keys.
- Added `scripts/entitlement-proof-report.sh` and the pinned `kiana.entitlement-proof.v1` schema; full commercial preflight now requires an accepted active entitlement proof from the external license/account backend instead of relying only on local license configuration.
- Created a local initial product commit so `HEAD` now resolves and the product tree is clean.
- Fixed user documentation drift for Rust version, config path, model ID, model listing, and README reference links.

## Still Blocking Full Commercial Release

These are not solved by the local release gate and must be completed before claiming broad commercial availability:

- Publish the real Git repository and remote URL, then activate CI on real push/PR/release events.
- Push the local product commit to the real remote, then create an immutable release tag from a reviewed clean tree.
- Run `bash scripts/commercial-release-blockers-report.sh --json --handoff-md dist/proofs/COMMERCIAL-BLOCKERS-HANDOFF.md` as the handoff checklist for release owners, assign every reported blocking item, close them with real evidence, and run `bash scripts/stage-commercial-release-proofs.sh` on the combined artifact set before final commercial verification.
- After packaging and `scripts/package-lifecycle-smoke.sh`, run `KIANA_LOCAL_RC_LIFECYCLE_SMOKE_STATUS=passed bash scripts/local-rc-evidence-report.sh` from a clean tracked tree to produce the local RC evidence bundle; any `local_blockers` in that report must be closed before it is handed to external release owners.
- Activate the release workflow on the real remote, then add real `KIANA_SIGNING_COMMAND`, `KIANA_SIGNATURE_VERIFY_COMMAND`, macOS notarization command/proof where applicable, and an accepted `KIANA_RELEASE_OPS_FILE` so `scripts/sign-release-artifacts.sh`, `scripts/release-ops-report.sh full`, and `scripts/verify-commercial-release-artifacts.sh` can pass instead of blocking the draft release.
- Provide real install URLs and the `0.1.0` commercial GA distribution channels: GitHub Releases, Homebrew, winget, and enterprise offline bundle. apt/yum/npm-style channels remain post-GA candidates until matching generator and verifier gates exist.
- Implement package-manager publication; upgrade, rollback, uninstall, and changelog documentation plus local tarball lifecycle smoke now exists, and full commercial verification now fails while required channel manifests are blocked or missing from the combined artifact set.
- Run real remote/CCR/Session Ingress/token refresh end-to-end gates against production-like services; full preflight now invokes `scripts/remote-live-smoke.sh --required`, but credentials and hosted service readiness remain external blockers.
- Bring live terminal walkthroughs for TUI permission, diff, onboarding, resume, history, settings readiness, app-server client flows, and context-search workflows to reference-level usability; app-server now exposes plugin status, auth status, model catalog, bounded session event snapshots, context index, repo-map, context search, and context packs for clients, and full commercial preflight requires `scripts/product-acceptance-report.sh full`, but the accepted target-customer signoff file remains external.
- Finish provider ecosystem parity beyond Anthropic/fake/OpenAI-compatible/Ollama paths, including opt-in live model catalog and provider smoke reports against production-like credentials/daemons; OpenAI-compatible and Ollama tool calls plus live catalog parsing are covered by local mock gates, stream-json capability visibility now includes skills/plugins, provider credential/readiness metadata is visible through `auth status`, and full preflight now requires `scripts/provider-live-smoke.sh --required`, but real credentials/daemons are still external blockers.
- Add external signed plugin receipt policy where enterprise deployments require cryptographic provenance; local install receipts now cover auditability, administrator control, and tamper detection through stable integrity seals.
- Provide accepted platform-specific execution-isolation proof on real Windows, macOS, and Linux release runners; `scripts/platform-security-proof-report.sh full` and the commercial artifact verifier now enforce the proof contract, but release-runner acceptance files remain external inputs.
- Repeat dependency/compliance execution on the real release runners and complete third-party license/advisory signoff for the release record.
- Complete real enterprise account/license backend deployment and policy support; entitlement proof now has a release contract, but the real production backend proof file remains an external release input.

## Current Conclusion

This repository is now locally releasable as a source-build release candidate with package artifacts. It is not yet a complete commercial release until the external release infrastructure, live service gates, signing/compliance, and remaining reference-parity product gaps are closed.
