# Live visual authoring acceptance

You are the visual author for an already-running isolated Synth Workshop instance. Your job is to turn the existing real Craftax rollout stream into a polished, useful, live replay—not to edit product source code or invent data.

Read these completely before acting:

- `apps/synth_desktop/skills/use-synth-visuals/SKILL.md`
- `apps/synth_desktop/skills/run-live-container-evals/SKILL.md`
- `visuals/families/first_class_example_containers/live.craftax.v1/README.md`

Use the `synth_visuals` MCP as the authority for visual state. Use Computer Use through the available local UI tooling to inspect the running Workshop app when possible. The repository sandbox is intentionally read-only; all authoring changes belong in the visual registry through MCP.

Two host-captured Workshop screenshots are attached to this run as independent UI evidence: a wide canvas and a narrower window. If the nested Computer Use bridge is denied, inspect these attached images directly and say so; never pretend the bridge succeeded. They show the current trusted template defaults. After configuration iteration, return to the same final settings before using them as final-revision evidence.

Acceptance procedure:

1. List visuals and select the existing visual bound to a real Craftax stream. Reject fixtures, guessed URLs, invented events, and any fake environment fields.
2. Fetch its authoring context. Confirm the binding uses slot `stream`, the canonical Craftax template, and a declared stream URL.
3. Show the visual and open its canvas presentation in Workshop. Inspect the actual rendered result at a wide desktop viewport.
4. Iterate at least twice using only supported template configuration. Prefer an immersive ember canvas with dense but legible information, activity, plots, temporal controls, and the Trace V5 inspector visible. Each meaningful configuration update must create a new revision.
5. At each iteration, inspect rather than assume. Look for: a dominant gameplay surface, the Containers-backed PNG image player with frame scrub and playback speed, legible summary/reward/achievement plots, separate rollout/evaluation time controls, the Full trace and Policy focus modes, no overflow, and honest missing values.
6. On the final revision, perform and record two reviews at distinct viewport widths matching the attached evidence (1574×768 and 1224×768). Record concrete findings. Mark every check true only when directly observed:
   - `rendered`
   - `noOverflow`
   - `primarySurfaceVisible`
   - `temporalControls`
   - `traceInspector`
   - `realEvidence`
   - `imageReplay`
7. If any required check fails, revise and repeat both final-revision reviews. Do not mark a deficient visual ready.
8. Mark the current revision ready only after the gate accepts it. Show the final visual again and verify its ready receipt.

Do not start a new paid rollout in this authoring acceptance. The existing stream is the evidence source. Do not claim that a digest is an image, do not turn missing reward/usage into zero, and do not claim a review you did not perform.

In the final response, report the visual id, final revision, configuration choices, both viewport receipts, observed real-data evidence, remaining defects, and whether the readiness gate accepted the visual.
