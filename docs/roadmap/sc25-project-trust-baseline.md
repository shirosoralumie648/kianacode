# SC-25 ProjectTrust root baseline

## Delivered source slice

- `kiana-policy` now defines versioned `ProjectTrustRoot` and `ProjectTrustResolution` values
  with separate User, KIANA_HOME and Project scopes and explicit source priority.
- Resolution is bound to a canonical project-root digest and same-scope roots only. Missing,
  unknown, untrusted and same-revision conflicting roots return stable deny reasons.
- Root and resolution records carry redacted audit references and self-verifying digests, so a
  loader can record the decision before reading skills, plugins, hooks or MCP configuration.
- Existing filesystem adapters remain responsible for obtaining canonical/root digests and for
  invoking the policy contract; this step does not add a second loader or execution path.

## Boundary and proof ceiling

The policy contract does not inspect the filesystem, load project resources or grant capability
permissions. Existing `kiana-types`/skills adapters remain compatibility surfaces; their local
file durability and all-entrypoint wiring still require later integration evidence. This slice
does not claim live signed trust roots, cross-process recovery or production authentication.
