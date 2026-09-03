# Handoff — visual QA sweep, 2026-09-03

Branch `codex/capture-review-pipeline`, 10 commits, `4d6c898a..3f21eb5f`. **No upstream
configured; nothing is pushed.** 338/338 visuals tests, `visuals/` and app typecheck
both clean.

The work was: capture every visual surface, look at it, fix what the machine audit
cannot see. Every defect below was reported as `0 findings` by the capture audit —
correctly, because none are geometry or legibility faults. They are claims the surface
makes in words and numbers.

---

## 1. How to run the loop

The instance is `visualqa` (v0.9), a **cua-live shell against Vite**, so renderer and
template edits hot-reload with no rebuild.

```bash
cd /private/tmp/claude-501/-Users-joshuapurtell-GitHub/<session>/scratchpad/loop

# list visuals / runs
node mcp.mjs visualqa visuals visual_list '{}'
node mcp.mjs visualqa optimizers optimizer_list_runs '{}'

# capture one visual, isolated, deterministic
node mcp.mjs visualqa display workshop_capture '{"scope":"visual","target":"vis_..."}'
# -> {"path": ".../data/surface-captures/visual-vis-...-<ts>.png"}
```

`mcp.mjs` and `tools.mjs` (tool lister) are in that scratchpad — **copy them into the
repo if you want them to survive**, they are session-scoped today.

If the instance is down:

```bash
./scripts/desktop-instance.sh stop visualqa
SYNTH_BROWSER_RUNTIME_OPTIONAL=1 ./scripts/desktop-instance.sh cua-live-build visualqa
./scripts/desktop-instance.sh cua-live visualqa
```

- `rebuild-run` will **refuse**: it requires a clean checkout and the tree is dirty
  (see §4). `cua-live-build` is exempt by design.
- `SYNTH_BROWSER_RUNTIME_OPTIONAL=1` is required unless you have an assembled browser
  runtime. The resulting bundle cannot drive a browser; irrelevant for visual QA.
- `cua-live` starts Vite on a fixed port and will fail if one is already listening.
  That is usually fine — the app attaches to the running server.

Verification before any commit:

```bash
cd visuals && npx tsc -p tsconfig.json --noEmit    # visuals project
npm run typecheck                                   # app project
npm run test:visuals                                # 338 tests
npx esbuild <edited>.tsx --loader:.tsx=tsx --bundle --outfile=<scratch>/x.js \
  --external:react --external:react-dom --external:react/jsx-runtime
```

The esbuild step is not optional. `tsc` and the test suite **both passed** on a file
Vite could not parse, which is why `visuals/tsconfig.json` exists at all. Do not write
the outfile to `/dev/null` — esbuild also emits a `.css` sidecar and fails on
`/dev/null.css`, which looks like a parse failure and is not.

---

## 2. Coverage: what has been looked at

Reviewed by eye, defects found and fixed:

| Template | Containers |
|---|---|
| `trace.workbench.v1` | Banking77 (annotated + eval), Craftax, HealthBench |
| `experiment.overview.v1` | Banking77 (completed + failed), HealthBench |
| `optimizer.sft.live.v1` | Banking77 (completed + running), Craftax |
| `optimizer.cispo.live.v1` | Banking77 (failed) |
| `optimizer.gepa.live.v1` | Banking77 (failed) |
| `live.annotated_rollouts.v1` | Banking77, Craftax, HealthBench |
| `live.craftax.v1` | Craftax |
| `trace.rollout_inspector.v1` | Banking77 |
| `analysis.chart.v1` | probe chart authored by hand |

Not reviewed, and **why it is not just laziness**: `trace.workbench.v1` × glm/llama are
the same template on dead runs; the surface is now the best-covered one in the repo.

---

## 3. What is left

### 3a. Thirty templates have no visual instance at all

`visual_list_templates` returns 39; 9 have instances. The rest have never been
rendered by anything: `live.harbor_eval.v1`, `craftax.rollout_scrub.v1`,
`craftax.eval_matrix.v1`, `optimizer.gepa.frontier.v1`, `optimizer.sft.dataset.v1`,
`optimizer.sft.checkpoints.v1`, `optimizer.sft.rollouts.v1`, `optimizer.sft.examples.v1`,
`optimizer.sft.lineage.v1`, `model.compare.v1`, `reward.breakdown.v1`,
`posttrain.rollout_viewer.v1`, `trace.catalog.v1`, `live.container_rollouts.v1`,
`live.eval_stream.v1`, `live.intern_acceptance.v1`, `analysis.annotation_workbench.v1`,
`annotation.overlay.v1`, `compose.visual.v1`, `sourced.visual.v1`, the three `diagram.*`,
and others.

**This is the largest remaining gap and it needs a decision, not just effort.** Most of
these need a real bound run to populate them. Fabricating plausible data would defeat
the purpose — the whole value of this sweep was that the defects only showed up against
real evidence. Pick the templates worth standing up a run for.

