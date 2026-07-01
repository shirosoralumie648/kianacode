# Runtime Session Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build Phase 1 from `docs/reference-migration-roadmap.md`: a typed runtime event schema and a JSONL session tree that keeps current SDK session files compatible.

**Architecture:** Add stable runtime events in `kiana-types`, then adapt `kiana-entrypoints/src/sdk.rs` so the existing JSON session store can be read unchanged while new writes also produce event-log entries. CLI, TUI, SDK, remote, and bridge should gradually render from the same event model instead of private `serde_json::Value` shapes.

**Tech Stack:** Rust 2021, serde/serde_json, uuid, existing `kiana-types` and `kiana-entrypoints` crates, current cargo workspace tests.

---

## File Structure

- Create `kiana-types/src/runtime.rs`: owns `RuntimeEvent`, event payload variants, session-tree IDs, and SDK-message adapter helpers.
- Modify `kiana-types/src/lib.rs`: exports the runtime module and public schema types.
- Create `kiana-types/tests/runtime_event_schema.rs`: JSON golden tests for every required runtime event variant and SDK-message adapter behavior.
- Modify `kiana-entrypoints/src/sdk.rs`: introduces JSONL session-tree persistence while preserving current `<session_id>.json` compatibility files.
- Modify `kiana-commands/src/session.rs`: mirrors JSONL session-tree appends for local session slash-command writes.
- Modify `kiana-entrypoints/tests/cli_session.rs`: proves session CLI commands keep working with legacy files and can inspect sessions that have JSONL events.
- Modify `kiana-entrypoints/tests/cli_resume.rs`: proves resume and continue still work after JSONL session-tree writes.
- Modify `docs/reference-feature-matrix.md`: records Phase 1 owner paths, tests, and risk notes as implementation begins.

## Task 1: Runtime Event Schema

**Files:**
- Create: `kiana-types/tests/runtime_event_schema.rs`
- Create: `kiana-types/src/runtime.rs`
- Modify: `kiana-types/src/lib.rs`

- [ ] **Step 1: Write the failing JSON golden tests**

Create `kiana-types/tests/runtime_event_schema.rs` with:

