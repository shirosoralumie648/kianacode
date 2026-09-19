# CAP-33 Container / gVisor execution baseline (partial)

CAP-33 now has a product-path EnvironmentPort adapter for a digest-pinned OCI image. The
adapter constructs a non-root, read-only, networkless, capability-dropped launch plan with
bounded CPU/memory/process limits, an isolated /workspace bind mount, an explicit environment
allowlist, and owner/scope/plan labels. It probes the configured runtime, refuses unpinned
images, and verifies labels before execute/quiesce/dispose. Timeout cleanup stops the container
and reports result_unknown when inner-process stop cannot be confirmed.

The required negative cases remain explicit:

- container_never_mounts_host_control_socket_or_credentials
- container_runtime_failure_never_falls_back_to_host
- container_cancel_confirms_inner_process_stop

The plan registry is process-local until an EventLog-backed environment inventory/lease projection
is connected, so daemon restart recovery is unavailable rather than inferred from a container
name. Shell/MCP routing through this adapter, changeset/artifact publication, disk-full fixtures,
remote cleanup recovery, and a target CI runtime fixture are not yet complete. runsc is an
explicit runtime selection, but no gVisor machine receipt exists. CAP-33 therefore remains
partial with behavior_verified=false; the source and static compile gate must not be promoted
to live, durable, or physical proof.
