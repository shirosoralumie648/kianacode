# AUT-10 interval cursor baseline (partial)

AUT-10 extracts interval due calculation into the pure `plan_interval_due` domain contract.
`Skip`, `FireOnce` and bounded `CatchUp` now produce deterministic occurrence keys and an
explicit next cursor; arithmetic overflow, invalid clock input and unbounded catch-up fail closed.
The durable workflow planner consumes this helper, so a repeated tick cannot reuse an already
advanced scheduled timestamp.

This slice does not claim a trusted wall/monotonic clock adapter, durable cursor checkpoint,
timer worker, restart replay or external trigger effect. Those remain AUT-02, AUT-09 and later
workflow/ingress steps; CI owns the behavior fixtures.
