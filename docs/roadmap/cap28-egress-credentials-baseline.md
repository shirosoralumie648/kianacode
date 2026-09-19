# CAP-28 controlled egress and minimum credential baseline

CAP-28 records the server-owned egress and credential contracts already present in the local
spine. `NetworkPolicy` is an allow-list/require-TLS policy; an adapter supplies a bounded DNS
address observation and the contract validates every address, IPv4/IPv6 host match, loopback,
private, link-local, unusable and metadata ranges. Policy and resolution digests are bound to the
effect, so a resolver refresh or policy revision is a new admission decision.

Credentials are references, never raw values, in domain/protocol facts. `CredentialLease` binds a
provider account, purpose, audience and endpoint digest with expiry, one-shot consumption and
rotation/revocation generation checks. Trusted provider adapters resolve/inject at the effect
boundary; arbitrary shell/model/event payloads do not receive secret material. Missing, expired,
revoked, mismatched or unsupported stores fail closed.

The acceptance boundary remains explicit: direct connect or proxy-env override cannot bypass the
egress policy; DNS rebinding/redirect to local or private addresses is denied; a credential for
one origin cannot cross to another. This slice does not claim a production proxy, live registry,
or physical network interoperability; those require target-environment evidence. GitHub Actions
runs policy, credential, OAuth and source-guard fixtures. No local runtime tests or smoke commands
were run.
