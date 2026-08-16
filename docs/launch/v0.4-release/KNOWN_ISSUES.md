# v0.4 known issues

- This friends package is ad-hoc signed and not Apple-notarized. Gatekeeper may require an explicit user open action.
- Updates remain manual and return to the official Synth Desktop download page.
- The Rust workspace and Clippy complete successfully but retain an existing warning backlog.
- The Banking77 smoke produced no lift: the generated proposal scored 0.85 versus the parent's 0.90 on the minibatch gate, so the seed remains the one-member frontier. This is a valid bounded-search outcome, not a release failure.
- If the managed Laguna process is terminated outside the app, the readiness indicator can remain stale until the next local request. The next request relaunches the sidecar and recovers successfully.
- Clean-account payment rehearsal and independent fresh-machine Gatekeeper review require a separate identity/device and were not represented as automated release gates.
