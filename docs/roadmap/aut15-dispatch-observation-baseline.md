# AUT-15 dispatch and observation baseline

AUT-15 separates a claimed worker dispatch intent from its later observation. The intent binds
reservation, action digest, worker identity and authority epoch; observations must match those
facts. Succeeded/Failed become Observed, while uncertain transport remains Unknown and cannot be
converted to success. The Core facade is validation-only; no worker/provider/Broker effect runs.

GitHub CI runs the domain fixture and Core source guard in
`.github/workflows/aut15-dispatch-observation.yml`. Local Cargo tests/build/check/clippy/smoke were
not run. This slice is `feature_status=partial`, `proof_level=source`; durable worker claims,
restart recovery, retries and live effects remain AUT-16+.
