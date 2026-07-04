# Kiana Runtime Event Contract

Kiana session logs, TUI replay, bridge adapters, remote adapters, and local
app-server event snapshots use the `RuntimeEvent` JSON shape as their stable
event contract.

The pinned JSON schema is:

- `docs/schemas/kiana-runtime-event.v1.schema.json`

Every event has these common fields:

- `event_id`: stable event identifier within the session tree
- `session_id`: owning session
- `turn_id`: owning turn
- `parent_turn_id`: optional previous turn linkage
- `sequence`: zero-based event order
- `timestamp`: producer timestamp
- `type`: one of the pinned payload variants

Payload variants:

- `user_message`: includes `message`
- `assistant_message`: includes `message`
- `stream_delta`: includes `delta`
- `tool_call`: includes `tool_call_id`, `name`, and `input`
- `tool_result`: includes `tool_call_id`, `is_error`, and `content`; may include `changed_files` for tool-produced file edits and `error` for structured tool failures
- `permission_request`: includes `request_id`, `tool_name`, `action`, and `input`
- `session_event`: includes `subtype` and `metadata`
- `error`: includes `message` and `details`
- `result`: includes `status`, `stop_reason`, and `metadata`

Compatibility policy:

- New producers must keep existing fields stable for `v1`.
- New optional fields may be added only with a schema update and release note.
- Terminal result producers must set `stop_reason`; use `model_stop` for normal
  model completion when a more specific reason is unavailable.
- Product clients should ignore unknown future event `type` values only after a
  schema version bump; `v1` uses a closed enum.
- Secret values must not be emitted in `input`, `content`, `metadata`, or
  `details`.

Validation:

```bash
bash scripts/schema-contract-smoke.sh
cargo test -p kiana-types --locked --offline runtime_event_schema
```
