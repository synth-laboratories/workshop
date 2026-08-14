# Handoff — test Gemini 3.7 Flash in Workshop

**For:** an engineer proving the OpenRouter Gemini 3.7 Flash picker in a named Workshop instance.
**Ticket:** [SYN-3216](https://linear.app/synth-ai/issue/SYN-3216)
**Do not push. Do not merge onto the v0.2 release line.** The change is uncommitted.

This is the first v0.3 cut item. Everything else in v0.3 stays parked.

---

## 1. Tree (use this one)

| | |
| --- | --- |
| Worktree | `/Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-gemini` |
| Branch | `josh/v03-gemini-flash-openrouter` |
| Forked from | `josh/aug12-optimizers-workshop-visuals` @ `54bfcb7` |
| Status | **dirty, uncommitted.** Live OpenRouter turn has not been proven. |

Do **not** test from `~/Documents/GitHub/workshop`. That checkout does not have this work.

The instance launcher on this branch still brands as **v0.2** (`instances/v02/`, title `Synth Workshop v0.2 · <name>`). That is expected: the branch is cut from the v0.2 tip. It is not permission to land this on the v0.2 release branch.

---

## 2. What should appear

| Field | Value |
| --- | --- |
| Picker row | **Gemini 3.7 Flash** in the Remote group |
| Target id | `openrouter-gemini-flash` |
| OpenRouter slug | `google/gemini-3.7-flash` |
| Reasoning | Low → Medium → High → XHigh → Max (default Medium) |
| Input | text + image |
| Context | 1,048,576 |
| Tariff (verified by live OpenRouter usage 2026-08-14) | $0.375 / $1.875 per 1M; cache read $0.0375; cache write $0.02085 |
| Mark | Google logomark |

Gating is the existing OpenRouter path (`targetId.startsWith("openrouter-")`). No Synth Cloud, no Gateway, no new BYOK surface.

---

## 3. Launch

Named testing instances follow [`TEST_INSTANCE_LOGIN_CONTRACT.md`](./TEST_INSTANCE_LOGIN_CONTRACT.md). From **this worktree root**:

```bash
cd /Users/joshuapurtell/Documents/Codex/2026-08-14/let/worktrees/workshop-v03-gemini
./scripts/desktop-instance.sh dev gemini
```

Equivalent npm form: `npm run desktop:instance -- gemini`.

Do **not** use `npm run desktop:dev -- gemini`. That script hardcodes `codex` and will ignore the extra name.

Do **not** prefix the command with `OPENROUTER_API_KEY=…`, credential paths, or other env exports. The launcher copies allowlisted keys (`SYNTH_API_KEY`, `OPENROUTER_API_KEY`) from `~/.synth-desktop/.env` into the instance `data/.env` on every start.

If Settings → Account shows OpenRouter as unset, put `OPENROUTER_API_KEY` in the machine-local `~/.synth-desktop/.env` (mode `0600`) and relaunch. Never print the key, never paste it into a receipt, never commit `.env`.

Stop / inspect:

```bash
./scripts/desktop-instance.sh status gemini
./scripts/desktop-instance.sh stop gemini
```

Data root: `~/.synth-desktop/instances/v02/gemini/`.

---

## 4. Manual pass (this is the proof)

Do this in the `gemini` instance, not the installed `/Applications` app.

1. Composer model picker → Remote → **Gemini 3.7 Flash**. Confirm the Google mark. Confirm reasoning knobs are Low / Medium / High / XHigh / Max, default Medium. Confirm there is **no** Speed / service-tier knob.
2. Settings → authorized providers: row `Gemini 3.7 Flash` shows OpenRouter · Google and the **$1.875** output card. Count is six authorized models, not five.
3. Send a short text turn (“Reply with the single word ok.”). Tokens must stream. The turn must stay on `google/gemini-3.7-flash` via OpenRouter. It must **not** fall through to local Laguna (`sessionView` default) and must **not** use Synth Cloud.
4. Usage for that turn must show a **non-null** cost. Expect `tariff_estimate` first, then a provider-reported settled charge if OpenRouter returns one. **Do not treat `$0` as a pass.** A real Flash turn is not free; if the UI shows `$0` or `none`, that is a bug — record the `cost_source` and stop.
5. Optional: attach a small image and confirm the turn is accepted (image input is declared).
6. Optional: change reasoning to Low and Max and send one turn each. Both should complete.

No-key banner (only if this machine truly has no OpenRouter key): composer shows **OpenRouter API key required** and send is disabled. Do not unset a working machine key to force this. Playwright covers the picker row either way.

---

## 5. Automated gates (cheap, run from this worktree)

```bash
cd apps/synth_desktop/src-tauri && cargo test --lib tariffs
```

That is 7/7. Do **not** run `cargo test tariffs` without `--lib`; unrelated integration-test crate-root noise will fail and is not this ticket.

UI specs already updated on this dirty tree:

- `apps/synth_desktop/tests/bombadil/composer-toolbar.spec.ts`
- `apps/synth_desktop/tests/playwright/synth-cloud-provider.spec.ts` (picker text `/Gemini 3\.7 Flash/`)
- `apps/synth_desktop/tests/playwright/runtime-regressions.spec.ts` (authorized model count 6, `$1.875`)

A live OpenRouter turn is **not** covered by those specs. The manual pass in §4 is the missing proof.

---

## 6. Out of scope

- Synth Cloud Gemini (there is none)
- ChatGPT / Codex Gemini (there is none)
- Gateway / intern / mailbox
- Merging onto `josh/aug12-optimizers-workshop-visuals` or any v0.2 GO ticket (SYN-3202 / SYN-3215 / SYN-3212)
- Committing or pushing unless Josh asks
- Inventing a `$0` cost for OpenRouter BYOK

---

## 7. Report back

Paste:

- instance name + `git -C <worktree> rev-parse --short=12 HEAD` and whether the tree was still dirty
- screenshot or exact composer row + reasoning knobs
- one turn id / whether tokens streamed
- billed vs estimated cost and `cost_source` (must not be `$0` / `none`)
- any fallthrough to Laguna or Cloud

If the picker is missing, you are on the wrong tree.

---

## 8. Live proof (2026-08-14)

- Throughput turn: `019ffe9b-88c7-7ee2-b8b7-7ca5a79687f4`
- Model/provider: `google/gemini-3.7-flash` / OpenRouter; no Laguna or Synth Cloud fallthrough
- Stream: 745 output tokens including 54 reasoning tokens; 2.426 seconds from first to last output
- Throughput: 307.09 provider output tokens/s; 284.83 visible tokens/s; 66.97 end-to-end output tokens/s
- Latency: 8.619 seconds TTFT; 11.124 seconds end to end
- Cost: OpenRouter reported `$0.004860`; Workshop estimated `$0.004860` with `tariff_estimate`
- Regression: the 9,229-input / 23-output short turn must estimate exactly `$0.003504`
