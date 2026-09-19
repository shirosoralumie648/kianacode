# CAP-18 controlled hook execution and reauthorization baseline

CAP-18 keeps hook configuration parsing in the existing query-shaped adapter but executes every
trusted hook command through the daemon sandbox/ProcessSupervisor path. Project-local hooks are
opened only after ProjectTrust; host/user sources are snapshotted with a digest, command count and
deadline, and any snapshot drift blocks before spawn. Hooks receive read-only, narrow path scope,
bounded input/output and cancellation; their decision is only Allow/Block/Ask and never grants a
capability or bypasses the ControlPlane.

Input rewrite responses are deliberately rejected as `hook_update_input_unsupported` in this
slice; no stale approval is silently reused. Allow/block/ask, untrusted project, timeout,
cancellation, nonzero output and adapter StopReport evidence remain auditable. No direct query
shell, recursive hook broker call or write-capable hook path is added.

GitHub Actions runs existing daemon hook allow/block/update/ask fixtures, CAP-18 source guards
and workspace compilation. No local runtime tests or smoke commands were run.
