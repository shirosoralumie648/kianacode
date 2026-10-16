# BQ-05 RateCardStore Baseline

## Scope

BQ-05 adds the `RateCardStore` port and bounded `InMemoryRateCardStore` fixture adapter. Cards are
resolved by provider/model and a caller-supplied timestamp, pinned by `RateCardId`/`card_version`,
and rejected when effective windows overlap for the same provider/model selector. An effective
provider-wide `model_selector="*"` card may coexist with an exact model card, with the exact card
winning deterministically. Unknown model, expired card, duplicate version and invalid lookup are
stable failures; no implicit “current price” fallback exists.

`RateCard` now exposes explicit dimensions for cache, reasoning, audio input/output, requests,
tools and effects. `CostEstimate` carries the pinned `rate_card_version`; it remains an estimate,
not a provider invoice or financial authorization.

## Evidence and limits

- `kiana-ports/tests/bq05_rate_card_store.rs` covers pinned resolution, exact-model precedence
  over provider-wide `model_selector="*"` fallback, dimension mapping, audio/effect pricing,
  duplicate-version/overlap, unknown-model and expiry denial.
- `kiana-core/tests/bq05_rate_card_guard.rs` protects the port-only/no-provider/no-financial-
  authority boundary. The focused local receipt below is bounded and does not replace CI.

This slice is `feature_status=implemented`, `proof_level=source`: exact model cards take precedence
over an effective provider-wide `model_selector="*"` fallback; no durable rate-card projector,
provider discovery, invoice import, currency conversion or reservation admission is claimed.

## 2026-10-05 focused local receipt

The stale source guard was narrowed to the `RateCardStore`/`InMemoryRateCardStore` source interval;
the rest of `kiana-ports/src/lib.rs` contains unrelated capability and budget ports. The overlap
fixture now uses `card_version=2`, while a disjoint same-key `card_version=1` fixture explicitly
proves duplicate-version denial.

```text
source_snapshot: current dirty worktree; kiana-ports RateCardStore fixture and kiana-core scoped guard
worktree_status: roadmap row 194 remains 🔄; fixture/guard-only correction, no production billing or manifest change
command_argv: bounded serial cargo test -p kiana-ports --test bq05_rate_card_store --locked --offline -- --test-threads=1; cargo test -p kiana-core --test bq05_rate_card_guard --locked --offline -- --test-threads=1
cwd·environment: repository root Linux/bash; CARGO_BUILD_JOBS=1, RUST_TEST_THREADS=1, RUST_MIN_STACK=33554432, stripped non-incremental tests; ports max RSS 265992 KiB, Core guard 85744 KiB, swaps 0
fixture·cassette: ports 2/2 (pinned lookup/dimensions; duplicate-version, overlap, unknown-model and expiry denial); Core guard 1/1
exit_code: both commands 0; logs/resources `/tmp/kiana-roadmap-20261005-evidence/bq05-{ports,core}.{log,resources}`
status_change: selected BQ-05 behavior is local_behavior; whole card remains feature_status=implemented, proof_level=source
proof-level_change: selected fixture/guard source → local_behavior; no durable/live/physical promotion
limitations: cache/reasoning inclusion consistency, invalid ID handling, bounded-memory policy, durable projector, provider discovery, invoice/currency conversion, reservation admission and external proof remain open
reviewer: integration agent with BQ-05 read-only audit; no human release reviewer
```

## 2026-10-13 bounded in-memory card capacity

`InMemoryRateCardStore` now has an explicit `MAX_RATE_CARD_STORE_CARDS` limit of 256. The
adapter checks duplicate-version and effective-window conflicts before capacity, preserving the
more specific denial when a full store receives an existing conflict. A 257th distinct card is
rejected with `rate_card_capacity_exceeded`, and an existing card remains readable after the
rejection.

