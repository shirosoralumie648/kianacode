# AUT-12 trigger concurrency baseline (partial)

Durable trigger state now keeps occurrence digests alongside pending and fired keys. The planner
rejects a same-key/different-payload collision, bounds pending occurrences at 128, and keeps
Reject/Queue/Coalesce/Replace branches explicit. Replace marks active instances
`CancelRequested` and queues the successor without starting it in the same planning pass; an
unknown stop remains on the existing reconciliation path.

The compatibility fields remain readable for older snapshots through serde defaults. This slice
does not add a durable pending index, background cancellation worker or proof that a physical stop
completed; EventLog CAS and runtime reconciliation remain the authority.
