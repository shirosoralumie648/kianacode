# ER-26 cursor query, snapshot and slow-consumer baseline

ER-26 separates the durable EventLog cursor from disposable UI/run-stream cursors.
EventLog pages advance only at committed transaction boundaries; cursor reads reject
non-boundary/gap requests and unsupported stores fail closed. Protocol/UI envelopes
carry daemon epoch, logical sequence, source cursor and projection version, so an epoch
change or sequence gap requires snapshot hydration.

RunStreamBus uses bounded broadcast channels, terminal replay and explicit gap signals;
slow/disconnected consumers never block EventLog commit. Web SSE, Workbench and CLI only
refresh/read snapshots and receipts after a gap or restart; they do not create a run or
treat deltas/UI events as authority. GitHub Actions runs P4-J7-02 sequence, OA-17
cursor, P2-M2/P2-M5 UI sync and ER-26 source guards plus workspace compilation.

This is source/static evidence only. It does not claim cross-process live-stream
durability, external replay storage or physical delivery guarantees.