```rust
use kiana_types::{
    sdk_message_to_runtime_event, MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent,
    RuntimeEventPayload, RuntimeResultEvent, RuntimeSessionEvent, RuntimeStreamDeltaEvent,
    RuntimeToolCallEvent, RuntimeToolResultEvent, RuntimePermissionRequestEvent,
};
use serde_json::json;

#[test]
fn runtime_event_serializes_user_message_with_parent_turn() {
    let event = RuntimeEvent::new(
        "evt-1",
        "session-1",
        "turn-2",
        Some("turn-1".to_string()),
        7,
        "2026-06-23T00:00:00Z",
        RuntimeEventPayload::UserMessage(MessageRuntimeEvent {
            message: json!({"role": "user", "content": "hello"}),
        }),
    );

    assert_eq!(
        serde_json::to_value(event).unwrap(),
        json!({
            "event_id": "evt-1",
            "session_id": "session-1",
            "turn_id": "turn-2",
            "parent_turn_id": "turn-1",
            "sequence": 7,
            "timestamp": "2026-06-23T00:00:00Z",
            "type": "user_message",
            "message": {"role": "user", "content": "hello"}
        })
    );
}

#[test]
fn runtime_event_schema_covers_required_payloads() {
    let payloads = vec![
        RuntimeEventPayload::AssistantMessage(MessageRuntimeEvent {
            message: json!({"role": "assistant", "content": [{"type": "text", "text": "hi"}]}),
        }),
        RuntimeEventPayload::StreamDelta(RuntimeStreamDeltaEvent {
            delta: json!({"type": "text_delta", "text": "hi"}),
        }),
        RuntimeEventPayload::ToolCall(RuntimeToolCallEvent {
            tool_call_id: "toolu_1".to_string(),
            name: "Read".to_string(),
            input: json!({"file_path": "README.md"}),
        }),
        RuntimeEventPayload::ToolResult(RuntimeToolResultEvent {
            tool_call_id: "toolu_1".to_string(),
            name: Some("Read".to_string()),
            is_error: false,
            content: json!({"text": "content"}),
        }),
        RuntimeEventPayload::PermissionRequest(RuntimePermissionRequestEvent {
            request_id: "perm-1".to_string(),
            tool_name: "Bash".to_string(),
            action: "run".to_string(),
            input: json!({"command": "cargo test"}),
            reason: Some("command requires approval".to_string()),
        }),
        RuntimeEventPayload::SessionEvent(RuntimeSessionEvent {
            subtype: "created".to_string(),
            message: Some("session created".to_string()),
            metadata: json!({"cwd": "/work"}),
        }),
        RuntimeEventPayload::Error(RuntimeErrorEvent {
            code: Some("api_error".to_string()),
            message: "request failed".to_string(),
            details: json!({"status": 500}),
        }),
        RuntimeEventPayload::Result(RuntimeResultEvent {
            status: "completed".to_string(),
            assistant_text: Some("done".to_string()),
            metadata: json!({"iterations": 2}),
        }),
    ];

    let types: Vec<_> = payloads
        .into_iter()
        .map(|payload| {
            let event = RuntimeEvent::new(
                "evt",
                "session",
                "turn",
                None,
                1,
                "2026-06-23T00:00:00Z",
                payload,
            );
            serde_json::to_value(event).unwrap()["type"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();

    assert_eq!(
        types,
        vec![
            "assistant_message",
            "stream_delta",
            "tool_call",
            "tool_result",
            "permission_request",
            "session_event",
            "error",
            "result",
        ]
    );
}

#[test]
fn sdk_message_adapter_maps_roles_to_runtime_events() {
    let user = sdk_message_to_runtime_event(
        "session-1",
        "turn-1",
        None,
        0,
        "2026-06-23T00:00:00Z",
        json!({"role": "user", "content": "question"}),
    );
    let assistant = sdk_message_to_runtime_event(
        "session-1",
        "turn-2",
        Some("turn-1".to_string()),
        1,
        "2026-06-23T00:00:01Z",
        json!({"role": "assistant", "content": [{"type": "text", "text": "answer"}]}),
    );

    assert_eq!(serde_json::to_value(&user).unwrap()["type"], "user_message");
    assert_eq!(serde_json::to_value(&assistant).unwrap()["type"], "assistant_message");
    assert_eq!(assistant.parent_turn_id.as_deref(), Some("turn-1"));
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cargo test -p kiana-types --test runtime_event_schema
```

Expected: FAIL with unresolved imports such as `no RuntimeEvent in the root`.

- [ ] **Step 3: Implement the schema**

Create `kiana-types/src/runtime.rs` with public serde types matching the tests. Use `#[serde(tag = "type", rename_all = "snake_case")]` on `RuntimeEventPayload`, keep `parent_turn_id` optional, and make `sdk_message_to_runtime_event` choose `UserMessage`, `AssistantMessage`, or `SessionEvent` from the SDK message role.

- [ ] **Step 4: Export the schema**

Add this to `kiana-types/src/lib.rs`:

```rust
pub mod runtime;

pub use runtime::{
    sdk_message_to_runtime_event, MessageRuntimeEvent, RuntimeErrorEvent, RuntimeEvent,
    RuntimeEventPayload, RuntimePermissionRequestEvent, RuntimeResultEvent, RuntimeSessionEvent,
    RuntimeStreamDeltaEvent, RuntimeToolCallEvent, RuntimeToolResultEvent,
};
```

- [ ] **Step 5: Run the focused tests**

Run:

```bash
cargo test -p kiana-types --test runtime_event_schema
cargo test -p kiana-types
```

Expected: all `kiana-types` tests pass.

## Task 2: JSONL Session Tree Compatibility

**Files:**
- Modify: `kiana-entrypoints/src/sdk.rs`
- Modify: `kiana-commands/src/session.rs`
- Modify: `kiana-entrypoints/tests/cli_session.rs`

- [ ] **Step 1: Add a failing SDK persistence test**

In the `sdk.rs` test module, add a test named `sdk_session_store_writes_runtime_event_jsonl_tree`. It should create a fixed session ID, append two prompts, and assert that `<root>/<session_id>/events.jsonl` exists with two JSON lines whose `type` values are `user_message`, sequences are `0` and `1`, and the second event has `parent_turn_id = "turn-0"`.

- [ ] **Step 2: Run the failing test**

Run:

```bash
cargo test -p kiana-entrypoints sdk_session_store_writes_runtime_event_jsonl_tree
```

Expected: FAIL because `events.jsonl` is not created.

