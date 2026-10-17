# EXT-27 extension visibility baseline

## Scope

EXT-27 adds one redacted, server-owned visibility projection for Skills, Plugins and the
extension lifecycle registry. CLI, Workbench, Web and Desktop adapters request the same
`ExtensionVisibilitySnapshot` through `DaemonHost`; they do not parse packages, load Skill bodies,
or decide authorization locally. `list`, `search` and metadata-only `inspect` are read-only
commands. Activation and revoke labels are intents and must return through `extension.manage`,
ControlPlane and the Capability Broker for a fresh admission check.

The snapshot binds `snapshot_id`, `generation`, source digest and a self digest. A surface may
render only bounded identity, summary, state, source trust, risk and action labels. It carries no
body text, package path, secret value, or internal command/entrypoint. `require_generation` fences
stale UI actions; the final registry, trust and approval checks remain server-side.

## Source slice

- `kiana-domain/src/extension_visibility.rs`: versioned redacted DTOs, digest and generation
  validation, bounded search/inspect and action intents.
- `kiana-daemon/src/extensions.rs`: joins the trust-filtered Skill catalog and lifecycle registry
  into one snapshot without returning package internals.
- `kiana-core/src/commands.rs`: bounded visibility query normalization and read-only risk mapping.
- `kiana-daemon/src/lib.rs` / `kiana-client/src/lib.rs`: authenticated DaemonHost and typed client
  query path.
- `kiana-entrypoints/src/extension_projection.rs`: shared CLI/Workbench/Web/Desktop adapter;
  Web exposes `/api/extensions`, Desktop labels the same route, and Workbench exposes `/extensions`.
- `kiana-domain/tests/ext27_visibility.rs` and `kiana-core/tests/ext27_visibility_guard.rs`:
  redaction, trust, identity/generation and cross-surface source guards.

## Completion slice (2026-10-17, CI-only)

The first slice shipped a DTO-level fixture and a source guard. This slice adds the behaviour
fixtures the Entry-projection gate asks for — one shared runtime projection, deny-first — plus the
missing read-only classification that made `search` unusable on the product path.

- `kiana-core/src/commands.rs`: `normalize_extension_command` classifies `list`, `search` and
  `inspect` as `RiskLevel::ReadOnly`. `search` was previously a mutation, so a bounded query was
  rejected with `extension_mutation_fields_required` (or forced a meaningless lifecycle binding).
  The daemon's `visibility_snapshot` query bound and the injected server-owned fields are unchanged.
- `kiana-entrypoints/tests/ext27_extension_visibility.rs` (new): drives the shared adapter
  `extension_visibility_on_host` against a real `DaemonHost`. Deny first: an unknown-trust project
  gets no projection — the raw envelope is `Blocked`/`project_untrusted` with `output: null`, and
  neither the envelope nor any adapter error echoes the Skill name, body token, locator or secret.
  Then a trusted project ships a Skill whose body carries body-token/locator/secret markers: bounded
  `search` returns zero entries for each marker, the matching search returns exactly one redacted
  descriptor, and the encoded projection contains none of the body/path/secret/`SKILL.md`/`.claude`
  markers. CLI, Workbench, Web and Desktop then read the identical `snapshot_id`, `generation`,
  `snapshot_digest` and entries; after the material changes, `require_generation` refuses the cached
  identity (`extension_visibility_snapshot_stale`) in both directions.
- `kiana-daemon/src/extensions.rs` (test module): `ext27_visibility_projection_is_redacted_bounded_and_role_scoped`
  proves the registry-side deny paths — an unknown-trust project's Skill is not loaded, the registry row
  is a `stale` diagnostic with `Unknown` trust and no `Activate` label (`ext27_visibility_action_denied`),
  an unlisted id has no intent (`extension_visibility_not_found`), bodies/locators/secrets are not
  searchable, an oversized query is `extension_visibility_query_invalid`, and role drift removes the row.
  `ext27_visibility_projection_identity_fences_stale_surface_caches` folds one more CAS generation and
  proves the cached id/generation pair is refused while the live pair still passes.