```text
source_snapshot: current dirty worktree; kiana-ports/src/lib.rs rate-card adapter; kiana-ports/tests/bq05_rate_card_store.rs; kiana-core/tests/bq05_rate_card_guard.rs
worktree_status: BQ-05 row 194 remains 🔄 / feature_status=implemented / proof_level=source; selected in-memory capacity behavior is local_behavior; no manifest/lockfile/provider/financial authority change
command_argv: bounded serial `cargo test -p kiana-ports --test bq05_rate_card_store --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test bq05_rate_card_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root `/media/shirosora/4A183E5C183E46EB/codestorage/kianacode`; one Cargo/compiler job and one Rust test thread; 32 MiB Rust test stack; stripped/non-incremental locked/offline profile; completed runs reported 0 swap
fixture·cassette: ports 6/6 including 256 accepted distinct cards, 257th capacity denial and retained-card lookup; Core guard 1/1 checks `MAX_RATE_CARD_STORE_CARDS` and `rate_card_capacity_exceeded`
exit_code: ports 0, Core guard 0; ports max RSS 4081692 KiB / 89% CPU / 0:59.80; Core guard max RSS 4039880 KiB / 91% CPU / 1:23.17; resource receipts `/tmp/kiana-roadmap-20261010-evidence/bq05-capacity-{ports,core}.{resources,log}`
status_change: selected bounded in-memory adapter moves source → local_behavior; whole BQ-05 remains open
proof-level_change: local_behavior only; no durable/live/physical promotion
limitations: the bound applies only to the fixture adapter; durable card projection, provider discovery, invoice/currency conversion, reservation admission, cross-process behavior and full workspace/remote acceptance remain open
reviewer: root integration agent with BQ-05/BQ-06/CAP-07 read-only audit; focused rate-card port reviewer
```

## 2026-10-06 wildcard resolution receipt

`RateCard.validate` already permits `model_selector="*"`, while the previous in-memory resolver
only considered exact model IDs. The minimal adapter fix resolves an effective exact model card
first, then an effective provider-wide wildcard card, and preserves the existing unknown-versus-
expired error split when neither is effective. The fixture proves both fallback and exact override.

```text
source_snapshot: current dirty worktree; kiana-ports/src/lib.rs and kiana-ports/tests/bq05_rate_card_store.rs
worktree_status: roadmap row 194 remains 🔄; wildcard lookup is local_behavior only; no manifest/lockfile or second execution path
command_argv: bounded serial `cargo test -p kiana-ports --test bq05_rate_card_store --locked --offline -- --test-threads=1`; `cargo test -p kiana-core --test bq05_rate_card_guard --locked --offline -- --test-threads=1`; `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; `CARGO_BUILD_JOBS=1`, `RUST_TEST_THREADS=1`, `RUST_MIN_STACK=33554432`, stripped/non-incremental test profile; one Cargo worker; ports max RSS 4100564 KiB (~3.91 GiB), Core guard max RSS 1197964 KiB (~1.14 GiB), 0 swaps
fixture·cassette: ports 3/3 (pinned dimensions, wildcard fallback, exact-model precedence, duplicate-version, overlap, unknown-model and expiry denial); Core guard 1/1; format and diff checks green; logs/resources `/tmp/kiana-roadmap-20261006-bq05-wildcard/{ports,core}.{log,resources}`
exit_code: all listed commands 0
status_change: selected wildcard resolution behavior is recorded as `local_behavior`; whole card remains `feature_status=implemented`, `proof_level=source`
proof-level_change: selected adapter/fixture source → local_behavior; no durable/live/physical promotion
limitations: cache/reasoning inclusion consistency, invalid ID policy beyond existing bounded validation, bounded-memory enforcement, durable projector, provider discovery, invoice/currency conversion, reservation admission and external proof remain open; full workspace remains non-green/resource-intensive
reviewer: root integration agent with BQ-05 read-only audit; no human release reviewer
```

## 2026-10-07 nil RateCardId lookup denial

`InMemoryRateCardStore::read_rate_card` now rejects a nil `RateCardId` before taking the cards
read lock. Invalid identity is therefore distinct from a well-formed but unknown card and returns
the stable `rate_card_lookup_invalid` failure.

