# SC-42 recovery and retention rehearsal baseline (partial)

SC-42 adds a fake/source-bound rehearsal matrix for restart, quarantine restore, replay,
`result_unknown` reconciliation and retention prune. Success requires old-lease fencing, no
duplicate effect, quarantine verification, committed retention watermark and legal-hold respect;
Unknown requires reconcile and forbids automatic retry.

GitHub Actions validates the existing package lifecycle, OA-28 live-handoff and OA-26 durable
observability script boundaries and runs the domain/source fixtures. It does not delete data,
restore a production root, kill a live process, contact a provider or claim durable/live/physical
proof. SC-42 remains partial until approved local-durable and target-environment rehearsals
produce independent receipts and cleanup evidence.
