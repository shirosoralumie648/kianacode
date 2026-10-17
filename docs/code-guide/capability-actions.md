# Capability Action Contracts

## Scope

`kiana-domain/src/actions.rs` defines the canonical contract for named capability operations. It is used by three parts of the main execution path:

```text
model tool call
  -> Runner maps the fixed tool into CapabilityRequest and calculates a minimum risk
  -> Core removes caller supplied authority fields, stamps trusted identity, runs hooks,
     normalizes again, attaches ExecutionScope and creates PreparedAction
  -> policy / Gate / approval / DispatchPermit checks
  -> Broker verifies the prepared request and invokes the registered handler
  -> normalize_capability_result checks the handler result before Core records/delivers it
```

The Domain functions normalize and validate data. They do not authorize a user, issue a grant, approve an operation, create a dispatch permit, or perform the operation. `PreparedAction` is a consistent, normalized request snapshot; reaching its constructor is not permission to call a handler.

## Why This File Is Not Annotated Inline

`capability_action_catalog_digest()` includes the complete source text of `actions.rs` through `include_str!("actions.rs")`. The same digest also includes `operation_catalog.rs` and `tool_catalog.rs`. Adding even a comment to any of those files changes the computed catalog digest. That value is copied into prepared actions and execution scopes, so an inline comment would change observable identity data despite leaving Rust control flow untouched.

To preserve the requested no-behavior-change constraint, this guide carries the beginner explanation for those digest inputs. Do not add comments to the three hashed source files unless the project intentionally accepts and reviews a catalog-digest change.

## Catalog And Preparation

`CapabilityActionDescriptor` is the description of one registered operation. `operation` and `binding_version` identify the operation implementation contract; `capability` and `minimum_risk` provide the declared capability class and base risk; argument/result schemas describe the JSON boundary; resource fields name the request values relevant to resource scope; effect, cancellation, reconciliation and idempotency fields state how the operation is expected to interact with external effects. These strings are metadata, not proof that a handler obeys them.

`validate_action_catalog()` walks the fixed `ACTION_OPERATIONS` list. It rejects an empty or duplicate operation, missing or mismatched descriptors, empty/duplicate resource names, missing effect lifecycle metadata, invalid schemas, unspecified `additionalProperties`, and invalid tool-to-action bindings. This is deliberately fail-closed: an operation that has no complete description cannot silently inherit generic read-only behavior.

`capability_action_descriptor(operation)` first maps a model-tool name or accepted operation alias to its canonical operation specification. It then uses the model tool parameter schema when one exists, otherwise the operator schema, and attaches the common result schema and action metadata. It explicitly sets `additionalProperties` to `true` for compatibility with server-stamped request metadata; this does not replace the later Core authority stamping and scope checks.

`PreparedAction::new(request)` is the normalization boundary used by Core after the request context, hooks and effective scope have been assembled. It validates the closed catalog, normalizes the request, computes three values, and validates the completed object:

- `input_digest` identifies the canonical operation/capability/argument input.
- `catalog_digest` pins the current operation and parser contract version.
- `digest` covers the catalog digest and the complete `CapabilityRequest`.

Its fields are private and it only implements serialization, not deserialization. The accessors borrow the pinned values; `into_request()` returns the normalized request to the caller. `validate()` recomputes catalog and request digests and re-runs normalization to detect drift. It does not check current user permissions or produce a Broker permit.

## Operation And Risk Rules

`canonical_action_operation()` accepts only names recognized by the model-operation or operation registry and returns their canonical name. Unknown names return `None`; there is no fuzzy matching. `operator_only_action()` reads the operation's registry flag and is metadata lookup only.

`capability_action_minimum_risk()` returns the risk floor for the operation and its arguments:

- Static operations use the catalog's declared minimum.
- `SandboxMode` treats `workspace-write` as `LocalWrite`, `read-only` or missing sandbox as `ReadOnly`, and rejects other values.
- `WorkspaceTransactionAction` allows `list`/`inspect` at `ReadOnly`, requires `Critical` for `recover`/`rollback`, and rejects other action names.
- `ListOrInspectExternal` keeps `list`/`inspect` read-only and classifies other actions as external effects.
- `ExtensionVisibilityAction` (`extension.manage`) keeps `list`/`search`/`inspect` read-only — the three actions read one server-owned, trust-filtered visibility projection — and classifies every state-changing action as an external effect.
- `ListExternal` keeps only `list` read-only; every other action is classified as an external effect.
- `ConnectorInvocation` delegates argument-specific classification to the connector risk helper.

Risk ordering is `ReadOnly < LocalWrite < ExternalSideEffect < Critical`. `capability_action_contract()` additionally requires an operator-only action to be outside a Cell and to carry `operator_authorized=true`, requires the request capability kind to match the registered operation, and rejects a supplied risk below the computed floor. The operator marker is still only a request contract; Core supplies the trusted caller context and makes the actual authorization decision.

## Request Normalization

`normalize_capability_action()` mutates a request into the one shape used for hashing, approval presentation and later verification. Its order matters:

