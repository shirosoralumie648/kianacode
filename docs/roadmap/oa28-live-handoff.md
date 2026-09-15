# OA-28 Live / physical handoff runbook

This runbook is a handoff contract, not an enable switch. The default state is
`not_supported`; no provider, connector, OTLP backend or operating-system effect is contacted by
the repository or its CI. A target can move to `opted_in` only after an operator supplies an
isolated environment, a non-secret `secret-ref:...`, a reviewed scope and an approval reference.

## Required manifest

Create a `kiana.live-handoff.v1` manifest for every target/account/operation combination. Bind the
configuration digest and source snapshot to the exact binary and policy revision. Never put a key,
bearer token, cookie, endpoint credential or raw payload in the manifest.

```json
{
  "target": "provider|connector|otlp_backend|operating_system",
  "target_id": "isolated-account-or-backend-id",
  "environment": "staging|production-with-explicit-approval",
  "credential_ref": "secret-ref:managed/path",
  "configuration_digest": "sha256:<64 lowercase hex>",
  "source_snapshot": "git:<commit>",
  "operator_approval_ref": "approval:<id>",
  "provider_receipt_ref": "receipt:<id>",
  "retention_class": "class-name",
  "cleanup_plan": "bounded rollback/revoke/delete and evidence retention steps",
  "status": "opted_in|verified|unknown|not_supported",
  "limitations": ["target-specific constraints"]
}
```

## Sequence

1. Pin the target, account, project/data boundary, source commit, binary hash, policy/config digest,
   sampling and retention class. Confirm no broad wildcard scope or shared production credential.
2. Run `scripts/oa28-live-handoff-preflight.sh` with `KIANA_LIVE_HANDOFF_OPT_IN=1` and only
   non-secret references. Without that flag the script exits `2` with `live_opt_in_required`.
3. Obtain independent operator approval. The manifest remains `opted_in`; it is not a provider or
   connector permit. ControlPlane authorization, budget, gate and capability admission still run.
4. Perform one target-specific operation through the existing DaemonHost → ControlPlane → Broker
   path. Record the provider/connector/OTLP/OS receipt, request/attempt IDs, source cursor, redacted
   artifact hash, timeout/stop state, incident and cleanup result. A network 2xx or exporter ACK is
   not a business Outcome.
5. If the effect or receipt is ambiguous, set `unknown`, fence resources and reconcile with a new
   authorized observation. Never retry an unknown operation using the same idempotency key or close
   the incident from a UI/telemetry self-report.
6. Revoke/rotate credentials and clean the isolated workspace according to retention/legal hold.
   Preserve only the bounded manifest, receipt reference, hashes and audit metadata. Verify no raw
   secret entered EventLog, Receipt, Audit, Metric, Trace, export, stdout/stderr or cache.
7. Set `verified` only when the independent receipt, approval, configuration/source binding,
   retention and cleanup evidence are complete. Otherwise leave `unknown` or `not_supported`.

## Target matrix

| Target | Minimum independent evidence | Current repository ceiling |
|---|---|---|
| Provider | isolated account, request/usage receipt, redacted response hash, stop/retry/reconcile | no live provider is contacted by this gate |
| Connector | binding/account scope, provider receipt/query, idempotency, cancel/compensation, cleanup | only local fixture/stdio paths are source-level |
| OTLP backend | endpoint/auth audience, sampling policy, export/flush/shutdown receipt, retention | no external telemetry backend is configured |
| Operating system | target OS, privilege boundary, safety controller, operator/physical confirmation | physical effect remains `not_supported` |

CI validates the manifest contract, unknown/secret fail-closed behavior and preflight default deny;
it does not provide live or physical proof. Every real target must add a separate `CURRENT_STATUS.md`
evidence block with environment, version, account/scope, command, exit code, receipt, incident,
cleanup and limitations.
