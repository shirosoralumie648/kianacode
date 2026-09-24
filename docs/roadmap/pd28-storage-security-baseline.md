# PD-28 · Storage security baseline

`PD-28` closes the persistence boundary for project artifacts and the JSONL EventLog. The
boundary is deny-first: secret sentinels are rejected before a file or fact is created, file
entries are confined to a pinned parent, and encryption metadata carries only an opaque
`SecretRef` bound to its owner scope, namespace and purpose.

## Source contract

- `kiana-domain::StorageEncryptionBinding` validates an opaque key reference, owner-scope,
  namespace, purpose and ciphertext/binding digests. Key material and redaction sentinels never
  cross the contract.
- `kiana-domain::StorageFileIdentity` records root/path, regular-file, symlink, hardlink and
  permission observations with a digest. Escape paths, symlinks, hardlinks, broad modes and
  digest drift fail closed.
- `StorageSecurityCapabilities` reports platform guarantees and limitations. Unix adapters prove
  `O_NOFOLLOW`, single-link regular files and owner-only modes. Non-Unix adapters perform an
  explicit symlink check while exposing that race-free no-follow and portable hardlink identity
  are not proven.
- JSONL journal and lock files are opened through the same identity checks. Unix rejects group or
  world permission bits and `nlink > 1`; non-Unix checks the directory entry before and after
  `OpenOptions` and retains the capability limitation.
- Project receipt, manifest and lessons writes validate structured JSON with the storage
  redaction contract (keeping opaque `secret_ref` values) or redaction-safe UTF-8 text before
  opening a confined path or creating a temporary file.

## CI-only fixture catalog

`pd28_storage_security.yml` runs the following on GitHub Actions; no runtime tests are run as a
local development step:

- `kiana-domain/tests/pd28_storage_security.rs`: secret sentinel/reference handling, path,
  symlink, hardlink and permission rejection, file-identity digest tamper, owner/namespace/
  purpose key binding and explicit platform limitations.
- `kiana-eventlog/tests/pd28_storage_security.rs`: secret append zero-effect, symlink/hardlink
  target protection and journal/lock permission rejection.
- `kiana-core/tests/pd28_storage_security_guard.rs`: artifact validation ordering and opaque
  reference preservation at the write boundary.

The workflow also runs formatting and workspace test-target compilation. CI output is evidence for
the source slice only; it does not establish power-loss, cross-host filesystem, encryption-provider
or physical deletion guarantees.
