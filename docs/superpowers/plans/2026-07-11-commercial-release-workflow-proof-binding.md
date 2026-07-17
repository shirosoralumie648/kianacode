# Commercial Release Workflow Proof Binding Implementation Plan

- [x] CLI verify 输出 `run_id` 和 `kiana.workflow-release-binding.v1`
- [x] 增加 release binding JSON Schema 与命令测试
- [x] commercial blocker 要求显式 `KIANA_RELEASE_WORKFLOW_RUN_ID`
- [x] commercial blocker 独立重算并比较 Git HEAD/diff/status 哈希
- [x] CLI 与 blocker 绑定 Workflow status/current node/last event/state/EventLog 哈希
- [x] commercial blocker 要求 `completed` 且最后事件为 `workflow_completed`
- [x] fixture 覆盖 satisfied、缺 run ID、错 run、非终态、stale Workflow/Git binding
- [x] 更新 reference matrix 与 commercial readiness
- [x] 运行 focused 和 workspace gates
