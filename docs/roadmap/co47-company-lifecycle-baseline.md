# CO-47 Company fake-model lifecycle and fault baseline

CO-47 extends the existing `p3_i06_company_golden` DaemonHost path with a CI-only lifecycle
fixture. The new `company_lifecycle` target exercises server-routed role denial, missing-project
close rejection, idempotency payload drift, rejected-event persistence, real-file absence and
duplicate business-fact accounting. The existing golden target remains the success path and checks
the actual output file, persisted run evidence, acceptance/review, delivery confirmation and
CompanyClosingReceipt.

`scripts/company-os-business-smoke.sh` is remote-only (`GITHUB_ACTIONS=true`) and runs the focused
deny/recovery/effect-accounting fixture, the existing golden path, source guard and workspace test
compile. It does not invoke a provider, external delivery, physical effect or second execution
loop. `ResultUnknown`/reconcile, cancellation, restart and broader multi-packet matrix remain
explicitly limited to the existing Company contracts and later evidence work.

`feature_status=implemented`; `proof_level=source`. GitHub CI is the test authority and its result
is not awaited here; no local test/build/check/clippy/smoke command was run.
