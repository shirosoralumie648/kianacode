# CAP-12 ProcessSupervisor and bounded resource baseline

CAP-12 consolidates shell, long-running process and stdio-MCP process ownership behind the
daemon `ProcessSupervisor`. The shared domain contracts distinguish a `ProcessResourceBudget`
from a `StopReport`; a terminal process is only confirmed after the leader is reaped and the
descriptor-observed process group is empty. TERM/KILL and both leader/reaper waits are bounded;
failure to establish those observations remains unconfirmed and keeps the execution fenced.

The supervisor applies the enforceable per-process CPU, address-space, file-size, open-FD and
process-count rlimits, while its receipt explicitly marks RSS and aggregate memory/pids as
observed or not hard-enforced without cgroup/pidfd evidence. Shell, process continuations and
stdio MCP all use the same preparation/stop boundary; output draining remains bounded and is
reported alongside the stop report.

GitHub Actions runs the domain contract tests, process source guard, focused daemon timeout
fixture and workspace compilation. No local runtime tests or smoke commands were run. This is
source/static evidence only; cgroup/pidfd aggregate enforcement and physical process-tree proof
remain open.