- [ ] **Step 3: Add session-tree paths and append helper**

In `sdk.rs`, add `session_tree_dir(root, session_id)`, `session_events_file(root, session_id)`, `append_runtime_events_for_missing_messages`, and `read_existing_runtime_event_count`. These helpers must validate `session_id`, create `<root>/<session_id>/`, and append only messages not already represented in `events.jsonl`.

- [ ] **Step 4: Wire the append helper into writes**

Call the append helper after successful legacy `write_session` operations that add or replace messages. For replacement flows such as hydrate/compact, rebuild the event file from the current session so the compatibility JSON and runtime JSONL agree.

- [x] **Step 5: Preserve legacy reads**

Keep `read_session` loading `<root>/<session_id>.json` first so existing user sessions are not migrated destructively. When the legacy JSON file is absent, fall back to `<root>/<session_id>/events.jsonl` and reconstruct the SDK session shape for listing, showing, and resume.

- [x] **Step 6: Run focused session tests**

Run:

```bash
cargo test -p kiana-entrypoints sdk_session_store_writes_runtime_event_jsonl_tree
cargo test -p kiana-commands session::tests::
cargo test -p kiana-entrypoints cli_session
```

Expected: session tree test and existing CLI session tests pass.

## Task 3: Resume, Fork, Export, And Compact Event Coverage

**Files:**
- Modify: `kiana-entrypoints/src/sdk.rs`
- Modify: `kiana-entrypoints/tests/cli_resume.rs`
- Modify: `kiana-entrypoints/tests/cli_session.rs`

- [x] **Step 1: Add failing tests for fork and reply**

Extend CLI session tests so `session fork` creates a fork event tree with copied parent events and a new `parent_session_id`, and `session reply --record-only` appends exactly one new runtime event.

- [x] **Step 2: Add failing tests for resume**

Extend CLI resume tests so `--continue` and `--resume <id>` still select sessions written with event trees.

- [x] **Step 3: Implement event-tree copy and append behavior**

When forking, copy source messages into the forked compatibility JSON and rebuild the forked runtime event file with parent-turn linkage. When replying, append only the new message event after the legacy JSON write succeeds.

- [x] **Step 4: Run focused tests**

Run:

```bash
cargo test -p kiana-entrypoints cli_session
cargo test -p kiana-entrypoints cli_resume
```

Expected: all focused session and resume tests pass.

## Task 4: SDK/RPC Event Surface

**Files:**
- Modify: `kiana-entrypoints/src/sdk.rs`
- Modify: `kiana-entrypoints/src/runner.rs`
- Modify: `kiana-remote/src/sdk_message_adapter.rs`
- Modify: `kiana-bridge/src/sdk_message_adapter.rs`

- [x] **Step 1: Add failing adapter tests**

Add tests proving SDK, remote, and bridge adapters can emit `RuntimeEvent` values for user messages, assistant deltas, tool calls, tool results, permission requests, errors, and turn results.

- [x] **Step 2: Implement adapter functions without changing public CLI output**

Expose conversion helpers that return `RuntimeEvent` while keeping existing JSON output stable. This reduces release risk because CLI text and old SDK JSON remain compatible.

- [x] **Step 3: Run adapter tests**

Run:

```bash
cargo test -p kiana-entrypoints runtime_event
cargo test -p kiana-remote runtime_event
cargo test -p kiana-bridge runtime_event
```

Expected: adapter tests pass without network access.

## Task 5: Tracking And Verification

**Files:**
- Create: `docs/reference-feature-matrix.md`
- Modify: `docs/reference-migration-roadmap.md`

- [x] **Step 1: Add the feature matrix**

Create `docs/reference-feature-matrix.md` with columns `Domain`, `Reference`, `Kiana owner path`, `Status`, `Tests`, and `Risk notes`. Mark Phase 1 rows as `in progress` until the focused tests and default gate pass.

- [x] **Step 2: Update roadmap progress notes**

Add a short Phase 1 progress note to `docs/reference-migration-roadmap.md` only after tests prove the runtime schema and session-tree writer.

- [x] **Step 3: Run default verification**

Run:

```bash
cargo fmt --all --check
cargo test -p kiana-types
cargo test -p kiana-entrypoints cli_session cli_resume
bash scripts/release-smoke.sh
```

Expected: default release smoke passes or any failure is documented with exact failing command and stderr summary.
