# ER-25 retry policy and new attempt baseline

ER-25 treats retry as a new, bounded attempt rather than a free repeat. The model
contract persists attempt identity, retry class, request-sent/effect state,
retry-after backoff and a hard deadline; the Harness reserves and settles each
attempt against the shared budget and records the attempt projection. Only
`BeforeSend`/`Rejected` model failures with no uncertain effect may retry, and every
new attempt rebuilds/prepares the request and re-enters model admission.

Unknown effects, denied capabilities, cancellation and missing idempotency never
auto-retry. Attempt count, task budget or deadline exhaustion returns a terminal
failure/Unknown classification. GitHub Actions runs H05/H07/H08/H11 fixtures, CP
Unknown/effect guards, the ER-25 source guard and workspace target compilation. Local
runtime tests and smoke commands are intentionally not run.

This is source/static evidence only; provider-specific live backoff, cross-process
retry durability and external effect reconciliation remain outside this slice.
