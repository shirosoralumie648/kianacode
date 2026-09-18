# ER-27 four-entry receipt and recovery command baseline

ER-27 keeps CLI, Workbench, Web and Desktop as views/adapters over one
`DaemonHost → ControlPlane → EventLog` path. Shared protocol/client DTOs carry receipt,
pending approval, resume, cancel, reconciliation and restore commands; entrypoints
construct envelopes and render safe projections only. DaemonHost authenticates the
principal, routes the envelope and returns the same status/error/receipt semantics.

Cursor gaps, reconnects and UI action cards remain read-only until the explicit command
is sent. No entrypoint parses or mutates EventLog, supplies an authority owner/scope,
starts execution from a receipt query, or auto-approves an action. GitHub Actions runs
CP-22 protocol-surface, P2-M3 action-card, P2-K3 Human Inbox, P2-M7 accessibility and
ER-27 source guards plus workspace target compilation. Local runtime tests and smoke
commands are intentionally not run.

This is source/static evidence only; desktop process supervision, external transport
and live/physical delivery remain outside this slice.
