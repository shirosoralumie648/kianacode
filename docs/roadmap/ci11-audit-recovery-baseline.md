# CI-11 audit, redaction and credential recovery baseline

CI-11 adds a versioned `recovery.credential` RuntimeEvent and a source/fixture projection binding
identity, assignment, config, credential, audit and redaction digests to generation, authority
epoch and lease state. `kiana-core` folds the committed recovery facts, while `kiana-daemon`
exposes the same read-only projection bridge used by the EventLog-backed product path.

Unknown schema, stale epoch/revision/generation, missing or revoked lease, credential refresh
failure and `result_unknown` remain blockers. Restart defaults to
`re_admission_required`; only a fresh `ReAdmissionAuthorized` fact may set
`resume_authorized=true`. The opaque EventLog rotation adapter advances SecretRef generation by
CAS and never stores raw credential material.

The slice is still source/local-fixture bounded: the in-memory rotation adapter is not a durable
SecretStore, the recovery projection has no cross-process checkpoint, and no live provider
rotation, production redaction scan or physical external credential receipt is claimed.
