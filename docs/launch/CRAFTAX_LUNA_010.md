# CRAFTAX-LUNA-010 — blocking friends-release scenario

This is the flagship Gate F research flow. It must run against the **exact installed, signed candidate**, not a debug build and not a fixture-only driver.

## Prompt (verbatim)

**Workshop agent model:** Luna xhigh  

> Find the Craftax Rust GameBench containers, register the appropriate container, run 10 rollouts with Luna low, and create a visual from the trace data and rewards you get.

**Rollout model:** Luna low  
**Scale:** exactly 10 terminal rollouts, recorded seeds, registered real container  

The user must not pre-register the container or hand-build the visual. Normal review/approval prompts are allowed. Hidden operator repair is not.

## Pass criteria

See the launch brief. Summary:

- Discover + register the real Craftax Rust GameBench container
- Record task IDs, seeds/split, model, harness revision, concurrency, budget, container identity before launch
- Configure Luna low; start exactly ten real rollouts
- Ten distinct rollout IDs reach honest terminal states
- At least two seeds produce substantively different frames/trajectories
- Live progress counts are truthful; no cross-bound sessions/seeds
- Trace V5 correlates observation, action, reward, frame, and model/tool event on matching rollout/seed identity
- Visual from actual trace/reward data; selecting a mark opens the matching rollout
- Luna xhigh returns a numerically grounded summary; no unsupported success claims
- Usage/cost for orchestrator + rollouts attributable, no duplicates
- Cleanup removes only this run’s resources and records IDs

## Harness

Automated driver (credentials required; will not fake a pass):

```bash
cd evals/workshop
npm run craftax-luna-010 -- \
  --instance "$WORKSHOP_INSTANCE" \
  --craftax-url "$WORKSHOP_GATE_CRAFTAX_URL" \
  --orchestrator-model openai/gpt-5.6-luna \
  --rollout-model openai/gpt-5.6-luna \
  --orchestrator-effort xhigh \
  --rollout-effort low
```

Also invocable from the launch gate:

```bash
npm run gate:release -- --workshop /path/to/workshop-v0.1 \
  --instance "$WORKSHOP_INSTANCE" \
  --craftax-url "$WORKSHOP_GATE_CRAFTAX_URL" \
  --craftax-luna-010
```

Without OpenRouter/Synth credentials and a live Craftax service the harness must **fail closed** (or skip as external), never emit `status: pass`.

## Required evidence pack

- screen recording from prompt through final return
- container registration/health receipt
- run manifest + ten rollout receipts
- trace IDs + correlation evidence (`synth.trace-correlation.v1`)
- visual screenshot/export
- final agent answer
- usage/cost reconciliation
- restart/reopen proof
- exact artifact SHA + source/service revisions

Negative variants: unavailable container, unhealthy container, auth/payment refusal, one rollout failure, telemetry disconnect/resume, app restart. UI and final answer stay truthful.
