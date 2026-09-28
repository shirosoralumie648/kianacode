# SC-35 evidence manifest and fixture/cassette registry baseline

> Snapshot date: 2026-09-28. This slice owns the shape an evidence record has to have before it is
> allowed to count as evidence. Local Cargo **test execution** was not performed; GitHub Actions owns
> fixtures and the workspace gate.

## Scope and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-35`](security-compliance.md#step-sc-35) |
| code landing | `kiana-core/src/evidence_manifest.rs`, registered by `kiana-core/src/lib.rs` |
| script | `scripts/validate-evidence-manifest.sh`, wired into `.github/workflows/ci.yml` |
| fixtures | `kiana-core/tests/sc35_evidence_manifest.rs`, `kiana-core/tests/sc35_evidence_manifest_guard.rs`, `scripts/fixtures/sc35-evidence-manifest.example.json` |
| feature_status | `partial` — the shape is enforced; nothing loads a manifest into a gate yet |
| proof_level | `source`; no local_behavior/durable/live/physical promotion |

## The card's six failure modes, and where each one is refused

| Card failure | Refusal |
|---|---|
| 命令 missing | `evidence_manifest_command_required` — argv is a `Vec`, not a shell string, because a string can be re-split and a re-split command is not the command that ran |
| 环境 missing | `evidence_manifest_environment_required`, plus `_duplicate` for a repeated key |
| 源码 missing | `evidence_manifest_source_snapshot_invalid` — 7..=64 lowercase hex, so a short sha and a full sha both pass and a sentence does not |
| fixture/cassette missing | `evidence_manifest_fixture_required`, or a stated `fixture_absent_reason` when there genuinely is none |
| 限制 missing | `evidence_manifest_limitations_required` — an empty limitations list is never accepted |
| 证据被手工改写 | `evidence_manifest_digest_mismatch` — the seal is recomputed from the record's own content |

`exit_code` is an `Option<i64>` rather than a sentinel. "The run was cancelled, or never started" is
a real state, and encoding it as `-1` would make it indistinguishable from a process that exited
non-zero.

## A claim may not be stronger than its subject

`proof_level` is an ordered ladder — `source < local_behavior < durable < live < physical` — and the
manifest refuses a combination that cannot be true: a `not_supported`, `target` or `deferred` thing
cannot be carrying `durable`, `live` or `physical`, because there is nothing there to have been
proven (`evidence_manifest_proof_above_feature`). This is the machine-checkable form of the rule the
repository keeps restating in prose, and it is the reason `Ord`/`PartialOrd` are derived on both
enums rather than left as strings.

## A citation is a binding, not a filename

`validate_against` checks every cited fixture against a **supplied** `FixtureRegistry`. Three
refusals matter:

- `evidence_manifest_fixture_unknown` — the file is not in the registry at all;
- `evidence_manifest_fixture_digest_mismatch` — the bytes moved after the claim was written. This is
  the common case and the reason the registry exists: a cassette is still a perfectly valid file
  after the source changes, and from that moment it is no longer evidence for anything;
- `evidence_manifest_fixture_kind_conflict` — a cassette relabelled as a fixture, which would make a
  stale recording look like a designed input.

Paths are repository-relative. An absolute path is machine-specific, and a manifest that means
different things on two developers' machines is not comparable evidence.

## A sequence of claims is a chain, not a pile

`verify_chain` links a manifest to the one it follows. A manifest that claims a predecessor but is
not supplied one is `evidence_manifest_chain_orphan` — that is what a forged chain looks like. A
predecessor whose digest does not match is `evidence_manifest_chain_broken`. A manifest that claims
to follow itself is `evidence_manifest_chain_identity`.

## Why there is also a shell script

The Rust module decides whether a manifest is *internally* consistent. It cannot police a JSON file
somebody committed by hand, so `scripts/validate-evidence-manifest.sh` checks a manifest file
against the same five fields plus the ladder rule, and the unified `ci.yml` runs it against
`scripts/fixtures/sc35-evidence-manifest.example.json` on every push. The split is deliberate: the
script checks shape, the module checks the seal and the registry binding, and the script says so
rather than pretending to recompute a digest it has no canonicaliser for.

## Honest limitations

No manifest in this repository is loaded by any gate. The registry is supplied by the caller, and
nothing here reads a filesystem to confirm a digest — deliberately, because a digest check that
depends on the machine running it is not a check on the claim. The validator is not wired into
`ControlPlane::handle_command`, into the release gate, or into the status ledger, so a manifest can
still be written that nobody validates. The example fixture carries placeholder digests and is
therefore an example, not evidence. There is no signature, no transparency log and no chain across
repositories; `previous_manifest_digest` links manifests within one sequence and nothing more. The
proof ceiling of this slice is `source`, which is the same ceiling it demands of the manifests it
checks.