1. It checks JSON size/complexity limits, resolves the operation alias, requires an object for arguments, and removes `null` members. Duplicate object keys must be rejected earlier while parsing raw JSON; once raw text has collapsed into a `Value`, this function cannot recover duplicate-key evidence.
2. It normalizes only fields registered as paths for that operation family (`context.*`, `apply_patch`, and `local.package`). Other string values are not guessed to be paths. This lexical/domain normalization does not replace Core's WorkPacket/path-scope checks or Daemon filesystem containment.
3. For `shell.exec` and `process.start`, it supplies `workdir="."` if absent and accepts either a nonempty command string or a nonempty argv whose executable is nonblank. Command values containing NUL are rejected. The Daemon later converts the command into its shell execution plan and enforces actual sandbox and working-directory boundaries.
4. For `mcp.call`, equal `tool` and legacy `tool_name` aliases collapse to one `tool`; conflicting aliases fail. Missing nested `arguments` becomes `{}`, then must be an object, and `tool` must be nonempty.
5. Supplied `timeout_ms`, `limit`, `max_tokens`, `max_bytes_per_file` and `max_snippet_lines` must be positive unsigned integers. The timeout is then capped at 60,000 ms (60 seconds), bounding a caller's requested wait before later layers apply their own deadlines.
6. The descriptor schema validates the normalized object. Operation-specific checks then require values such as package UUID, transaction ID/resolution, patch text, memory query/write fields, or a restore snapshot object.
7. Finally, the operation/capability/risk contract is checked so a low risk label or wrong capability kind cannot pass merely because its JSON shape is valid.

Failures are stable string codes such as `action_operation_unknown`, `action_arguments_object_required`, `action_command_required`, `action_tool_alias_conflict`, `action_numeric_argument_invalid`, `action_arguments_invalid`, and `action_risk_downgrade`. Core maps these into its structured action error path; the request must not continue to policy/dispatch as if normalization succeeded.

`require_text()` checks that a named value is a string with non-whitespace content. It does not trim or rewrite the actual value. Path canonicalization and other field transformations happen in their separate, explicitly selected branches.

## Digests And Result Boundary

`capability_action_digest()` hashes both the catalog digest and the full request. It is appropriate when the entire server-prepared request identity matters. `canonical_action_input_digest()` hashes only canonical operation, handler binding version, capability kind and the complete `arguments` object; the top-level `request_id` is excluded. The distinction matters: the Runner currently places `call_id`, `sandbox` and `project_root` inside `arguments`, so those nested values are included in the canonical input digest. This qualifies the baseline's broad statement that correlation IDs are excluded: top-level request identity is excluded, but nested `call_id` is not excluded by this function.

Both ordinary SHA-256 digests are unkeyed fingerprints. They let callers compare content or detect drift; they cannot authenticate who supplied the request or prove a handler effect.

`capability_action_catalog_digest()` combines the schema label, current descriptors, source text for the operation registry/normalizer/schema validator, and the model-tool catalog digest. This intentionally makes changes to parser semantics invalidate previously prepared requests even if their outward JSON schemas are unchanged. The `include_str!` source inclusion is why inline comments would change this digest.

`normalize_capability_result(expected_request_id, result)` is called after a handler returns and again on several Core cancellation/finalization paths. It forces result identity back to the expected request, checks JSON and evidence-reference size limits, corrects inconsistent success claims (nonzero subprocess exit, unconfirmed effect, embedded error, cancellation without stop proof), fills stable error/retry/reconciliation fields, and derives a compact outcome object for downstream receipts/events. It is a pure value transformation; it does not retry, stop a process, reconcile an external effect, or decide whether retry is safe by itself.

One review item remains: when the handler result has a mismatched `request_id`, the early return copies `evidence_refs` before the later limit check. That branch therefore does not apply this function's 256-reference/4096-byte-per-reference check. This is a potential oversized-metadata gap for downstream consumers; it is recorded for review and was not changed in this annotation-only task.

## Tests And Proof Limits

`kiana-domain/tests/cp03_action_contract.rs` exercises catalog completeness, forged risk downgrade, capability-kind matching, dynamic sandbox/transaction risk floors, prepared-action normalization and malformed input rejection. It constructs Domain values directly; it does not run Core policy, approval, Broker dispatch or an OS handler.

`kiana-domain/tests/cap02_input.rs` covers canonical digest equality/difference, MCP alias conflicts, parser/schema depth and bounds, duplicate JSON keys, and the fact that caller-supplied authority fields alter request input rather than become server authority. The source guard `kiana-core/tests/cap02_input_guard.rs` checks expected markers across Runner/Core/Broker/Daemon/Provider source and workflow text; it is not an execution test. The CAP-02 baseline records the target as `partial/source`, with its remote receipt tied to specific source snapshots rather than the present worktree as a whole.

Current production path evidence comes from `kiana-runner/src/tools.rs`, `kiana-core/src/capabilities.rs`, and `kiana-capability-broker/src/lib.rs`. The large Core, Runner, Broker and Daemon files are concurrently modified in the shared worktree and were read only for this guide; they were not edited here.