```text
source_snapshot: current dirty worktree; kiana-ports/src/lib.rs; kiana-ports/tests/bq05_rate_card_store.rs; kiana-core/tests/bq05_rate_card_guard.rs
worktree_status: roadmap row 194 remains 🔄 / feature_status=implemented / proof_level=source; bounded adapter identity check only; no manifest/lockfile/provider/financial authority change
command_argv: RED `cargo test -p kiana-ports --test bq05_rate_card_store rate_card_store_rejects_nil_identity_before_lookup --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-ports --test bq05_rate_card_store --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test bq05_rate_card_guard --locked --offline -- --test-threads=1`; final `cargo fmt --all --check`; `git diff --check`
cwd·environment: repository root Linux/bash; one Cargo worker/test thread; 32 MiB test stack; stripped/non-incremental profile; RED max RSS 468172 KiB / 96% CPU; ports GREEN max RSS 468260 KiB / 96% CPU; Core guard max RSS 1198724 KiB / 91% CPU; all completed runs reported 0 swaps
fixture·cassette: RED old adapter returned `Ok(None)` for nil UUID; GREEN ports 4/4 covers nil denial plus pinned/exact/wildcard/overlap/expiry behavior; Core guard 1/1 pins `rate_card_id.as_uuid().is_nil` and stable error
exit_code: RED 101 expected; ports/Core/fmt/diff 0
status_change: nil identity lookup denial moves source → local_behavior; whole BQ-05 remains implemented/source
proof-level_change: adapter deny path is locally verified; no durable/live/physical promotion
limitations: bounded-memory policy, durable projector, provider discovery, invoice/currency conversion, reservation admission and external proof remain open; full workspace remains non-green/resource-intensive
reviewer: root integration agent with BQ-05 read-only audit; focused rate-card port reviewer
```

## 2026-10-08 zero lookup timestamp denial

`InMemoryRateCardStore::resolve_rate_card` now rejects the zero timestamp before reading the
card set. This keeps an uninitialized lookup distinct from an expired or unknown model and
returns the stable `rate_card_lookup_invalid` failure.

```text
source_snapshot: current dirty worktree; kiana-ports/src/lib.rs; kiana-ports/tests/bq05_rate_card_store.rs; kiana-core/tests/bq05_rate_card_guard.rs
worktree_status: roadmap row 194 remains 🔄 / feature_status=implemented / proof_level=source; zero-time input denial is locally verified; no manifest/lockfile/provider/financial authority change
command_argv: RED `cargo test -p kiana-ports --test bq05_rate_card_store rate_card_store_rejects_zero_lookup_timestamp_before_lookup --locked --offline -- --exact --test-threads=1`; GREEN `cargo test -p kiana-ports --test bq05_rate_card_store --locked --offline -- --test-threads=1`; Core guard `cargo test -p kiana-core --test bq05_rate_card_guard --locked --offline -- --test-threads=1`
cwd·environment: repository root Linux/bash; one Cargo job/test thread; `RUST_MIN_STACK=33554432`; stripped/non-incremental profile; ports GREEN max RSS 83016 KiB / 65% CPU; Core guard max RSS 429016 KiB / 87% CPU; all measured runs reported 0 swaps
fixture·cassette: RED old resolver classified zero timestamp as `rate_card_expired`; GREEN ports 5/5 covers zero timestamp plus nil identity, pinned/exact/wildcard/overlap/expiry behavior; Core guard 1/1 pins the pre-read validation and stable error
exit_code: RED 101 expected; ports/Core 0
status_change: selected zero-time lookup denial moves source → local_behavior; whole BQ-05 remains feature_status=implemented, proof_level=source
proof-level_change: invalid timestamp boundary source → local_behavior; no durable/live/physical promotion
limitations: bounded-memory policy, durable projector, provider discovery, invoice/currency conversion, reservation admission and external proof remain open; full workspace remains non-green/resource-intensive
reviewer: root integration agent with BQ-05 read-only rate-card audit; focused lookup-input reviewer
```

