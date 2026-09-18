# CAP-08 shared PathResolver and file identity baseline

CAP-08 adds a shared `PathResolverPort` contract with explicit operation kind, root/
parent/target identity, missing-target semantics, resolution digest and parent-handle
binding. Unsupported resolver phases fail closed. Consumers must revalidate identity
before opening or mutating a path.

The existing patch, staged execution workspace, sandbox and memory adapters are guarded
as consumers: descriptor-relative `openat`/`renameat`, O_NOFOLLOW-style symlink and
hardlink rejection, root/parent identity checks, role/path containment, private-component
masking and bounded scope traversal. Read, replace, create, delete and rename source/
destination semantics remain distinct; lexical normalization alone is not treated as a
TOCTOU proof.

GitHub Actions runs CAP-08 source guards with SC-13 path/TOCTOU regression, CAP-07
backend and scope fixtures plus workspace target compilation. Local runtime tests and
smoke commands are intentionally not run.

This is source/static evidence only; kernel-specific openat2 coverage, non-Linux handle
implementations and physical filesystem race guarantees remain open.
