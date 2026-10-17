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

## Behavioural fixtures (2026-10-17)

`kiana-daemon/src/harness_sandbox.rs` now carries deny-first fixtures that plan **and spawn** the
real boundary (the same `bwrap_plan_scoped` + `sandbox_env` pair the shell and stdio-MCP adapters
install) and then observe the child. They are part of the crate's lib test target, so they need the
`bwrap` runtime and a `python3` probe; the CAP-11 workflow installs and verifies both, and a missing
tool fails the fixture instead of degrading to unsandboxed execution. Each child command is bounded
by a 60 s fixture deadline so a hanging probe is an explicit failure.

| fixture | what it must show |
|---|---|
| `secret_env_and_inherited_fds_are_unavailable` | provider/SSH-agent/loader/shell-startup/proxy names from the host are absent, an inherited descriptor's secret bytes are unreachable, and the `env -i` route does not restore them |
| `direct_ipv4_ipv6_dns_and_host_unix_sockets_are_denied` | a host ipv4/ipv6 listener, DNS resolution and a host unix socket are all unreachable/absent, and an explicit `HTTPS_PROXY`/`ALL_PROXY` override still grants no egress |
| `sandbox_cannot_rejoin_host_namespace` | the child joined new ipc/mnt/net/pid/uts namespaces and sees only the namespace-local loopback interface |
| `offline_local_shell_and_private_ipc_run_without_egress` | a local shell, a read-only project read, the `KIANA_SANDBOX_NETWORK_DISABLED=1` marker and a tool-owned private unix socket still work — local behaviour is not blanket-denied |
| `offline_stdio_mcp_boundary_keeps_pinned_mount_and_no_egress` | the stdio-MCP plan (one descriptor-pinned mount inserted before the `--` boundary, as `mcp_stdio::ConfinedMcpClient::connect` does) keeps its mount through the FD fence while egress stays denied |

`kiana-core/tests/cap11_ambient_authority_guard.rs` fails if any of these names disappears from the
module, the workflow or this baseline, if the workflow stops installing/verifying `bwrap` and
`python3`, or if a bypass marker such as `--share-net` reappears.

GitHub Actions runs the focused sandbox environment tests, the CAP-11 source guard and workspace
compilation. No local runtime tests or smoke commands were run. This is source/static evidence
(plus the CI-only behavioural fixtures); kernel-level network, namespace and descriptor behaviour
still requires the remote CI run and does not establish live/physical isolation by itself.
