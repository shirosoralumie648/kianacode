# CAP-14 shell executor and typed command-plan baseline

CAP-14 gives shell string and argv input one strict `ShellCommandPlan`. Strings are explicitly
opaque and execute only as a fixed non-login `/bin/sh -c` invocation with a command digest;
argv values are preserved as individual arguments and are never joined into a shell string.
NUL, empty executable, argument count/size and total command bounds fail before spawn. The plan
metadata records `opaque_shell_string` versus `argv`, fixed shell/login state and that no side
effect analysis was performed.

Both short shell execution and long-running process start consume the same plan and the existing
ProcessSupervisor, so caller-provided workdir/sandbox/deadline cannot replace the authorized
profile. No regex-based compound-command approval or shell-prefix borrowing is introduced.

GitHub Actions runs the typed-plan unit fixtures, focused shell regression, CAP-14 source guard
and workspace compilation. No local runtime tests or smoke commands were run.
