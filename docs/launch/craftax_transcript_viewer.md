# Craftax transcript-first trace viewer

## Root cause and design

The previous `live.craftax.v1` information architecture made gameplay and a semantic Trace V5 tree the primary surfaces. Policy deltas were correctly folded, but lifecycle events could become the selected inspector item, which made the model interaction read as “not applicable.” That was especially misleading for reasoning and tool evidence: the selected lifecycle record described neither the provider's emission behavior nor the nearest model call.

The new common path is a chronological model-call transcript. `visuals/runtime/agentTranscript.ts` is a transport-independent projection over already-ingested `LiveEvalEvent[]`; it does not discover bindings, open streams, or parse traces at app startup. Each `ModelCall` preserves its source sequence range, raw envelopes, provider/model/authority, environment-step association, usage, and explicit evidence states. Craftax supplies replay context around this reusable contract rather than placing Craftax logic in core.

Replay, transcript, raw trace, metrics, and integrity are separate surfaces. The evaluation cutoff remains mounted and drives all five. Environment-step buttons select the associated call only when the evidence supports that link. Focus mode reconciles to the first policy call, never a lifecycle item. Selection keys are durable call number plus opening sequence and reconcile across partial/completed replay and revision replacement.

Evidence labels distinguish recorded/visible, recorded/redacted, provider-not-emitted, not-applicable, pending, and producer-contract defect. The UI never infers private reasoning. “Thinking not emitted” is used when no reasoning envelope exists. Tool results are only not-applicable when no tool call was emitted; otherwise absent results are provider-not-emitted.

Large traces remain bounded at the normalized-call layer, and the source envelopes are rendered only inside collapsed `<details>` elements for the selected call. The existing Trace V5 viewer remains available on the Raw trace surface.

## Replay/capture integration plan

This branch starts at committed base `fb2acf7`. The shared `v0.4-replay-and-capture-contracts` checkout had uncommitted work and was inspected read-only.

After that work is committed:

1. Rebase `v0.4-craftax-transcript-viewer` onto the replay/capture branch tip.
2. Resolve `live.craftax.v1/shell.tsx` in favor of the replay branch's `LiveTemplateProps`, host-owned `ReplayClient`, explicit transport state, revision prop, and replay-authoritative frame base URL.
3. Retain this branch's `projectAgentTurns(visibleEvents)` boundary. Do not restore binding-derived `sseUrl` discovery or introduce a second stream consumer.
4. Preserve the replay branch's fixture rule: fixtures are local authoring evidence and never stand in for a declared stream.
5. Pass the replay revision into selection reconciliation; keep the call ID when present and otherwise choose the first call in Focus or the nearest extant call in Full.
6. Re-run replay transport, binding-envelope, 10-lane Craftax, transcript, responsive, accessibility, and performance suites. Verify frames still resolve relative to their emitting stream authority.

No uncommitted replay/capture changes were copied, overwritten, or cherry-picked.
