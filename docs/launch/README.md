# Workshop launch docs

v0.1 friends-release contract remains in this folder. **v0.2 launch status and E2E plan:** [v0.2-launch.md](./v0.2-launch.md).

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
