# PD-34 persistence capacity baseline (partial)

PD-34 extends the existing PerformanceBaseline/CapacityEnvelope contract with operation budgets
for append, flush, project, rebuild, query and export. Each pressure observation binds p95/p99
latency, queue depth, rejection rate, maintenance share, degradation reason and facts-preserved
evidence. Exceeding any budget blocks the report; bounded degradation remains visible instead of
being reported as healthy.

`PersistenceCapacityEvidence` now binds the report/baseline digests to benchmark and resource
receipts, representative-workload, facts-preserved and degradation acknowledgement fields. A
fixture or source-only report cannot be promoted to durable/physical capacity proof.

GitHub Actions runs the new budget fixture and the existing OA-25 percentile/capacity/migration
fixture. No local benchmark, stress test, queue resize or maintenance operation was performed;
CI fixture timings are not production P95/P99 or durable capacity proof. PD-34 remains partial
until representative storage/artifact/index/backup/prune pressure runs and platform-specific
resource receipts exist.
