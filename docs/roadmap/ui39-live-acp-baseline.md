# UI-39 live ACP/IDE opt-in baseline (partial)

The existing versioned UI protocol, client, DaemonHost, Workbench/Web and Desktop worker surfaces
are indexed by a UI-39 source gate. A future ACP/IDE adapter must map initialize/session/prompt/
update/permission/cancel/reconnect through the same snapshot/feed/action/receipt contracts. Host
editor or terminal capabilities cannot become a Kiana permit.

`UiLiveHostEvidence` now records protocol/host/environment/workspace identity, session state,
initialize/prompt/update/permission/cancel/reconnect observations, operator approval and receipt.
`UiHostCapability` rejects direct host effects and requires advertised capabilities to delegate
back through Kiana; `not_supported`, `opted_in` and `unknown` retain explicit limitations.

No ACP/IDE live adapter, external host, workspace attachment or live receipt is present in this
slice. The default state remains not_supported; live opt-in requires a fixed protocol/workspace,
operator approval, redacted data and independent evidence. CI/source success is not live,
durable or physical proof.
