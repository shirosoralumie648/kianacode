# AUT-11 workflow event ingress baseline

AUT-11 adds a bounded signed event envelope, server-owned source/project/key allowlist, event-kind
and top-level payload filter, HMAC verification and source+event-id dedupe. A verified ingress is
converted into a typed occurrence whose only execution material is an `AutomationCommand::Fire`
with an `event:` evidence reference; payload fields never become a capability request or prompt.

The AUT-11 source/CI ingress slice is complete at the adapter boundary. The daemon verifier does
not append the ingress fact or execute Fire; the caller must persist the accepted event through
EventLog and then use the existing ControlPlane workflow route. External HTTP/Webhook transport,
durable dedupe across restart, replay receipts and live source credentials remain explicitly open.
