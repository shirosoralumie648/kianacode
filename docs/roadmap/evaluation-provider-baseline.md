# EQ-10 Evaluation Provider Baseline

## Scope

EQ-10 adds `kiana-daemon::eval_runtime::{FakeProviderAdapter, FakeProviderScenario}` as a
strict, deterministic implementation of the existing `ModelClient` port.  A cassette can return
a complete reply, replay text in bounded chunks, return a tool-call declaration, or surface an
explicit malformed-stream/provider error.  The adapter records call count so fixtures can prove
that normalization does not create hidden retries or execute a tool.

The scenario is an offline value object: it has no endpoint, credential, environment lookup,
network client, runner, Broker, scheduler, or background task.  Tool calls remain model output;
they are not dispatched by the fake provider.  Stream callbacks receive only typed `ModelDelta`
values and the complete response remains the returned aggregate.

## Evidence and limits

- `kiana-daemon/tests/eq10_fake_provider.rs` covers complete, chunked stream, tool-call,
  malformed/error, strict unknown-field, and bounded scenario behavior.
- `kiana-core/tests/eq10_fake_provider_guard.rs` protects the offline provider-port boundary.
  GitHub Actions runs the fixtures; local tests are intentionally not executed.

This slice is `feature_status=implemented`, `proof_level=source`: it does not yet connect the
fake adapter to a DaemonHost target, deny-by-default Broker, fixture store, event/receipt capture,
fault/restart plan, or any real/live/physical provider or external effect.  Those remain EQ-11+.
