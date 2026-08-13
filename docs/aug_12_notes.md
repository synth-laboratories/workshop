# DiG-bench harness study — Aug 12, 2026

## Executive read

The interesting result in the DiG-bench launch thread is not merely that models still fail simple text games. It is the harness claim: their agentic Prime-style harness did not improve over a basic non-coding harness. Workshop is already shaped to test exactly that claim with two `policy_ref`s on one pinned game and one durable event vocabulary.

The current code is not yet a behavioral harness comparison:

- `containers@a20f994` advertises `react_legal_actions/react_legal_actions` and `codex/agentic_codex`.
- Both paths choose the first legal action in `DigbenchRuntime._live` and `DigbenchRuntime._mock`.
- The Codex path only adds synthetic `span.mcp.opened` / `span.mcp.closed` envelopes around that same action.
- The configured Codex model, `gpt-5.6-codex`, is not in the bundled catalog of local `codex-cli 0.145.0`; current bundled choices include `gpt-5.6-luna`, `gpt-5.6-sol`, and `gpt-5.6-terra`.
- No `DIGBENCH_API_TOKEN`, `OPENROUTER_API_KEY`, or `OPENAI_API_KEY` is present in the current shell. Codex itself is authenticated through ChatGPT.
- The existing headless receipt is correctly labeled structural: 18/18 tests pass against the mock and local relay, but it is not A8 and not a model/harness evaluation.

Therefore the next milestone is not “run more C8.” It is to replace the synthetic policy distinction with two real execution boundaries, emit comparable evidence, then render the paired trajectories in `live.digbench.v1`.

## Source result to reproduce

James Whittington's Aug 12 launch thread describes 70 self-contained text discovery games, 21 public, with hidden rules and win conditions discovered through experimentation. The thread reports:

1. Frontier models improved sharply, but still struggle on tiers 6–7.
2. Agentic harnesses such as Prime did not beat a basic non-coding harness.
3. The public games and API are intended for direct model evaluation.

Our study should test item 2 without changing model, game, budget, or scoring authority between arms.

