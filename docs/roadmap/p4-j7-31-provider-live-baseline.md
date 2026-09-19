# P4-J7-31 provider live boundary baseline (partial)

The existing provider live smoke is now indexed by a CI-only source boundary. Required mode
fails when no selected connection is configured; skip-if-unconfigured is explicitly a skipped
observation and never a live success. The smoke requires live catalog/tools flags and at least one
non-fake text and tools result, while the ProviderGateway keeps credential revision, model route,
usage and Receipt boundaries.

The domain now also exposes `ProviderLiveConnectionMetadata` and
`ProviderLiveConnectionEvidence`. The former is the secret-free selected-connection identity
(`provider_id`, connection, protocol, requested model, configuration digest, credential revision
and provider-account digest); the latter requires separate per-connection status, authorization,
budget, reported model, usage/Unknown, text/tools, delta, cancel, Receipt and artifact evidence.
Verified evidence rejects synthetic sources and incomplete observations; skipped/unverified rows
must retain a bounded limitation.

No live request is executed by this source gate. P4-J7-31 remains partial/unverified until each
authorized connection supplies an isolated account, fixed protocol/model/profile, budget, text
and tool round trips, native delta/cancel evidence, requested/reported model, usage/Unknown,
Receipt/artifact hash, retention and cleanup. Synthetic streaming, one provider path and CI
compilation cannot close the card.
