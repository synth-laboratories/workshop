# Workshop v0.11 MQ recovery changes

SSE clients previously lost lag notifications silently and retained their initial
authorization for the connection lifetime. The stream now emits `resync` after
broadcast lag, rechecks the credential and thread membership before emitting
events and every five seconds while idle, and closes with `revoked` when authority
is unavailable.

The Rust client adds bounded cursor catch-up. It persists each page through a
caller-supplied atomic inbox/cursor callback before advancing its position. It
rejects foreign threads, sequence gaps, reordered messages, repeated message IDs
and oversized pages. HTTP requests have a 30-second deadline, refuse redirects,
and bound streamed JSON/error bodies to 16 MiB/64 KiB respectively.

Validation at `f76c636`: `cargo test --locked --workspace --offline` passed
42 tests; five Postgres/Redis tests remained explicitly ignored. HTTP fixtures
cover lag, quiet-stream credential expiry, commit failure/restart, malformed
pages, redirects and fixed/chunked response limits. This is not deployed
Postgres/Redis or real Workshop runtime qualification.

The branch starts at the release's existing MQ pin `626760d`. Before merging,
review the selected target branch and qualify the five infrastructure tests.
Fine-grained grants, local participant identity, subscriptions, SSE head-sequence
IDs, automatic stream supervision, Workshop network integration and restricted
message-triggered execution remain outside these changes and are still required
for the release. No slot or production image has been updated.

Review publication is pending: the connected GitHub app reports this repository
unavailable, and no GH_TOKEN/GITHUB_TOKEN was present. Remote publication was not
performed; further work must use an authorized credential mechanism.
