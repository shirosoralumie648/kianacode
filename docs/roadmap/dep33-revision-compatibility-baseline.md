# DEP-33 revision pin and drain baseline (partial)

The domain now defines an immutable ExecutionRevisionPin containing build, workflow, provider,
extension catalog, project-skill trust and replay-schema digests. Replay compatibility requires
all of those inputs to match; silent definition/provider/extension swaps, unknown replay versions
and project-skill trust drift fail closed.

RevisionDrain models active → draining → retired with a bounded deadline, replacement-ready
condition and active run/writer counts. It cannot retire while any run or writer remains, and a
deadline overrun is visible.

DEP-33 remains partial with source/static evidence only. Workflow/runner/provider/skills/plugins
are not yet wired to persist or consume the pin, old revisions are not durably fenced/drained,
and no live replay or upgrade receipt exists.
