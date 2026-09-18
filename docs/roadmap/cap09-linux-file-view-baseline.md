# CAP-09 Linux minimal file view and write-set baseline

CAP-09 narrows the Linux environment view to explicit toolchain/system files, the
project root and private `/tmp`/cache, with `/proc`/`/dev`/network/capability limits and
credential/private-component masking. Mount sources stay descriptor-pinned; no host
root, service store, socket or credential path is implicitly visible.

Read-only execution has no host workspace write path. Workspace-write uses a private
staged `ExecutionWorkspace`, exact `path_allow` scope and baseline identity, then
publishes only through the existing ControlPlane/patch commit boundary. Symlink,
hardlink, special-file, mount-source swap, parent-scope and private-path violations
fail closed. Existing harness shell/apply-patch fixtures cover read-only/write behavior;
GitHub Actions runs them with CAP-07/CAP-08 source guards and CAP-09 guard.

This is source/static evidence only; actual kernel mount/userns/network enforcement and
physical host secret/socket probes remain outside local proof.
