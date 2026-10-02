# SC-34 security control crosswalk: SC01:T01–SC01:T12, SEC-01–SEC-12, NIST, OWASP and the internal controls

> Snapshot date: 2026-10-02. This document is a **crosswalk and a set of claims under review**.
> It is not a certification, an attestation, an audit result, or a statement that Kiana complies
> with SOC 2, ISO 27001, NIST SP 800-53, the EU AI Act or anything else. A row in this table
> says "this control item is claimed to address this clause, at this proof ceiling, and here is
> what is still uncovered". Nothing here upgrades a control above the ceiling it is written at.

## 1. Authority and proof ceiling

| Item | Record |
|---|---|
| roadmap card | [`SC-34`](security-compliance.md#step-sc-34) |
| threat input | [`security-threat-register.md`](security-threat-register.md) (SC-01, SC01:T01–SC01:T12) |
| clause input | [`company-os-security-constitution.md`](../company-os-security-constitution.md) (SEC-01–SEC-12), [`security-compliance.md`](security-compliance.md) §1.3 (INV-S01–INV-S12) |
| code landing | `kiana-policy/src/security_control_registry.rs`, registered by `kiana-policy/src/lib.rs` |
| fixtures | `kiana-policy/tests/sc34_control_registry.rs` (40 CI-only tests, deny-first), `kiana-policy/tests/sc34_control_registry_guard.rs` (source guard) |
| baseline | [sc34-control-crosswalk-baseline.md](sc34-control-crosswalk-baseline.md) |
| feature_status | `partial` — the registry can refuse an unsupported claim, but nothing loads this document into it |
| proof_level | `source` for **every** control in this document. No `local_behavior`, `durable`, `live` or `physical` |

The single most important line in this file is the proof ceiling in row 2 of the table above.
Every control below is written at `source`, because
[`CURRENT_STATUS.md`](../../CURRENT_STATUS.md) records `proof_level=source` for SC-02, SC-03,
SC-05, SC-20, SC-24, SC-26, SC-28, SC-29, SC-30, SC-31, SC-32 and SC-33, and because the CI runs
behind those slices were not awaited. Nothing in this slice changes that.

## 2. How to read a control row

Each control carries five fields, and they are deliberately not collapsible into one:

| Field | Meaning |
|---|---|
| **scope** | the `ControlScopeFacet` set the control claims to cover. A child may only narrow this. |
| **assumptions** | what must be true for the control to mean anything. An assumption that fails is not a partial pass, it is an `Unknown`. |
| **evidence** | the `ControlEvidence` the control actually carries. Every entry is a reference plus a digest, never a payload. |
| **proof ceiling** | the strongest level the control may ever be described as. It is capped by the evidence, not by ambition. |
| **not covered** | what a reader must not conclude from this row. |

### The trap this file is built to avoid

A control written at `ceiling = source` with `status = partial` is saying *"the source-level claim
is fully backed by source-level evidence, and that is all the claim is"*. It is **not** saying the
control is satisfied. The ceiling is what bounds the sentence, and at `source` that sentence is
about types, fixtures and review — never about a running system, a real boundary or a real
adversary.

The registry refuses the opposite move directly: a control whose strongest evidence is `source`
cannot be registered with a `durable` ceiling
(`security_control_proof_ceiling_exceeds_evidence`). That single rejection is the difference
between a crosswalk and a piece of marketing.

This table uses the `SC01:Tnn` identifiers from the SC-01 threat register. It does not reuse or
rename the separate `Tnn` definitions in `security-compliance.md` section 3.4; these IDs are not
aliases. See section 2.1 of the register for their many-to-many thematic crosswalk.

## 3. Threat to control crosswalk (SC01:T01–SC01:T12)

| SC-01 threat | Control | SEC | NIST | OWASP | Internal | Backing slices | Ceiling | Status |
|---|---|---|---|---|---|---|---|---|
| SC01:T01 wire actor/role impersonation | C-01 | SEC-01 | GOVERN | LLM06 | INV-S01 | SC-04, SC-06, SC-07 | source | partial |
| SC01:T02 project trust bypass / untrusted resource injection | C-12 | SEC-11 | MAP | LLM03 | INV-S11 | SC-25, SC-26, SC-27 | source | partial |
| SC01:T03 authority/policy/approval scope widening | C-02 | SEC-02 | GOVERN | LLM06 | INV-S02 | SC-05, SC-09, SC-10 | source | partial |
| SC01:T04 child delegation superset | C-02 | SEC-02 | GOVERN | LLM06 | INV-S02 | SC-09, SW-04, SW-05 | source | partial |
| SC01:T05 path traversal / symlink / TOCTOU | C-07 | SEC-07 | MEASURE | — | INV-S07 | SC-13, SC-14, PD | source | partial |
| SC01:T06 secret exfiltration through prompt/event/receipt/log/provider | C-05 | SEC-05 | GOVERN | LLM02 | INV-S05 | SC-18, SC-19, SC-20 | source | partial |
| SC01:T07 model/UI self-report approval or completion | C-10 | SEC-10 | MEASURE | — | INV-S10 | SC-31, SC-32 | source | partial |
| SC01:T08 duplicate / unknown / ambiguous side effect | C-09 | SEC-09 | MANAGE | LLM09 | INV-S09 | SC-12, SC-15, SC-33 | source | partial |
| SC01:T09 cancellation race / stale worker | C-08 | SEC-08 | MANAGE | — | INV-S08 | SC-15, SC-16, SW-10 | source | partial |
| SC01:T10 corrupt / torn / replayed storage | C-11 | SEC-10 | MEASURE | — | INV-S10 | SC-31, SC-32, ER, PD | source | partial |
| SC01:T11 resource exhaustion / retry storm | C-13 | SEC-12 | MEASURE | LLM10 | INV-S12 | SC-16, SC-22, SC-40, BQ, AUT | source | partial |
| SC01:T12 external connector / physical action misuse | C-04 | SEC-04 | MANAGE | LLM06 | INV-S04 | SC-12, SC-17, SC-30, INT | source | partial |

SEC-03, SEC-06 and SEC-12 are covered by controls that no single threat owns; they appear in
§4 as C-03, C-06 and C-13.

## 4. Control detail

### C-01 `ctl-principal-binding` — server-resolved identity

- **scope**: `identity`
- **assumptions**: the caller cannot mint a server secret; wire-supplied actor/role/project fields
  are inputs, never authority; loopback is not a durable principal.
- **evidence**: server-owned principal snapshot and role/department assignment types
  (`kiana-domain/src/security_contracts.rs`); the stable `AUTH_*` reason family.
- **proof ceiling**: `source`
- **not covered**: no external authentication provider has been exercised. The 2026-09-08
  auto-approve decision bounds entrypoint auto-approval to `LocalWrite`, default-off, and does
  **not** make an auto-approval a durable authorization: it does not survive a restart.

### C-02 `ctl-scope-intersection` — capabilities only ever narrow

- **scope**: `authorization`, `scope_limit`
- **assumptions**: every layer is server-owned; a caller cannot widen by adding a layer; the four
  code `RiskLevel` values are **not** the R0–R5 ladder and must not be converted into it yet
  (that mapping is still an open decision in the constitution).
- **evidence**: intersection-only grant derivation in `kiana-policy/src/security.rs` and
  `kiana-policy/src/project_trust.rs`; deny-first `PolicyBundle` with a non-`Allow` default.
- **proof ceiling**: `source`
- **not covered**: `PolicyBundle`/`BundlePolicyEngine` are implemented and tested but **not wired
  into the product path** — the running engine is still `DefaultPolicyEngine` in
  `kiana-policy/src/lib.rs`. The R0–R5 mapping is undefined, so no risk-level claim may be made.

### C-03 `ctl-cell-resource-bound` — a child never exceeds its parent

- **scope**: `cell_resource`
- **assumptions**: a child Cell is bounded by its parent, template, department, project,
  WorkPacket and approval simultaneously — the permission union is not representable.
- **evidence**: typed scope intersection and its property tests; bounded reservation types.
- **proof ceiling**: `source`
- **not covered**: no cross-process or restart-survival proof that the bound actually holds while
  work is running. Budget exhaustion behaviour is SC-16/SC-40 work.

### C-04 `ctl-external-effect-permit` — external effects need an exact, re-checked permit

- **scope**: `external_effect`
- **assumptions**: a permit is bound to an exact payload digest, target and expiry; the broker
  re-checks before the effect; an unavailable boundary is never replaced by a local success.
- **evidence**: pending-invocation/permit, CAS and idempotency contracts; capability-broker
  boundary checks.
- **proof ceiling**: `source`
- **not covered**: payment, ride-hailing, ticketing, IoT and physical adapters stay
  `not_supported`. Opening the HTTP MCP transport would change this row, and it is currently
  frozen and returns `mcp_transport_unsupported`.

### C-05 `ctl-secret-broker-boundary` — a secret never leaves the broker's memory

- **scope**: `secret`
- **assumptions**: secrets travel as opaque `SecretRef`; injection happens in controlled memory;
  logs, events, receipts, metrics, backups and error text keep only references or digests.
- **evidence**: recursive and streaming redaction in `kiana-domain/src/redaction.rs`; sentinel
  scanning at the Event, Receipt, Argv, Env, Stdout and Stderr channels; the boundary guards in
  `kiana-core/src/events.rs` and `kiana-core/src/receipts.rs`.
- **proof ceiling**: `source`
- **not covered**: redaction covers known markers and sentinel shapes, **not arbitrary high-entropy
  strings**. An unrecognised credential format is not caught. Key custody, HSM/OS-keyring
  behaviour and crash/restore key hygiene are SC-20+ / ER work and are not demonstrated here.

### C-06 `ctl-loopback-not-auth` — a loopback socket is not an identity

- **scope**: `loopback`, `identity`
- **assumptions**: `Host`/`Origin`/page tokens are inputs; a durable principal comes from the
  server, not from the transport.
- **evidence**: entrypoint identity boundary contracts; the `AUTH_*` unknown-claim rejection path.
- **proof ceiling**: `source`
- **not covered**: the four-entrypoint parity fixture (CLI/TTY/Web/Desktop resolving the same
  context) belongs to SC-11 and has not been observed here.

### C-07 `ctl-path-toctou` — root-relative, re-checked at effect time

- **scope**: `filesystem`
- **assumptions**: the resolved path stays inside the root; inode/generation is stable between
  check and effect; symlinks, hardlinks and renames are not followed across the boundary.
- **evidence**: lexical containment plus no-follow helpers; path locks and fence checks.
- **proof ceiling**: `source`
- **not covered**: the check-then-use window is closed in source, but no real filesystem race has
  been executed. Effect-time zero-effect proof under a real adversary is SC-13/SC-37 work.

### C-08 `ctl-cancel-fence` — no new effect after cancel

- **scope**: `cancellation`
- **assumptions**: cancel stops new intake and fences the permit/lease; a started attempt is
  observed to `Stopped` or `Unknown`, never assumed finished.
- **evidence**: `Started`/`Stopped`/`Unknown` lifecycle types; terminal-scope and epoch checks.
- **proof ceiling**: `source`
- **not covered**: a cancelled run that already reached an external boundary may still be visible
  to the outside world. That is what the reconcile inbox and SC-33 incidents are for; it is not
  something this control can make true.

### C-09 `ctl-unknown-first-class` — `Unknown` never becomes a success

- **scope**: `unknown_state`
- **assumptions**: an unknown external result is a fact, not an error alias; recovery needs new
  evidence; no blind retry without a fresh idempotency reference.
- **evidence**: the ordered contain → fence → reconcile → close workflow in
  `kiana-core/src/security_incident.rs`, where `Closed` is terminal and a close without a
  reconcile is refused.
- **proof ceiling**: `source`
- **not covered**: the workflow is a read-only contract over supplied records. It appends no
  incident event, persists nothing, pages nobody, fences no real capability and is **not yet
  reachable from a command** — that needs a branch in `kiana-core/src/commands.rs`.

### C-10 `ctl-audit-fact-source` — the EventLog is the only authority

- **scope**: `audit`
- **assumptions**: audit records carry actor, decision, reason and source cursor; transcripts,
  UI timelines, caches and model self-reports are discardable views.
- **evidence**: the strict append-only `AuditRecord` envelope in `kiana-domain/src/audit.rs`,
  validated before any write by `kiana-eventlog/src/audit_contract.rs`; the `audit.record` kind
  allow-list that refuses self-submitted `audit.*` events.
- **proof ceiling**: `source`
- **not covered**: an audit record existing is not proof that the recorded decision was *correct*.
  The record proves that a reason code was emitted, not that the world behaved as claimed.

### C-11 `ctl-audit-projection-rebuild` — projections are rebuildable, never authoritative

- **scope**: `audit`
- **assumptions**: a projection may be deleted and rebuilt from the EventLog; cursor/hash drift
  quarantines rather than silently repairs; a stale view is never labelled fresh.
- **evidence**: audit projector CAS/cursor/freshness/rebuild-equivalence contracts
  (`kiana-core/src/audit_projection.rs`, `kiana-core/src/projection_checkpoint.rs`).
- **proof ceiling**: `source`
- **not covered**: restart-and-rebuild has not been executed as a rehearsal. SC-42 owns the
  durable recovery rehearsal; until that runs, "rebuildable" is a source-level claim only.

### C-12 `ctl-untrusted-input` — model, project and network text are inputs

- **scope**: `untrusted_input`
- **assumptions**: repository text, web pages, tool descriptions and plugin manifests cannot
  change system policy, widen capability or mint identity; HTTP MCP stays closed.
- **evidence**: `ProjectTrust` root resolution in `kiana-policy/src/project_trust.rs`
  (implemented, tested, **not wired** — the daemon still resolves trust through
  `kiana-skills`' own `SourceTrust`); the closed connector-transport gate.
- **proof ceiling**: `source`
- **not covered**: indirect prompt injection reaching a real model turn is SC-39 red-team work and
  has not been run. The not-wired `ProjectTrust` gap is called out in its own module header and is
  repeated here deliberately.

### C-13 `ctl-resource-bounds` — every limit has a ceiling and backpressure

- **scope**: `resource_exhaustion`
- **assumptions**: input, output, queue, concurrency, bytes, wall time, turn and retry counts are
  all bounded; exceeding a bound fails safely rather than dispatching anyway.
- **evidence**: bounded-channel and reservation types; queue and retry ceilings.
- **proof ceiling**: `source`
- **not covered**: no fault injection has been run. Disk-full, clock rollback and provider
  429/5xx behaviour belong to SC-40.

### C-14 `ctl-supply-chain-provenance` — extensions and artifacts carry their origin

- **scope**: `untrusted_input`, `audit`
- **assumptions**: a manifest digest, capability intersection and version pin decide what an
  extension may do; a release is described by its builder, source, toolchain and subject digest.
- **evidence**: `ExtensionManifest`/`CapabilityCatalog` digest binding; `ReleaseManifest` and
  SLSA-style provenance in `kiana-domain/src/release_attestation.rs`; route attestation in
  `kiana-domain/src/route_attestation.rs`; the SBOM/advisory scanner (SC-28).
- **proof ceiling**: `source`
- **not covered**: scanner metadata cannot prove signed artifacts, transparent-log provenance or
  production advisory freshness. The offline verifiers have not been run against a real release.

## 5. Slice coverage: SC-02 … SC-33

| Slice | Control it backs | What it contributes as evidence | Ceiling |
|---|---|---|---|
| SC-02 security IDs / schema registry | C-10 | versioned IDs, strict envelopes, **unknown major rejected** — the precedent for `security_control_framework_major_unknown` | source |
| SC-03 stable reason codes | C-10 | the `AUTH_*` … `UNKNOWN_*` families with class/retryability/remediation, which is what makes every registry rejection addressable | source |
| SC-05 policy bundle / decision trace | C-02 | deny-first evaluation, explicit reason requirement, revision/epoch binding | source |
| SC-20 redaction / secret egress | C-05 | recursive text redaction and fail-closed egress guards across every boundary | source |
| SC-24 query data boundary | C-11 | one digest-bound governance decision for memory/index/cache/export visibility | source |
| SC-26 extension manifest / capability catalog | C-12, C-14 | manifest digest and capability intersection; read-only/write negative tests | source |
| SC-28 supply-chain scanner | C-14 | lockfile digest, SPDX/CycloneDX SBOM, licence and advisory thresholds, quarantine | source |
| SC-29 release provenance | C-14 | release manifest, subject digest, provenance verification | source |
| SC-30 route attestation | C-04, C-14 | route digest, data/use policy, credential/account/audience binding | source |
| SC-31 audit record | C-10 | strict append-only audit envelope with redacted field contract | source |
| SC-32 audit projector | C-11 | CAS, cursor, freshness and rebuild-equivalence contracts | source |
| SC-33 incident workflow | C-09 | ordered contain/fence/reconcile/close with evidence single-use and reviewer separation | source |

### Slices that back **no** control in this document

SC-06 through SC-18, SC-22, SC-23, SC-25, SC-27, SC-37 through SC-43 are referenced by the
threat register but have no control row here. They are **not** covered by omission-by-design and
not claimed: a control that has no slice behind it has no evidence at all, and this document does
not manufacture one. The most conspicuous gaps are listed in §6.

## 6. What this crosswalk does not cover

Stated plainly, because a crosswalk that only lists what it covers is how the "we are compliant"
misreading starts:

1. **No compliance claim of any kind.** Nothing here is an audit, an attestation, a SOC 2 report,
   an ISO 27001 statement or an EU AI Act conformity assessment. The four framework columns are
   *cross-reference vocabulary* so that a future assessor has stable identifiers to argue with.
2. **No runtime evidence at all.** Every ceiling is `source`. There is no `local_behavior`, no
   `durable`, no `live` and no `physical` row in this document, because
   [`CURRENT_STATUS.md`](../../CURRENT_STATUS.md) records none for the backing slices.
3. **No CI result was observed.** The fixtures for this slice are CI-only by instruction. Their
   result is unobserved at the time of writing.
4. **Two named controls are not wired.** C-02's `PolicyBundle` and C-12's `ProjectTrust` are
   implemented and tested but are not on the product path. Their module headers say so; this
   document repeats it rather than relying on the reader to open the source.
5. **The incident workflow has no command.** C-09 is a read-only contract; there is no branch in
   `kiana-core/src/commands.rs` that can open an incident.
6. **The R0–R5 risk mapping is undefined.** The constitution leaves the four code `RiskLevel`
   values ↔ R0–R5 mapping open. No row in this document converts between them.
7. **No control here is a gate.** Nothing in `kiana-policy` reads this document, and no release
   gate consults it. SC-41 owns the security gate; SC-35 owns the evidence manifest. Until one of
   them does, this is a reviewable claim set with nothing enforcing it.

## 7. Maintenance

The crosswalk is append-only in the same sense as the threat register: a newly discovered gap adds
a row and a deny-first fixture rather than weakening an existing row's ceiling. Raising a ceiling
requires new evidence **and** a matching `proof_level` promotion in
[`CURRENT_STATUS.md`](../../CURRENT_STATUS.md) — never a doc edit alone. The registry in
`kiana-policy/src/security_control_registry.rs` is the machine-checkable form of the same
contract; it does not parse this file, and nothing loads this file into it.
