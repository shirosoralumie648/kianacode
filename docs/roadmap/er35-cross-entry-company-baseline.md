# ER-35 cross-entry and CompanyOS gate baseline

ER-35 now indexes the existing OA-24 entrypoint parity, OA-27 Company governance and EQ-12
DaemonHost spine fixtures through one CI-only source gate. The fixture catalog requires the four
entrypoints to project the same committed cursor/event/receipt facts, keeps Company runtime
completion separate from Review/Acceptance/Delivery/ClosingReceipt, and preserves Unknown→reconcile
and source-gap boundaries.

The gate does not create a second execution loop, local authority, Broker path or external effect.
It does not prove durable cross-process query/index rebuild, real browser/Desktop runtime, external
delivery, provider receipt or physical/live outcome. `feature_status=implemented` for this bounded
source/CI gate and `proof_level=source`; ER-35 remains partial until those runtime/durable limits
have independent evidence.
