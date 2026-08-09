# Workshop

> **Visibility note:** This repository is currently **private**. It is intended to become **public**.

Synth Desktop / Local Agent Workbench — a local-first agent research and development workbench where agents can run locally (Laguna XS 2.1) or in Synth Cloud (Intern sync/async), and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

## Status

Greenfield. Product and architecture context lives in [`HANDOFF.md`](./HANDOFF.md). That document is the seed for a first-pass implementation plan against the broader Synth codebase.

## Product framing

Synth Desktop is **not** primarily another coding IDE.

> Synth Desktop is a local-first agent research and development workbench where agents can run locally or in Synth Cloud, and where every run produces inspectable, replayable, quantitative, version-linked artifacts.

Core loop:

**observe → understand → modify → evaluate → fine-tune → deploy**

## V1 wedge (narrowed)

Two execution targets only:

1. **Local Laguna XS 2.1** (MLX / Metal)
2. **Synth Intern**, with **sync** and **async** as first-class modes of the same abstraction

```text
                    Synth Desktop
                         │
                  Synth Runtime API
                         │
             ┌───────────┴───────────┐
             │                       │
           LOCAL                   INTERN
             │                       │
      Laguna XS 2.1          Synth Intern agent
       MLX / Metal             │          │
             │                 │          │
          synchronous        sync       async
          session             run        job
```

## License / ownership

Owned by [synth-laboratories](https://github.com/synth-laboratories). Public release planned; treat contents as pre-release until then.
