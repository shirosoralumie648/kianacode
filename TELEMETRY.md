# Telemetry

## Current State

Kiana does not currently enable automatic product analytics by default. The codebase uses local tracing/logging primitives and networked features only when users configure and invoke them.

## Explicit Network Features

The following features can send data to configured services:

- Anthropic-compatible model provider calls.
- Web fetch/search tools.
- HTTP, SSE, WebSocket, and stdio MCP integrations.
- Remote sessions, bridge workers, CCR v2 worker events, and live smoke tests.
- Plugin and marketplace installation from remote sources.

## Commercial Release Requirements

Before enabling any telemetry in a commercial release, the implementation must provide:

- Opt-in consent by default unless a managed enterprise policy explicitly enables it.
- A documented event schema and retention policy.
- Redaction of prompts, secrets, file contents, headers, and local paths unless users explicitly include them.
- A visible disable switch in config and environment variables.
- Tests proving telemetry remains disabled in the default configuration.
- Privacy documentation linked from README, installer output, and release notes.

## Current Verification

The local release smoke gate does not require telemetry credentials and should continue to pass without any analytics endpoint.
