# Rhodes evaluation observation

Workshop attaches to an evaluation already accepted by the public backend. Use
`optimizer_manage` with `operation: "reconcile_rhodes"` and arguments `pool_id`,
`rollout_id`, and optional `open_visual`. The dedicated
`optimizer_reconcile_rhodes` tool exposes the same operation. The local IPC route
is `POST /v1/optimizers/reconcile_rhodes` with camelCase fields.

The existing Synth backend configuration supplies the endpoint and API key.
This adapter does not access the Secrets registry or Keychain. Its only remote
operations are authenticated rollout and event-page reads. Deployment mutation
and execution remain owned by Rhodes; closing Workshop does not cancel either.

Each mirror is bound to the backend endpoint, pool, and rollout. A changed
backend configuration requires an explicit new attachment. The local delivery
sequence is allocated independently of the source sequence. One database
transaction commits the events, projection and source cursor. Gaps, malformed
pages and ownership mismatches stop ingestion before advancing that cursor.

A scientific terminal result and provider cleanup are separate facts. Later
cleanup events are evidence amendments linked to the existing terminal record.
The observer stays attached until terminal status, drained pages and explicit
`cleanup_pending: false`, and no pending result publication. An older backend without this field cannot certify
cleanup; its state remains unknown. The inspector shows score, cleanup, trace
publication, limits and source cursor independently. Missing scores remain null.
A named result snapshot preserves the rollout read; it never supplies the replay
cursor or overrides the event page's status.

Reads are bounded to 200 events and 4 MiB per response. One observer runs per
mirror, at most 64 per Workshop process. A capacity pause is recorded visibly;
Refresh or reconcile resumes observation when capacity is available. Ten
consecutive read failures pause observation with a durable error. Backend jobs
continue independently. Startup scans persisted mirrors with stable ID-based
pagination and resumes unfinished observers. Refresh uses this same adapter.

This is operational replay and a local durable mirror. It does not establish
Trace V5 custody, settled billing, scientific validity, or target visual-frame
publication merely because an execution completed. Those retain their own
receipts and authority.

Result publication has its own inspector field and pending replay signal. The
observer rereads the rollout after a terminal event page so completion between
reads cannot seal a mirror with a stale score or publication snapshot. This
continues to preserve the source cursor as the sole replay authority.
