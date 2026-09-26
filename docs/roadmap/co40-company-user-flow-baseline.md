# CO-40 Company CLI and Workbench user flow baseline

## Scope

CO-40 adds a narrow Company user flow adapter shared by CLI and Workbench. Friendly actions
`inspect`, `next`, `inbox`, `decide`, `delivery` and `close` are parsed into existing versioned
`RequestEnvelope` commands and sent through `DaemonHost`; the adapter never applies Company state,
approves future work or starts another model loop.

## Implemented source slice

- CLI accepts `kiana company inspect|next|inbox|decide|delivery|close` with explicit session,
  project/card target, decision option/digests and JSON arguments as appropriate.
- Workbench accepts `/company inspect|next|inbox|decide|delivery|close` and maps to the same
  `company.governance.v1`, `human.inbox`, `human.resolve` and `company.business` command names.
- Both surfaces route through the existing DaemonHost/ControlPlane protocol and preserve server
  role, permission profile, target and stale-decision checks; no raw envelope is required for the
  common inspect/inbox/decide flow.

## CI-only evidence

`.github/workflows/co40-company-user-flow.yml` runs formatting, parser/route fixtures, the source
guard and entrypoint test-target compilation on GitHub Actions. Local Cargo tests, builds, checks,
clippy and smoke scripts were not run and CI is not awaited.

```text
feature_status: partial
proof_level: source
```

## Limitations and handoff

- Delivery/close actions still carry the existing versioned business JSON arguments and rely on
  server policy; the adapter does not invent business payloads or bypass gates.
- Full interactive lifecycle, Web/Desktop parity and durable restart behavior remain CO-41+ work.
