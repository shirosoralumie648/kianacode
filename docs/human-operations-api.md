# Local human operations

These commands use `RequestEnvelope::command` through the same DaemonHost and ControlPlane as
runs. They do not introduce model tools or a second agent loop. The CLI accepts
`kiana command <name> --arguments '<JSON>' --session-id <id> --project-root <directory>`;
`--role reviewer` selects a separate human review session and `--permission-profile balanced`
enables state mutations. A session's role assignment remains frozen; each approval retains its original permission profile.
Workbench has `/inbox` and `/command <name> <JSON>`. Web exposes the same actions in its human
inbox, edit snapshot and improvement suggestion panels. Human decisions remain explicit.

`human.inbox` returns `{schema, revision, items}`. Items contain Approval, Review, Acceptance,
Incident, Reconciliation or Feedback plus their server-derived available actions. `human.resolve`
takes `item_id`, `action_id`, the returned `inbox_revision`, `fields` containing only the action's
listed required fields, and a stable `idempotency_key`. Changed inbox state or unexpected fields
are rejected. Approval resolution uses the original one-use challenge/proof. Review, acceptance
and business incidents invoke their original Company commands with the current frozen Company
revision; they are not independently approved by the inbox. The Web approval button uses the
same dedicated approval protocol directly, with the exact server challenge.

`failure.incidents` projects the six failure classes (crash, timeout, cancel, disk full, MCP
failure and provider uncertainty) from owned EventLog facts. Each incident includes a recovery
plan with automatic retry disabled. A run with no terminal fact and no continuation in this host
is explicitly marked unconfirmed; that does not assert that another process stopped. Result
Unknown incidents enter the reconciliation portion of the inbox. `failure.reconcile` takes
`incident_id`, `resolution` (`observed_succeeded`, `observed_failed` or `no_effect`), owned
`event:<EventId>` evidence references, `expected_revision`, and `idempotency_key`. The failure
event alone cannot be its own reconciliation evidence. Reconciliation records never overwrite a
Run outcome and never execute a retry. If a full disk prevented all failure evidence from being
written, it cannot be reconstructed as a proven disk-full event from an absent record.

`feedback.list` returns the candidate collection and its revision. `feedback.submit` takes
`candidate_id`, `category` (`quality`, `workflow`, `prompt`, `reliability`, `usability`),
`observation`, `proposed_change`, owned event evidence, `expected_revision`, and
`idempotency_key`. It creates only a candidate. `feedback.review` requires a Reviewer or Sponsor
session separate from the submitting session, `candidate_id`, `outcome` (`quality_approved` or
`rejected`), evidence references and the same revision/idempotency contract. Approval records a
quality gate result on the candidate; no Role, Grant, Policy, historical fact or deployed prompt
is changed by either operation. Shared human state is folded from a CAS EventStore stream.

`workspace.checkpoint.list` returns this session's immutable edit checkpoint metadata.
`workspace.checkpoint.create` accepts `paths`, an explicit array of authorized relative file
paths. `workspace.checkpoint.preview` accepts `checkpoint_id` and only reads files; its response
contains the current revision, target revision and file changes with `writes_performed: false`.
`workspace.checkpoint.restore` takes the checkpoint ID and `expected_revision` from that preview,
then requests Critical-risk approval. Approving it rechecks the immutable stored snapshot,
session, actor, role, original WorkPacket, current Company project state, path scope, data epoch
and current file revision. Closed/archived projects and paths beyond the original packet fail
closed. The restore path revokes all pending project approvals and stops old runner contexts
before writing. An unconfirmed stop prevents restoration. The EventLog marks restoration intent
before mutation, so a crash also invalidates stale runner snapshots. A successful result creates
a separate `workspace.restored` event; restoring a snapshot does not rewrite transcript history.

Input boundaries and side-effect invocations automatically record an EventLog transcript offset,
workspace revision, run and invocation binding. `apply_patch` captures its exact target files
before execution, including deleted and newly created paths. User-input boundaries and effects
whose file write set is unknown (such as shell/MCP) record metadata only and do not promise an
undo for those effects. Explicit file snapshots and patch snapshots support bounded UTF-8 edits
(128 files, 128 KiB per file, 2 MiB total). Revision hashes cover precisely the stored file set.
Binary files, protected metadata, detected secret content, hard links and symlink traversal are
rejected. Linux reads use descriptor-relative no-follow traversal; other platforms currently
reject file capture rather than use an unsafe fallback. Permission changes and recreation of a
deleted executable need a separate explicit operation. Restoration uses the existing patch
lock, preconditions, descriptor-relative writes and rollback, not a shadow Git repository.

This slice has source-level evidence only. `cargo check -p kiana-entrypoints --locked --offline`
and JavaScript syntax checks passed in the implementation snapshot. No tests were added or run,
as requested. Runtime, crash recovery, durable and live proof levels have not been promoted.