## 2026-10-08 unproven cache/reasoning overlap denial

`RateCard::estimate` now returns an explicit `Unsupported` unknown result when a usage vector
contains positive cumulative input/output tokens together with positive cache or reasoning
subdimensions. Provider adapters can expose both totals and subsets, but this domain contract has
no inclusion metadata proving that subtracting or separately pricing them is correct; billing must
therefore remain unknown instead of silently double-counting. `CostBreakdown::from_usage` already
omits lines whenever the estimate carries an unknown reason, keeping estimate and receipt lines
consistent for this boundary.

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_pricing.rs; kiana-domain/tests/bq04_pricing.rs; kiana-core/tests/bq04_pricing_guard.rs
worktree_status: BQ-05 row 194 remains 🔄 / feature_status=implemented / proof_level=source; selected overlap boundary is locally verified
command_argv: RED `cargo test -p kiana-domain --test bq04_pricing estimate_rejects_unproven_cache_and_reasoning_overlap --locked --offline -- --exact --nocapture --test-threads=1`; GREEN same selector; full BQ-04 pricing target; Core BQ-04 guard
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB test stack, stripped/non-incremental profile; RED 224392 KiB / 30% CPU; selector GREEN 4091672 KiB / 87% CPU; full pricing 83152 KiB / 14% CPU; all runs reported 0 swaps
fixture·cassette: input=10, cache_read=4, output=8, reasoning=2 with all corresponding prices present; RED produced payable amount 50, GREEN selector 1/1 returns amount=None and `BillingUnknownReason::Unsupported`, full BQ-04 4/4 and Core guard 1/1
exit_code: RED 101 expected; selector/full pricing/Core guard 0
status_change: selected cache/reasoning overlap denial moves source → local_behavior; whole BQ-05 remains implemented/source
proof-level_change: fail-closed estimate boundary source → local_behavior; no provider receipt, durable ledger, ControlPlane admission, live or physical billing proof
limitations: provider inclusion metadata is not modeled, so all positive cumulative+subset combinations remain unknown; measured provider receipts, durable billing projection, reservation/invoice reconciliation and live/physical proof remain open
reviewer: root integration agent with BQ-05 billing audit; focused overlap reviewer
```

The corresponding `CostBreakdown::from_usage` path also returns `Unknown(Unsupported)` and emits
no payable lines for the same overlap fixture. The new focused receipt test passes; the broader
`bq13_cost_receipt` target still has an unrelated legacy provider-receipt serialization assertion
failure, so this evidence does not promote the billing receipt suite.

```text
source_snapshot: current dirty worktree; kiana-domain/src/billing_cost.rs; kiana-domain/tests/bq13_cost_receipt.rs
worktree_status: BQ-05 remains 🔄 / feature_status=implemented / proof_level=source
command_argv: focused `cargo test -p kiana-domain --test bq13_cost_receipt overlapping_usage_is_unknown_before_cost_lines_are_emitted --locked --offline -- --exact --nocapture --test-threads=1`; full bq13 target
cwd·environment: repository root Linux/bash; one Cargo worker/test thread, 32 MiB stack, stripped/non-incremental profile; focused 237328 KiB / 32% CPU, full 83700 KiB / 66% CPU; all runs reported 0 swaps
fixture·cassette: known input/output plus cache-read/reasoning subsets; focused 1/1 proves unknown and empty lines; full bq13 6/7 with legacy measured-receipt serialization assertion failure
exit_code: focused 0; full target 101 baseline failure
status_change: estimate-to-line suppression is locally verified; whole BQ-05 remains implemented/source
proof-level_change: cost-line boundary source → local_behavior; no durable/live/physical promotion
limitations: provider inclusion metadata and measured receipt serialization parity remain open; durable ledger/invoice/reconciliation and full workspace regression remain non-green
reviewer: root integration agent with BQ-05/BQ-13 billing audit; focused cost-line reviewer
```
