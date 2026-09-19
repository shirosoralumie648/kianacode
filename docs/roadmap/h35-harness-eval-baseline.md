# H35 Harness trajectory evaluation and performance baseline (partial)

H35 now has a CI-only source/fixture gate over the existing GoldenTrace, provider-independent
EvalCase/EvalResult/EvalSuite, evidence-capture and PerformanceBaseline contracts. The gate
requires trace source/input/event/receipt binding, replay divergence visibility, provider/tool
effect prohibition during replay, bounded output and explicit cost/latency/capacity limitations.
Missing result or extra side effects cannot be promoted as a passing trace.

This is not a real model benchmark or live Provider result. No local runtime/test is executed by
the task; GitHub CI runs the focused fixtures after push. Real Provider/account performance,
RSS/cleanup thresholds, full CLI/TTY/Web/Desktop scenario replay, and durable benchmark artifacts
remain partial. H35 stays open until the pinned snapshot and all required scenario evidence are
actually produced.
