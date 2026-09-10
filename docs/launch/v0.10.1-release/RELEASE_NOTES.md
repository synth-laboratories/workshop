# Workshop 0.10.1 — candidate

This patch consolidates reviewed fixes from the v0.10 development branches.
It is not published until the exact packaged archive passes acceptance.
The published v0.10.0 artifacts remain unchanged.

- Annotation decisions persist and display in insertion order, including tied
  timestamps, without rewriting sealed evidence.
- Report edits recover across navigation and reload in the same browser session.
  Saving preserves newer edits and their revision base, and a late save does not
  navigate backward. Failed saves cannot silently seal an older draft.
- Root renderer failures offer a reload action.
- Source builds and the managed MLX runtime use the same immutable compatibility
  revision, including Workshop model aliases, managed model identity and
  serialized policy registration.

Distribution remains ad-hoc signed and **not Apple-notarized**. Build locally
using `scripts/install.sh` and `scripts/workshop.sh build-and-run`; TBLite is not
a production prerequisite. Report draft persistence uses session storage, not a
guarantee of recovery after a full app quit. AI/provider-backed acceptance is
excluded at the release owner's direction, not reported as passed.
