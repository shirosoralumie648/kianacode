# H20 immutable StepContext and explainable ContextPlan baseline

## Delivered source slice

- `ContextPlan` compiles PromptBundle sections and server-supplied candidates into deterministic
  Product/Context layers with source, permission scope, revision, priority, estimate, inclusion and
  omission reason.
- Product content must originate from a Prompt source; workspace/Memory/tool material remains
  Context and cannot enter the privileged layer. Budget overflow is explicit rather than silent.
- `ResolvedStepContext` binds the plan digest, StepIdentity, model profile, route/catalog digests,
  workspace revision, data epoch, rendered prompt digest and TokenBudget into one request digest.
- Rechecking route/catalog/workspace/data bindings rejects drift between estimate and provider send;
  the runner source guard records its existing PromptBundle/StepIdentity/ModelRequest path.

## Boundary and proof ceiling

The domain contract does not query files, Memory or providers. Adapter-side retrieval, provider
wire serialization, real tokenization and live prompt execution remain CI/integration work; no
external business outcome or production tokenizer accuracy is claimed.
