# CM-38 context/memory end-to-end fake and live-opt-in baseline (partial)

CM-38 now has a CI-only guard tying the existing FakeProviderAdapter, harness memory broker,
EventStore memory journal, candidate/approval/projection boundaries and live handoff contract into
one evidence checklist. The fake path is bounded and offline; live opt-in must still use a
non-secret reference, fixed scope, operator approval, redaction, independent receipt and cleanup.

`ContextMemoryGoldenPathEvidence` now gives the checklist a typed, digest-bound shape. It requires
scope and redaction profile digests, records ContextPlan/provider request/tool receipt/retrieval/
candidate/approval/projection/recovery/run-receipt stage digests, and keeps fake cassette,
explicit live opt-in, proof level, usage-independent limitations and provider-live evidence
separate. A fake cassette cannot claim live proof; incomplete or non-verified paths must retain a
bounded limitation.

This does not claim a durable full golden path or live Provider proof. No local test or external
request runs in this task; real context selection, candidate approval, restart recovery, index
maintenance and live scope/redaction receipts remain partial. Mock/fake output cannot promote a
live provider or prove physical effect.