- `kiana-core/tests/ext27_visibility_guard.rs`: new `search_is_read_only_and_the_runtime_fixtures_stay_wired_to_the_lane`
  binds the read-only classification, both runtime fixtures, all four `EntryPointKind` labels, the four
  deny markers and the manual lane.
- `.github/workflows/ext27-extension-visibility.yml` (new, `workflow_dispatch`-only): fmt gate plus
  the domain fixture, the core guard, the daemon `--no-run` compile, the daemon `ext27_` library
  fixtures and the entrypoints cross-surface target.

Local tests/builds are not run in this session, so the proof level stays `source` and EXT-27 remains
🔄 until the lane returns a receipt with the Entry-projection gate covered.

## Close-out (2026-10-17, CI-only)

A follow-up static audit on branch `step/ext-27-20261017-2` (base master `5b08a38e`, which already
contains this completion slice via `97d840d4`) confirmed every source-level exit condition above is
present and CI-wired: the read-only classification, both daemon `ext27_` fixtures, both entrypoint
fixtures, the core guard and the manual-lane command list. The roadmap card and index row were
therefore flipped to ✅ (CI pending a receipt); the unknown-trust refusal is a policy `Denied`
(not a generic `Blocked`) with `project_untrusted`, and the four-surface parity fixture covers one
process and one trusted/untrusted project pair. The proof level stays `source`; the limitations
below are unchanged.

## Evidence (2026-10-17 completion slice)

```text
source_snapshot: branch step/ext-27-20261017-1 on master 824885bd7a61 + this commit
worktree_status: isolated worktree /workspace/kianacode-worktrees/EXT-27; unrelated worktrees untouched
command_argv: cargo fmt --all (pre-commit); bash scripts/ci/validate-doc-references.sh; git diff --check.
              No local cargo build/test/check/clippy/run, rustc, make, npm, smoke or acceptance script was run.
cwd·environment: /workspace/kianacode-worktrees/EXT-27; Linux; rustfmt only
fixture·cassette: GitHub-only .github/workflows/ext27-extension-visibility.yml runs the domain fixture,
              the core guard, the daemon ext27_ library fixtures and the entrypoints cross-surface target
exit_code: fmt/validator exit codes only; remote CI result intentionally unobserved, not awaited
status change: search reclassified read-only; runtime deny/parity fixtures added for daemon and
              entrypoints; manual lane restored; roadmap card and index stay 🔄 (CI-only)
proof-level change: source only — no local_behavior, durable, live or physical claim
limitations: UI cache persistence, cross-process registry recovery, approval/UX flows, package
              signing/revocation and real Hook/provider effects remain later EXT/UI steps; the
              four-surface parity fixture covers one process and one trusted/untrusted project pair
reviewer: Codex source review; no local runtime test reviewer; CI receipt pending
```

## Evidence (2026-09-24 source slice)

```text
source_snapshot: f1cce824 + EXT-27 source slice (rebased onto origin/master before commit)
worktree_status: isolated branch ext-27-skills-plugins-hooks-20260924 rebased onto origin/master=f1cce824; unrelated changes preserved
command_argv: git diff --check; GitHub Actions runs cargo fmt and EXT-27 fixtures (not awaited)
cwd·environment: /tmp/kiana-step-ext-27; Linux; no local test/build/check/clippy/smoke command run
fixture·cassette: GitHub-only ext27-visibility.yml runs domain fixture, core source guard and fmt
exit_code: local diff check only; remote CI result intentionally unobserved
status change: EXT-27 source slice and CI wiring implemented; roadmap card and index remain 🔄 pending CI result
proof-level change: source plus remote CI wiring; no local_behavior, durable, live or physical proof
limitations: UI cache persistence, cross-process registry recovery, mutation CAS/approval UX,
             package signing/revocation and runtime Hook/provider effects remain later EXT/UI steps
reviewer: Codex source review; no local runtime test reviewer
```
