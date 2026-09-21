# PD-19 ContextIndex generation baseline

## Scope

PD-19 binds a context index generation to the canonical root, sorted file/content fingerprints,
ignore-rule digest, index configuration and deterministic embedding model. A single atomic envelope
publishes the Ready-only generation and its source manifest; readers never combine components from
different source fingerprints.

## Implemented source slice

- `ContextIndexSourceManifest` records root, generation, source fingerprint, ignore/config/model
  digests and explicit `Current`/`Stale` comparison.
- `build_context_index_generation` uses the existing `IndexGenerationState` transition contract
  and publishes only a Ready `IndexManifest` bound to the source manifest.
- `ContextIndexGenerationEnvelope` is written with temp-file sync + atomic rename and validates
  root/generation/digest bindings before read.
- Changed source content, ignore rules or model/config inputs become `Freshness::Stale`; callers
  must rebuild instead of reusing a previous generation.

## Evidence boundary

GitHub Actions is the test authority for the focused query fixtures and source guard. Local tests
are intentionally not run. This step proves source and remote-CI wiring only; it does not claim
multi-process projector leases, durable cache eviction, vector quality or live external effects.
