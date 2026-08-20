# Workshop launch docs

**v0.7:** [v0.7-scope.md](./v0.7-scope.md) · runtime plan [v0.7-optimizers-runtime.md](./v0.7-optimizers-runtime.md) · freeze map [v0.7-release/COMMIT_MAP.md](./v0.7-release/COMMIT_MAP.md).

v0.1 friends-release contract remains in this folder. **v0.2 launch status and E2E plan:** [v0.2-launch.md](./v0.2-launch.md). **v0.3 launch notes and integration status:** [v0.3-launch.md](./v0.3-launch.md). **v0.3 themes:** [v0.3-themes.md](./v0.3-themes.md). **v0.2 second-pass review + address plan (not started):** [v0.2-second-pass-2026-08-13.md](./v0.2-second-pass-2026-08-13.md). **v0.2 finish handoff (receipts / `v02golden`; dirty snapshot stale):** [V0.2_FINISH_HANDOFF_2026-08-13.md](./V0.2_FINISH_HANDOFF_2026-08-13.md). **Harbor GameBench code-policy DEO + Codex Luna med (visual-first):** [HANDOFF_HARBOR_GAMEBENCH_DEO_LUNA.md](./HANDOFF_HARBOR_GAMEBENCH_DEO_LUNA.md).

## v0.1 (frozen)

Frozen contract and remaining Gate F / Gate P work.

| Doc | Purpose |
|---|---|
| [V01_SCOPE_AND_OWNERS.md](./V01_SCOPE_AND_OWNERS.md) | Product contract, owner matrix, candidate SHAs |
| [LAUNCH_OPS.md](./LAUNCH_OPS.md) | Monitoring, flags, rollback, no-go, post-publish smoke |
| [GATE_SEQUENCE.md](./GATE_SEQUENCE.md) | Deterministic / integration / fault-injection sequence |
| [CLEAN_USER_REHEARSAL.md](./CLEAN_USER_REHEARSAL.md) | Download / signup / sign-in / checkout rehearsal |
| [CRAFTAX_LUNA_010.md](./CRAFTAX_LUNA_010.md) | Blocking Luna xhigh → 10 Luna-low Craftax scenario |
| [AUTH_WEB_HANDOFF.md](./AUTH_WEB_HANDOFF.md) | Clerk, device-init, download, upgrade deep link |
| [UPDATES_AND_CHANNELS.md](./UPDATES_AND_CHANNELS.md) | Passive v0.1 check, stable/nightly isolation, updater plan, rollback |
| [LAUNCH_READINESS_STATUS.md](./LAUNCH_READINESS_STATUS.md) | Live status vs Gate F / Gate P blockers |

Helper: `scripts/run_launch_gates.sh` runs the deterministic subset.
