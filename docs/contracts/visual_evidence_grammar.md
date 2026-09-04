# Visual evidence grammar

Workshop visual surfaces use one small grammar for evidence state, measurements,
comparability, provenance, and readiness. Templates may choose their layout, but
they must not invent conflicting meanings for these concepts.

## Evidence state

`live`, `terminal`, `partial`, `stale`, and `unavailable` are distinct states.
Missing is not zero. Partial is not failed. Terminal means the declared source
has closed, not that a human has judged the visual correct. Stale means a prior
judgment no longer matches the current content or renderer identity.

## Measurements

A metric has a label, value, and optional qualifier. Producers leave absent
measurements absent; renderers show an em dash. A positive-reward rate is the
fraction of numeric rewards greater than zero. It must never be labeled as task
success unless the producer separately reports a task-success contract.

## Comparison scope

Cross-row ranking is permitted only when the payload declares a comparison
contract and every row carries that exact contract digest. The contract names
at least the benchmark and primary metric, and may also name evaluator, dataset,
and split. A run catalog is descriptive even if two values happen to share a
scale. The renderer never infers a winner across unverified contracts.

## Provenance

Derived payloads name their source tables, source run identities, projection
version, and source/payload digests. A reward component view also declares its
aggregation scope. A single-rollout event decomposition must not be presented
as a decomposition of a run-wide mean.

## Readiness identity

A ready certification binds:

- visual id and revision;
- content and bindings digests;
- template id and recursive template-package digest;
- renderer kind and renderer/source digest;
- build revision and executable digest;
- immutable screenshot bytes and viewport.

Any mismatch makes the gate stale. The stale gate cannot authorize a seal or a
VisualsBench export. Certification requires a clean committed renderer build.

## Capture lifecycle

Review captures are content-addressed and append-only. The adjacent observation
receipt carries the screenshot digest and the complete certification identity.
The review endpoint re-hashes the image and compares the identity. Readiness
selects the latest passing review at each required width and copies their
immutable receipts into the quality gate.
