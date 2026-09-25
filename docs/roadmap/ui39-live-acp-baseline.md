# UI-39 live ACP/IDE opt-in baseline (source boundary)

The existing versioned UI protocol, client, DaemonHost, Workbench/Web and Desktop worker surfaces
are indexed by a UI-39 source gate. `kiana-client::LiveAcpSession` now requires an explicit IDE
opt-in, a fixed ACP protocol/host/workspace identity, an operator approval reference, local-only
transport metadata and redacted payloads. It maps initialize/session/prompt/update/permission/
cancel/reconnect through the existing `AcpSessionAdapter` and shared snapshot/feed/action/receipt
contracts. Host editor or terminal capabilities cannot become a Kiana permit.

`UiLiveHostEvidence` now records protocol/host/environment/workspace identity, session state,
initialize/prompt/update/permission/cancel/reconnect observations, operator approval and receipt.
`UiHostCapability` rejects direct host effects and requires advertised capabilities to delegate
back through Kiana; `not_supported`, `opted_in` and `unknown` retain explicit limitations.

`scripts/verify-ui39-live-acp-opt-in.sh` is a fail-closed preflight fixture: it accepts only explicit
opt-in, ACP v1/v2, local stdio/unix-socket metadata, digest-bound workspace/environment, an approval
reference and redacted payloads. It emits no credentials and does not launch an external host.

The adapter is intentionally transport-free: it does not open a socket, spawn a process, contact a
provider, or execute a host capability. `LiveAcpSession` remains `OptedIn`/`Unknown` and never
self-promotes to `Verified`; external host/version/environment, receipt and reconnect evidence
must be independently captured. CI/source success is not live, durable or physical proof. The
workflow path filter also includes current CM-36 `kiana-domain/src/memory_workbench.rs`, so a fresh
remote format/source-guard run covers the repository-wide fmt dependency; that result is
pending/unobserved.
