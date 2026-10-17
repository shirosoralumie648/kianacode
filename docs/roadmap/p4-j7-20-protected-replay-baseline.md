# P4-J7-20 protected provider replay baseline

This source slice adds an explicit scope contract and a bounded in-process store for private
reasoning/signature material. References bind connection, protocol, model, route, prompt/tool/data
digests, source call and expiry; receipts and EventLog can carry only the reference metadata.
Deletion and expiry fail closed. The provider compiler validates a reference envelope and refuses
to dispatch when a protected artifact adapter is unavailable; it never guesses at private bytes.

GitHub Actions owns all fixtures. No local test, build, check, clippy or smoke command was run.
The proof ceiling is `source` plus CI wiring; live provider behavior remains open follow-up work.
No private reasoning bytes are written to the repository, EventLog or receipt.

## 2026-10-17 completion slice (CI-only)

The increments named as open in the first slice are now implemented at source level and wired into
the focused lane:

- `kiana-domain/src/protected_replay.rs`: the digest-only `PublicReasoningSummary`
  (`redaction = digest_only_not_lossless`, `resume()` refused with
  `public_replay_summary_is_not_resume_material`) is separated from the non-serializable
  `ProtectedReplayMaterial`; `ReasoningReplayKind` binds each dialect and travels on
  `ProtectedReplayRef.kind`; `ProtectedReplayStore` gains `revoke`/`revocation_reason`/
  `invalidate_route_change` (`model_switch`), TTL-widening rejection and
  `ProtectedHistoryBoundary` (continuations never enter the compaction input).
- `kiana-provider/src/replay.rs`: per-protocol capture (`capture_from_value`), verbatim injection
  planning (`plan_injection`), the bounded `ReplayCaptureBuffer` (disabled by default, so a
  deployment without an adapter keeps `provider_private_replay_not_configured`),
  `RingReplaySealer` (ChaCha20-Poly1305, random nonce, artifact metadata as AAD),
  `SealedReplayArtifact` with `sealed_sha256`, the ciphertext-only `SealedReplayArtifactStore`
  with export/import, and `ProtectedReplayService` (in-process store plus sealed store,
  implementing `ProtectedReplaySource`).
- Request/response codecs: `compile_with_protected_replay` resolves every declared reference
  before dispatch; private items are re-sent byte for byte before the public parts of the matching
  assistant turn; OpenAI Responses final values are stripped of private items before the public
  decoder runs; private blocks are never projected into public deltas. The DeepSeek-compatible
  dialect requires exactly one captured string item.

Fixtures (deny first): `kiana-provider/tests/p4_j7_20_protected_replay.rs`, the crate-internal
`p4_j7_20_protected_replay_tests` module in `kiana-provider/src/request.rs`,
`kiana-domain/tests/p4_j7_20_protected_replay.rs`, and the source guard
`kiana-core/tests/p4_j7_20_protected_replay_guard.rs` (private material must stay outside serde).
The focused lane `.github/workflows/p4-j7-20-protected-replay.yml` runs `cargo fmt --all --check`
plus the domain, provider integration, provider library and guard targets; the sharded `ci.yml`
covers the whole `kiana-provider` crate (`targets=null`) and already lists both domain/core
targets, so `scripts/ci/test-shards.json` is unchanged.

Still open: transport-level streaming capture wiring, the daemon protected-artifact adapter, live
provider behavior and any durable-store or billing claim. Only `capture_from_value` (non-streaming
envelope) is a public capture entry point today; streaming capture is exercised through the
crate-internal `Accumulator::push_capturing`/`finish_capturing` fixtures. The key is supplied by
the caller and never written to the repository, an event or a receipt; sealed envelopes carry
ciphertext only.
