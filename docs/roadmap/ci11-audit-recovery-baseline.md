# CI-11 audit, redaction and credential recovery baseline (partial)

CI-11 adds `CredentialRecoveryProjection`, a source/fixture contract binding identity,
assignment, config, credential, audit and redaction digests to generation, authority epoch and
lease state. Unknown schema, stale epoch/revision, missing lease, credential refresh failure and
`result_unknown` remain blockers; restart defaults to paused/re-admission-required and only an
explicit re-admission may authorize resume.

The contract is not an EventLog projector or SecretStore implementation. CI-11 remains partial:
no cross-process recovery receipt, live credential rotation, durable audit query or production
redaction scan is claimed.
