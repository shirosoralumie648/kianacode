# CAP-27 process handle and PTY baseline

CAP-27 treats a long-running process as a continuation of an already authorized capability.
`JobHandle` is server-created and opaque to callers while binding the start request/invocation,
Run/Turn, owner/session, project digest, authority epoch, process-group evidence and expiry. Poll,
stdin, resize and stop revalidate the owner, project, authority and handle digest; polling does
not extend the lease. A handle without live process evidence after restart returns an explicit
unavailable/result-unknown state and cannot be used to stop a different process.

The process occupies capacity, workspace locks and the bounded resource/output contract until the
supervised lifecycle ends. ProcessSupervisor owns stop/reap and process-group observation; an
unconfirmed stop, descendant escape, authority revoke or terminal-record failure remains Unknown
and fenced (`result_unknown`, `automatic_retry:false`). PTY is opt-in, reports unsupported
backends explicitly, validates rows/columns, isolates control/output capture and documents
stdout/stderr 合流, 控制字符 and OSC handling.

GitHub Actions runs the opaque handle fixture, existing ER-19 process/stop guard and CAP-27
source guard plus workspace compilation. No local runtime tests or smoke commands were run.
