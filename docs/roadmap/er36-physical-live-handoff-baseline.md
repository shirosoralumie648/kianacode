# ER-36 physical/live boundary and handoff baseline (partial)

The existing LiveHandoffManifest and OA-28 runbook are now indexed by an ER-36 CI-only boundary
gate. Every target is an explicit provider/connector/OTLP/operating-system handoff with a
non-secret secret-ref, source/config digest, independent operator approval, provider receipt or
physical observation, retention/incident reference and bounded cleanup plan. The default preflight
denies without explicit opt-in and rejects raw credentials; Unknown remains fenced and requires
reconciliation.

The repository does not contact external services, target OS controls, providers, connectors or
OTLP backends in this gate. CI source/fixture success is not live or physical proof, and mock
receipts are not target receipts. ER-36 remains partial with proof level source until a human
supplies each target's isolated environment, approved credential reference, operation scope,
independent receipt, incident/reconciliation and cleanup evidence. Unsupported targets remain
not_supported rather than being promoted by compilation.