Two are self-contained and can be exercised immediately:
- `analysis.chart.v1` — done, see §3d.
- `diagram.mermaid.v1` — `visual_create` with `arguments.content`; untried.

### 3b. `visual_chart` spec shape is easy to get wrong

Four attempts failed before it worked. The traps, all in
`apps/synth_desktop/src-tauri/src/visuals/charts.rs`:

- `spec` is an **object** in the MCP call, but is serialized and parsed as a string by
  `parse_and_validate`, so a shape error surfaces as
  `chart spec must be valid bounded JSON` rather than a field error.
- `version: 1`, not `schemaVersion`.
- `MetricItem.value` is a **`String`**, not a number.
- `BarSeries` uses `name`, not `label`.
- Every panel is `deny_unknown_fields`.

Worth a schema example in the tool description; it cost more turns than the QA did.

### 3c. Observation, not yet a defect: null bars

In `analysis.chart.v1`, a `null` category renders as a short **grey stub** — clearly
distinct from the orange zero-lines beside it, so the "never renders as zero" contract
holds. But the declared contract says "a gap or a hatched cell", and a short bar can
read as a small measured value. Someone should decide whether the stub is good enough.

### 3d. The probe visual

`vis_03a4969a3c2c48318688409fec67da2a` ("QA chart probe") is a draft I authored to
exercise the chart template. No delete tool is exposed on the visuals adapter; remove it
through the UI if you want the registry clean.

---

## 4. Gotchas that will bite you

**A concurrent agent is working in this tree.** ~68 files are modified and uncommitted
and they are not yours. Consequences:

- **Never `git add <file>` without checking the diffstat first.** Staging
  `container_eval.rs` for a 7-line change pulled in **80 insertions** of someone else's
  `task_instance_id` work. Recover by staging a filtered patch:
  ```bash
  git diff -U3 <file> > /tmp/full.patch
  # keep only the hunks containing your change, then:
  git apply --cached /tmp/mine.patch
  ```
- **Never `git stash`.** The other agent's work is unbacked.
- 14 `npm run test:a11y` failures are theirs — every failing assertion names
  `App.tsx` / `routes.tsx` / side-panel CSS. The 34 tests covering files touched here
  all pass. This was verified by reading the assertions, not by a clean-tree baseline
  run, because getting one would require stashing.

**Do not trust a script's exit code through a `;` chain.** I reported three "silent
exit-0 failures" from `desktop-instance.sh` that were nothing of the kind — the shell
was returning the exit code of my trailing `echo`. The script is correct. Use
`cmd > log 2>&1; echo $?` on its own line, and read the log tail.

**Capture used to fail on repeat.** Fixed in `4d6c898a`, but know the symptom in case it
regresses: capturing the *same* visual twice in a row failed 100% of the time, while the
page displayed it correctly the whole time. The acknowledgement was keyed on the
selected visual, so a request for the visual already on screen changed no dependency and
the effect never re-ran. There is now a request counter plus a timeout racing the
`requestAnimationFrame` (macOS suspends rAF for occluded windows, and capture is driven
from a terminal).

---

## 5. Corrections to the record

Three claims in earlier commit messages and reports were wrong. They are corrected in
later commits, but if you read the history in order you will hit them:

1. `ba043d4e` says `run.error` was "not reaching the renderer, which is host-side".
   **Wrong.** It is stored in `payload_json`, verified directly in the instance DB, and
   carried end to end. `normalizeRun` in `FamilyShell.tsx` was dropping it. Fixed in
   `f70fb02e`.
2. I stated `OptimizerRunRecord` has no `error` field. It does, `models.rs:789` — I had
   truncated a `sed` range at 790.
3. I reported `desktop-instance.sh` exits 0 on failure and called it the highest-value
   fix available. It does not. See §4.

All three came from stopping a trace one level short and reporting the intermediate
conclusion as fact. The pattern to copy instead is what found the real bug: check the
database, check the type, check the normalizer, in that order.

---

## 6. Recommended order for whoever picks this up

1. **Push the branch.** Ten commits exist only on this machine.
2. Decide §3a — which unexercised templates justify standing up a run. That is the only
   remaining work with real defect yield, and it is a scoping call, not an engineering one.
3. If you want cheap coverage first: `diagram.mermaid.v1` needs no run data.
4. Fold `mcp.mjs` / `tools.mjs` into `scripts/` so the capture loop is not
   session-scoped.
5. Consider whether §3b deserves a schema example in the `visual_chart` tool description.

The defect rate held at roughly **two per surface across nine surfaces**, and it did not
fall off as the sweep went on — the last two surfaces produced the traceback
misattribution and the empty-column scoreboard. Assume unreviewed surfaces still hold
defects at that rate.
