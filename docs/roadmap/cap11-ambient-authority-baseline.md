# CAP-11 ambient authority and default-deny sandbox baseline

CAP-11 makes the Linux bwrap boundary explicit for tool and stdio-MCP child processes. The
child receives only the fixed sandbox environment, with synthetic HOME/TMP/PATH and no provider,
SSH-agent, loader, shell-startup or proxy variables. `/proc`, `/dev`, `/tmp`, `/run` and `/sys`
are namespace-local views; `--unshare-all`, capability dropping and `no_new_privs` are requested
by the OS-facing launch boundary, and a missing Linux FD/no-new-privileges primitive fails the
spawn rather than falling back to the host.

Mount descriptors are registered in one bwrap plan. Unknown inherited descriptors are marked
CLOEXEC before the helper execs the tool, while only descriptor-pinned mounts remain available to
the helper. MCP executable/configuration mounts use the same registration path.

GitHub Actions runs the focused sandbox environment tests, CAP-11 source guard and workspace
compilation. No local runtime tests or smoke commands were run. This is source/static evidence;
kernel-level network, namespace and descriptor behavior still requires the remote CI fixture and
does not establish live/physical isolation by itself.
