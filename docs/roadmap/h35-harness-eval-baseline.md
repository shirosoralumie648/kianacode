# H35 Harness 轨迹评测与性能验证基线

H35 now has a CI-only contract/fixture gate over GoldenTrace binding, provider-independent
EvalCase/EvalResult/EvalSuite, evidence capture and PerformanceBaseline. The new HarnessEval
records bind source/model/prompt/tool-directory/input/workspace/runtime versions, make the first
replay difference explicit, keep offline replay free of network/process/effect calls, classify
the required task/protocol/safety/compaction/progress/cancel/resource/cost metrics, and compare
serial/parallel, old-truncation/new-summary and baseline/extra-verification variants.

`promote` is derived from candidate result integrity and safety, not performance: an extra side
effect, missing result, permission-denial regression or incomplete usage prevents promotion.

## Contract catalog

| Contract | Purpose |
|---|---|
| `kiana.harness-trace-binding.v1` | GoldenTrace/source/model/prompt/tool/input/workspace/runtime and Receipt binding |
| `kiana.harness-replay-report.v1` | offline no-effect replay, first difference, missing-result/extra-effect verdict |
| `kiana.harness-metric-set.v1` | bounded task/protocol/safety/compaction/progress/cancel/resource/cost observations and usage completeness |
| `kiana.harness-eval-comparison.v1` | same-task A/B variants with safety/integrity veto over performance improvement |

## CI-only fixture catalog

| Fixture | 断言 |
|---|---|
| `trace_binding_covers_source_model_prompt_tools_input_and_workspace` | pinned trace inputs cannot omit source, model, prompt, tool directory, input or workspace hashes |
| `offline_replay_fails_missing_result_and_blocks_any_effect` | missing result is a failure; any process/network/effect or live-provider replay is blocked |
| `metric_set_separates_incomplete_usage_and_bounds_cost_promotion` | incomplete usage is explicit and cannot become a cost-passing promotion |
| `ab_comparison_never_trades_security_or_integrity_for_performance` | performance improvement never offsets safety or result-integrity regression |
| `harness_eval_is_replay_bound_and_side_effect_free` | existing GoldenTrace/eval/evidence/performance/source boundaries remain one read-only evaluation path |

## Proof ceiling and handoff

H35 proof ceiling is `source` plus remote CI wiring. No local runtime/test is executed by the
task; GitHub CI runs the focused fixtures after push and is not awaited. Real Provider/account
performance, RSS/cleanup thresholds, full CLI/TTY/Web/Desktop scenario replay and durable
benchmark artifacts remain H36/PD/provider evidence and are not claimed here.
