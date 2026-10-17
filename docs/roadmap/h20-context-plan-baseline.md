# H20 immutable StepContext and explainable ContextPlan baseline

## Delivered source slice

- `ContextPlan` compiles PromptBundle sections and server-supplied candidates into deterministic
  Product/Context layers with source, permission scope, revision, priority, estimate, inclusion and
  omission reason.
- Product content must originate from a Prompt source; workspace/Memory/tool material remains
  Context and cannot enter the privileged layer. Budget overflow is explicit rather than silent.
- `ResolvedStepContext` binds the plan digest, StepIdentity, model profile, route/catalog digests,
  workspace revision, data epoch, rendered prompt digest and TokenBudget into one request digest.
- `kiana-runner/src/step_context.rs` compiles one per-step snapshot from the frozen PromptBundle,
  the message snapshot and the final wire budget, then rechecks route/catalog/workspace/data
  bindings before budget reservation and send (`step_context_fence:resolved_context_route_changed`
  on drift). The explanation object (`kiana.step-context-explanation.v1`) records digests and
  omission reasons without message text and is attached to `ModelTurn` metadata; legacy
  text-instruction runs without a bundle keep their previous path.

## 2026-10-17 completion slice (CI-only)

Named acceptance fixtures in `kiana-runner/tests/h20_context_behavior.rs`, driving a real
`KianaHarness` and a capturing fake `ModelClient` (no network, no tool execution):

- Deny first: `h20_untrusted_project_instruction_never_enters_privileged_section` — the project
  instruction stays in the Context layer of the wire request; claiming Product authority for a
  workspace source fails closed with `context_product_source_untrusted`.
- Deny first: `h20_route_change_between_estimate_and_send_is_fenced` — the second step's route is
  rechecked against the pinned digest before budget reservation/send and is fenced with
  `step_context_fence:resolved_context_route_changed`; the fenced request never reaches the
  provider.
- Success: `h20_same_step_snapshot_renders_same_request_and_provenance` — the prepared request is
  byte-identical to the sent request, and recompiling the recorded explanation from the same
  inputs reproduces plan/request digests, workspace/data bindings and Task-layer provenance.

## Boundary and proof ceiling

The domain contract does not query files, Memory or providers. Adapter-side retrieval, provider
wire serialization, real tokenization and live prompt execution remain CI/integration work; no
external business outcome or production tokenizer accuracy is claimed.
`ScopeSet` is a runner-side placeholder (real authorization and scope intersection remain in
ControlPlane); workspace/data bindings are content-derived local digests, not EventStore
revision/epoch authority.

The manual focused diagnostic workflow [`h20-context-plan-diagnostic.yml`](../../.github/workflows/h20-context-plan-diagnostic.yml)
runs the domain ContextPlan target, the runner source guard and the runner behavior fixtures in one
job (rustfmt check, per-fixture `--exact` runs and `--no-run` compile steps). An unawaited remote
run does not raise the proof level.
