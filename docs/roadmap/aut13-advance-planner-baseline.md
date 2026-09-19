# AUT-13 Advance planner baseline (partial)

`Advance` now derives a stable ready-node view, commits one node execution per transition, and
records output digests so fan-in cannot treat a missing or tampered result as an empty input.
FanOut is bounded to 32 declared branches; FanIn aggregates dependency outputs in sorted order.
SubWorkflow admission fixes the declared definition version, requires the same owner, bounds
depth and child step budget, and pure replay reconciles child terminal state back to the parent.

The planner still returns intent/effect to ControlPlane; it does not execute a child or capability.
Scope/path/budget leases, durable reservation/worker dispatch and physical child recovery remain
AUT-07/AUT-14/AUT-19 work. Older snapshots default missing output/depth fields conservatively.
