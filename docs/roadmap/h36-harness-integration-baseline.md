# H36 Harness integration and evidence closeout baseline (source gate)

The H36 source gate indexes the CLI, TTY Workbench, loopback Web and Desktop surfaces against the
same DaemonHost → ControlPlane → KianaHarness/Provider spine. It also keeps cancellation,
result_unknown and receipt/live-handoff boundaries visible. Existing OA-24/ER-27 parity and
daemon-spine fixtures are CI inputs; no entrypoint may create a second Broker/model loop.

H36 remains partial. The repository has cassette/source coverage and a default-deny live handoff,
but no real Provider/account/target receipt was supplied or executed. Full short-task/tool
repair/steer/cancel/approval/compaction/restart matrix, Desktop state receipt, latency/stream
evidence and per-provider live verification remain open. Unsupported providers stay independent
and are not promoted by a single configured model path.

The CI-only HarnessIntegrationMatrix makes those eight scenarios explicit across CLI, Workbench,
Web and Desktop, requiring reasoned non-verified rows and receipt/fence/stream/Desktop state
evidence for Verified rows. `live_closeout_blockers()` and `live_closeout_ready()` now make the
missing per-surface live evidence, approval, Receipt and result-unknown reconciliation explicit.
They are coverage contracts, not runtime execution or live proof.

The H36 CI gate also indexes the existing ProviderLiveConnectionEvidence and LiveHandoffManifest
contracts and the common DaemonHost → ControlPlane → KianaHarness → ProviderGateway source spine.
The provider-live fixtures run without external credentials; a skipped or synthetic observation is
not a Verified live result.

## Source-gate handoff

The matrix remains `partial` until an operator supplies a named provider/account/model/profile,
budget and approval and produces independent live Provider/Receipt/cleanup evidence for the
required surfaces. H36 does not auto-enable live traffic on push.