Sources: [DiG-bench launch thread](https://x.com/jcrwhittington/status/2087535497480388729), [Codex non-interactive command reference](https://learn.chatgpt.com/docs/developer-commands?surface=cli), and [Codex MCP configuration](https://learn.chatgpt.com/docs/extend/mcp?surface=cli).

## The causal question

For a fixed model and game, does a Codex-style agentic harness improve discovery relative to a bounded next-action/ReAct harness?

The treatment is the harness, not the model:

```text
same model snapshot / effort / game / step budget / prompt facts
                              |
              +---------------+---------------+
              |                               |
      basic next-action                  agentic Codex
      rolling transcript                 persistent task
      REST step relay                    digbench-mcp tools
      no filesystem                      scratch files allowed
      structured action only             tool-mediated actions
              |                               |
              +---------------+---------------+
                              |
                env terminal status + trace
```

Do not call a Luna ReAct run versus a different Codex model a harness comparison. That is a model+harness bundle comparison and should be reported separately.

## Harness contracts

### Basic / next-action arm

The basic arm should be deliberately capable but narrow:

- One persistent rolling transcript per game session.
- Input: current observation, exact legal actions, level, lives, steps remaining, and bounded prior action/observation history.
- Output schema: one exact legal action plus an optional short public hypothesis summary. No hidden reasoning is required or logged.
- The relay, not the model, calls `start_session`, `step`, and `get_session`.
- No MCP tools and no filesystem.
- Parse failures retry at most once, then fail closed or use an explicitly labeled harness fallback. Fallback actions never count as model-authored.
- Context compaction is mechanical and recorded as a harness event.

### Agentic Codex arm

The agentic arm should be a real Codex process boundary:

- Run Codex non-interactively or through its app-server/SDK, with a pinned model and effort.
- Give it only the DiG-bench MCP server plus a run-local scratch directory. Disable unrelated MCP servers and web search.
- The agent owns `start_session`, `step`, and `get_session` through MCP. The outer runner owns the fixed game id, budget, timeout, artifact capture, and termination.
- Persist Codex JSONL events and normalize tool-call open/data/close into the same rollout stream.
- Record Codex CLI version, model slug, config digest, prompt digest, MCP server version, and sandbox/approval posture.
- Treat MCP startup failure, missing tool discovery, model refusal, timeout, and budget exhaustion as distinct terminal reasons.
- Never infer model reasoning from prose. Observable hypotheses may be logged only when the agent explicitly publishes them as task artifacts or structured messages.

Official Codex supports scripted `codex exec --json` runs and local stdio MCP servers. That is the minimum honest boundary; adding `span.mcp.*` around an outer REST call is not.

## Phase 0: make the current floor tell the truth

Before paid/live runs, add tests that prevent structural smoke from being misreported as harness performance:

- `digbench_mock` receipt contains `execution_class: structural_stub`.
- Each `trace.opened` includes immutable `world_ref`, `environment_ref`, `task_instance_id`, and secret-free `policy_ref`.
- Synthetic first-legal actions carry `action_authority: harness_stub`.
- Synthetic MCP spans carry `evidence_class: simulated` and cannot satisfy the live agentic gate.
- `digbench_public --paid` refuses `codex/agentic_codex` unless an actual Codex executor is configured and observed.
- The configured model slug must resolve against the Codex catalog before the first session starts.
- The A8 receipt requires `execution_class: live_model`, real model-call evidence in both arms, and real MCP tool-call evidence in the Codex arm.

This should intentionally break any test that equates “MCP-shaped envelopes exist” with “Codex ran.”

## Test ladder

### L0 — pure contract tests

Fast, deterministic, no model, no public API:

- Policy-pin validation: basic must not bind MCP; agentic must bind `digbench-mcp`.
- Redaction of bearer tokens, token env-var names, nested raw payloads, exception bodies, trace seals, and Workshop bindings.
- Status mapping: `completed -> 1`, `game_over -> 0`, running/failed/credential-missing -> `null`.
- Cursor invariants, duplicate events, gaps, out-of-order replay, and reconnect without checkpoint kinds.
- Mid-session policy rebinding refused.
- Game id frozen before mutation; unavailable pinned game fails closed rather than swapping.
- Same game may have distinct rollout/session ids across arms.
- Stub, live-basic, and live-agentic evidence classes cannot be confused.

### L1 — adversarial local Agent API

Extend the existing `DigState` server into scripted cases:

- Empty legal actions.
- Legal action rejected; illegal action accepted.
- Repeated observation with decreasing budget.
- Level/lives fields missing, null, renamed, or wrong type.
- Status only on `GET /sessions/{id}` versus on step response.
- Duplicate step index, skipped index, stale retry, and idempotent replay.
- HTTP 401/403/409/429/500, non-JSON body, truncated JSON, slow body, connection drop.
- Token echoed in error text and deeply nested payloads.
- Session expires after terminal; local sealed replay must still render.

Run every case against both adapters. Most failures should be identical because environment semantics are shared; only the agentic arm should emit real MCP transport evidence.

### L2 — toy discovery games

Use deterministic hidden-rule games that require more than selecting the first legal action:

- `toggle`: infer that alternating A/B unlocks completion.
- `counterfactual`: one probe changes a latent rule; repeating the prior action fails.
- `delayed_key`: inspect before take, then use; premature use costs a life.
- `alias`: observations rename the same latent state to test memory.
- `reset_trap`: reconnect preserves the session; restarting loses progress.
- `branch_budget`: locally attractive actions exhaust steps before the winning branch.

Each game should have an oracle, a known minimal action count, deterministic seeds, and mutation tests proving the oracle catches degenerate first-legal behavior.

### L3 — real harness smoke without DiG-bench

Run the two real execution boundaries against the toy games:

- Basic: model call per step with structured action output.
- Codex: one persistent process with the toy MCP server.
- Same model slug and effort in both arms.
- Capture provider/model usage when available; otherwise keep usage fields `null`, never zero.
- Assert that Codex actually discovers and invokes MCP tools.
- Assert that the basic arm never invokes MCP.
- Re-run one exact case to measure nondeterminism and trace stability.

### L4 — public DiG-bench canary

Blocked until `DIGBENCH_API_TOKEN` is supplied to the runner through a trusted environment source.

- One pinned public game.
- One basic and one agentic rollout.
- Visual opened and stream subscribed before either `start_session`.
- Small, equal budgets and the same model.
- Confirm terminal or honest incomplete outcome, token redaction, seal, and offline reopen.
- This is a canary, not a leaderboard claim.

### L5 — paired public panel

Recommended first real panel:

```text
games:         all 21 public games, if API terms permit
models:        Luna and Sol as separate paired studies
harnesses:     basic next-action, agentic Codex
replicates:    3 per model × harness × game to start
pairing:       shared game and budget; independent session ids
primary:       win within budget
secondary:     highest level, steps to win, lives lost, invalid actions,
               unique observation fingerprints, action entropy, tokens,
               wall time, cost, terminal reason
```

That is 252 runs. Start with 3 games × 2 models × 2 harnesses × 2 replicates = 24 runs to validate cost and reliability before expanding.

Do not scrape private tiers or infer difficulty metadata the API does not expose.

## Analysis

Primary analysis is paired by game and model:

- Report wins/attempts and Wilson intervals for each arm.
- Report paired win delta with a game-clustered bootstrap interval.
- Use McNemar's test for paired binary outcomes only after the panel is large enough.
- Fit a hierarchical logistic model only as a secondary analysis: harness fixed effect, game random intercept, model and harness×model terms.
- For incomplete runs, plot survival curves over steps-to-terminal rather than coercing them to losses unless the benchmark contract says budget exhaustion is `game_over`.
- Report cost-normalized success and step-normalized exploration separately from raw win rate.
- Publish all terminal reasons. A harness that crashes less may be useful even if conditional win rate is unchanged.

The key diagnostic is not just state count. Useful discovery requires state diversity followed by exploitation. Track:

- observation fingerprints visited;
- novel transition rate;
- repeated failed transitions;
- action distribution and entropy;
- recovery after invalid action or lost life;
- progress per model call and per 1k tokens;
- MCP overhead: tool calls, tool errors, startup latency, redundant reads;
- context pressure: compactions, transcript size, and late-game forgetting;
- explicit public hypothesis revisions, when emitted.

## Workshop visualization

`live.digbench.v1` is the right live surface, but the harness study needs a stronger paired view.

### Live lane view

Keep the existing observation/actions/stats/history layout and add:

- Human labels: `Basic · Luna · medium` and `Codex · Luna · medium`, not raw rollout ids alone.
- Pinned game id and immutable policy ref visible in each lane.
- Evidence badge: `stub`, `live basic`, or `live Codex+MCP`.
- Terminal reward fetched from the authoritative `/reward` result; status-derived expectation can be shown separately but must not impersonate the scored node.
- Counts for model calls, MCP calls, invalid actions, unique observations, and remaining budget.
- Agentic tool timeline folded into history without a nested second log.
- A visible warning when usage, cost, or reward is missing.

### Paired comparison view

Add `eval.digbench_harness_compare.v1` after the live contract is stable:

- Paired win strip by game.
- Steps/progress small multiples for both arms.
- Exploration-versus-exploitation trajectory: cumulative unique observation fingerprints over step.
- Action entropy and repeated-failure indicators.
- Token/cost/wall-time comparison.
- Terminal-reason matrix.
- Click a cell to reopen the sealed `live.digbench.v1` lane at the relevant sequence.

The aggregate consumes receipt rows and resource refs to sealed rollouts. It must not copy raw bearer headers, fabricate frames, or flatten missing rewards to zero.

### Required event metadata

At minimum, every lane needs:

```json
{
  "rollout_id": "...",
  "world_ref": "world:digbench:P-1",
  "task_instance_id": "P-1",
  "policy_ref": {
    "harness": "codex",
    "config": "agentic_luna_medium"
  },
  "execution": {
    "class": "live_model",
    "model": "gpt-5.6-luna",
    "effort": "medium",
    "executor": "codex-cli",
    "executor_version": "0.145.0",
    "mcp_bind": "digbench-mcp"
  }
}
```

The basic arm uses the same shape with `executor: next-action` and no MCP bind.

## Acceptance gates

### Structural C8 remains green

- Existing 18 headless tests pass.
- No frames, Harbor nouns, optimizer nouns, token leakage, guessed stream URLs, or checkpoint claims.

### Real harness gate

- Both arms contain observed model-call evidence.
- Agentic contains observed Codex process metadata and MCP tool calls.
- Basic contains no MCP calls.
- Same resolved model and effort across paired arms.
- No fallback action is labeled as policy-authored.
- Run termination and usage are honest.

### Workshop A8 gate

- `live.digbench.v1` is ready before the first public `start_session`.
- Both live lanes render and can be flipped without stalling the other.
- Text observation, legal actions, stats, action/history, and status are current.
- Authoritative terminal reward renders; incomplete remains missing.
- Trace seal reopens after public sessions are gone.
- Token is absent from event log, trace, receipt, visual bindings, app logs, and screenshots.

### Study gate

- Predeclared matrix, models, efforts, budgets, games, and exclusions.
- Paired analysis with raw counts and uncertainty.
- Stub/canary/panel results never share the same result label.
- All artifacts include source revisions and harness/config digests.

## Immediate implementation order

1. Add honest execution-class metadata and policy refs to Containers trace open.
2. Add adversarial local-relay and toy-game tests that first-legal behavior cannot pass.
3. Build a basic next-action executor with structured output and bounded history.
4. Build a Codex executor using `codex exec --json` plus a run-local stdio DiG-bench MCP config.
5. Normalize both into the same seven DiG-bench kinds plus policy/model/tool spans.
6. Add two-lane fixtures and visual tests in Workshop.
7. Add human lane labels and diagnostic metrics to `live.digbench.v1`.
8. Run the 24-run pre-panel on toy/local games.
9. Supply the public token through a trusted environment source and run the 2-run A8 canary.
10. Expand only after cost, rate limits, and trace integrity are known.

## Evidence captured today

### Real public DiG P-1 canary (supersedes the local synthetic score as benchmark evidence)

On Aug 12, 2026, a DiG account token was minted through the public account UI and verified against the API's 21-game catalog (`P-1` through `P-21`). The token is stored outside Workshop and is absent from fixtures and receipts.

Two terminal `P-1` sessions were run with ChatGPT-authenticated `codex exec`, medium effort, and a narrow action tool over the official DiG session API:

| Model | Seed | Result | Levels beaten | Applied moves | Wall | Locally rejected illegal attempts | Malformed local commands | Authority gate |
|---|---:|---|---:|---:|---:|---:|---:|---|
| `gpt-5.6-luna` | 391695617305236574 | `game_over` | 8/14 | 672 | 921.675s | 90 | 88 | FAIL |
| `gpt-5.6-terra` | 7934651718125062974 | `game_over` | 1/14 | 269 | 315.482s | 1 | 0 | pass |

The raw Luna-minus-Terra delta is +7 levels, but it is not a ranking estimate: this is n=1 per model, DiG assigned different seeds, and Luna failed command-authority compliance. Its malformed commands failed locally and did not reach DiG, so the server score remains a real diagnostic observation while the run is excluded as a clean benchmark point.

Workshop's authenticated-results fixture now shows these two official terminal lanes and renders score separately from command compliance. The previous six-run local hidden-rule canary remains useful harness smoke evidence but must not be presented as public DiG performance.

- X launch thread inspected on Aug 12, 2026.
- `containers` branch `josh/aug12-containers-platform` at `a20f994` is clean.
- Existing DiG-bench suite rerun: `18 passed in 4.26s`.
- Workshop visuals suite rerun: `65 passed`.
- `workshop` branch has unrelated in-progress changes; this note should not be used as proof that A8 passed.
- `DIGBENCH_API_TOKEN`, `OPENROUTER_API_KEY`, and `OPENAI_API_KEY` absent in the current shell.
- `codex-cli 0.145.0` present and authenticated through ChatGPT.
