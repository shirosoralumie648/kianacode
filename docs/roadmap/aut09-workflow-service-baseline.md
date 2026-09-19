# AUT-09 workflow service baseline (partial)

`DaemonHost` now owns one bounded Tokio queue service. The service routes claim, heartbeat,
effect, fence, reclaim and ready-tick operations through a bounded `mpsc` channel and keeps the
ControlPlane/Broker execution spine separate. Shutdown drains accepted commands in order, fences
active leases and reports in-flight/unknown work as recovery-required.

The service is a coordination boundary, not a second model or capability loop. Its CI fixture
uses the in-process queue store; it does not prove scheduler timing, durable ack/fence events,
cross-process restart, or external effect reconciliation. AUT-10+ still owns trigger/timer and
workflow execution semantics.
