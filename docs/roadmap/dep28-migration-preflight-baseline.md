# DEP-28 read-only migration preflight baseline (partial)

The domain now evaluates a bounded nine-axis preflight matrix: store, schema, projection,
workflow, provider, config, space, clock and lease. Every axis returns Ready or Blocked with a
stable reason, remediation and observed digest. The report binds the MigrationRegistry digest,
fact digest, source/target format versions and generated time; it is strict, deterministic and
explicitly read-only with effect_calls=0.

The deny-first matrix covers major mismatch, downgrade, concurrent runner, insufficient space,
active Unknown work, old writer, untrusted clock, digest drift and an unverified backup. A blocked
preflight remains a report and is never treated as a skip or permission to apply.

DEP-28 remains partial with source/static evidence only. The facts are supplied by adapters, but
there is not yet a durable store/projection/lease collector, backup verifier, migration runner or
runtime receipt. No preflight code invokes a provider, shell, MCP, filesystem write or broker.
