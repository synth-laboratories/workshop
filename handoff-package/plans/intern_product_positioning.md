# Research Intern — Product Positioning

**Date:** 2026-07-31  
**Analogy:** Anthropic [Claude Tag](https://www.anthropic.com/news/introducing-claude-tag) — a shared teammate you tag into work, not a one-off chat.

## One-liner

**The Intern is your org’s Synth teammate** — one Intern per organization, always yours, helping maintain and drive your Synth infra (Factories, Efforts, Runs/Swarms, optimizers, Experiments, Visuals, data).

## Claude Tag → Intern

| Claude Tag | Synth Intern |
|---|---|
| One @Claude in a shared workspace/channel | One Intern per org (shared teammate, not per-user silo) |
| Tag @Claude with a request; leave and do other work | MCP message into the Async Intern mailbox |
| Multiplayer — anyone can continue the same Claude | Org members share the same Intern context/projection |
| Builds channel/tool context over time | Durable Intern memory + infra state (Factories, Runs, evidence) |
| Ambient / takes initiative when enabled | Always-on Async instance: wakes, follows up, maintains cadence |
| Works asynchronously over hours/days | Nonstop `InternAsyncWorkflow` — progress without a connected client |
| Direct/live collaboration when you’re present | Sync sessions (many) — live workbench attach |
| Scoped tools + spend limits | Capability grants + budgets on the Intern |

## What “YOUR Intern” means

- **Org-scoped identity:** not a disposable session agent; the same Intern that already knows your infra.
- **Job:** help maintain Synth research infrastructure — keep Factories/Efforts healthy, drive Runs, surface blockers, produce attributable progress — not “chat about research in the abstract.”
- **Async is the teammate who never leaves:** one nonstop instance; MCP = how you tag them.
- **Sync is sitting with them live:** many bounded sessions when you want the cockpit/workbench.

## Interaction (product feel)

```text
You (via MCP / later FE)     →  “hey, keep chasing X / fix Y on our Factory”
        Intern (Async)       →  works in the background on your Synth stack
You return / intern_events   →  progress, checkpoints, asks, outcomes
```

Same spirit as tagging @Claude in Slack: **delegate, leave, come back to a shared teammate’s thread** — except the workplace is Synth Cloud infra, and the Async surface is a durable message queue.

## Not this

- A per-chat throwaway agent with no org memory  
- Many independent Async Interns competing as the primary model  
- Magi theater labels without a real always-on engine  
- Replacing Factory/Run authority — Intern **coordinates and actuates** under grants  

## Runtime cardinality (unchanged)

```text
Org → one Intern identity
        ├── Async: exactly one nonstop long-running instance (MCP mailbox)
        └── Sync: many live sessions (FE workbench)
```
