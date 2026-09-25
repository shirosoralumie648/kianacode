# P4-J7-29 · Offline provider contract matrix baseline

This source slice adds the shared offline contract vocabulary for provider adapters. It does not
create a second provider client or retry loop and it does not promote a fake response to provider,
durable, live or physical evidence.

## Source contract

- `kiana-domain/src/provider_contracts.rs` defines a versioned `ProviderContractMatrix` whose
  every cell must explicitly carry fixture coverage, an unsupported preflight error, or an
  unclaimed `unknown` state. Duplicate protocol/capability cells and secret-bearing identifiers
  fail closed.
- `ProviderCassette` stores only protocol, source/mode, payload digest and redaction-profile
  digest. Replay is offline by construction; recording requires captured source plus explicit
  live opt-in, while synthetic cassettes cannot enter record mode.
- `ProviderFaultCorpus` requires one deterministic case for arbitrary chunking, out-of-order or
  duplicate events, truncation, frame limits, cancellation, incomplete tool calls, missing usage
  and unsupported capabilities. Fault fixtures are bounded to one expected attempt and cannot
  allow an external connection.
- `kiana-provider::replay_stream_fixture` reuses the existing `Framer` and `Accumulator` over
  caller-supplied bytes. It is a parser-only fixture boundary: it cannot resolve credentials,
  consume a permit, call `ProviderGateway`, open a socket or dispatch a capability.

## CI-only fixtures

`.github/workflows/p4-j7-29-provider-contract.yml` runs target formatting, the neutral matrix and
cassette fixture, arbitrary-chunk stream replay, bounded frame failures, fault/retry corpus and
the Core source guard, followed by workspace test-target compilation. The workflow path filter
includes the current CM-36 `kiana-domain/src/memory_workbench.rs` module so the repository-wide
format dependency is part of a fresh remote run. Local tests, builds, checks, clippy and smoke
commands are deliberately not run, and GitHub CI is not awaited.

## Evidence ceiling and limitations

The slice is `feature_status=implemented` with `proof_level=source` plus CI wiring. It does not
prove that every live adapter implements every claimed matrix cell, does not run a fuzz campaign,
does not open a real provider connection, and does not provide durable cross-process replay,
invoice truth, live model effects or physical capability evidence. The fault corpus records
expected safety classifications; it is not evidence that all seeds or all byte permutations have
been explored.
