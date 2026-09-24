# UI-09 schema registry

This directory is the canonical JSON Schema boundary for the versioned UI DTOs used by
`kiana-protocol` and the typed clients. `schema-lock.v1.json` is the checked-in lock: it binds each
schema id to its Rust owner, unknown-field policy, byte ceiling, redacted examples and content hash.
The lock is data, not an execution authority.

## Generation boundary

JSON Schema is canonical at the wire boundary. Rust DTOs remain hand-written in
`kiana-protocol/src/ui_contracts.rs` (and the UI entity projection in `kiana-client/src/ui_store.rs`);
`kiana-client/src/ui_schema_generated.rs` is a generated, static catalog of the locked ids and
limits. TypeScript generation is deliberately deferred until a browser package exists. This keeps
the CLI, Workbench, Web and Desktop clients on the same versioned contract without introducing a
second runtime parser or execution loop.

Regenerate the static catalog and refresh schema hashes with:

```bash
python3 scripts/validate-ui09-schemas.py --write
```

The default command is a read-only gate:

```bash
python3 scripts/validate-ui09-schemas.py
```

It checks deterministic lock ordering, schema ids and hashes, examples, strict unknown-field
fixtures, byte ceilings, redaction/path rules, compatibility rules and generated catalog parity.
CI is the authority for running this command; local tests and builds are intentionally not part of
the UI-09 evidence.

## Compatibility policy

`compatibility-matrix.v1.json` fixes same-major changes to additive-only evolution. Required-field
removal, type changes, state meaning changes and unknown major versions fail closed. Unknown command
names are rejected with a schema-unsupported outcome before a client can construct an action. The
matrix records old-client/new-server read directions but does not claim live cross-version behavior;
that remains an integration concern for later entrypoint cards.

Every contract has a valid redacted example and an explicit unknown-field rejection example. Examples
use synthetic ids, digests and workspace labels; they contain no tokens, secrets, real paths or
provider payloads.
