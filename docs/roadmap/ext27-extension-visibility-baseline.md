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

## Evidence

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
