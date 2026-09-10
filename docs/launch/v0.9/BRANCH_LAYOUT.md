# Workshop v0.9 branch layout

Workshop v0.9 development is integrated on `codex/v0.9.0`.

## Active topology

```text
f35aab12  shared pre-release ancestor
├── origin/main ── v0.8.0 ── public release reconciliation
└── codex/finish-inline-eval-refactor @ 10f9792e
    └── codex/v0.9.0  ← current v0.9 integration and build line
```

The v0.8 public branch was reconciled as a release/source-publication history.
It is intentionally not merged mechanically into v0.9: doing so presents the
same product source as widespread delete/add conflicts. The v0.9 integration
line carries the complete tested product tree and shares the pre-release
ancestor with the published v0.8 history.

## Branch roles

| Ref | Role | Mutation policy |
| --- | --- | --- |
| `v0.8.0` | Immutable shipped release | Never move or rewrite |
| `origin/main` | Published release history | Reconcile completed releases only |
| `codex/finish-inline-eval-refactor` | Completed Craftax and visual-shell source lane | Retain as a fixed handoff point |
| `codex/v0.9.0` | Canonical v0.9 integration branch | All v0.9 work lands here through reviewed feature branches |
| `codex/v0.9.0-<topic>` | Short-lived v0.9 feature or fix lane | Branch from and merge back into `codex/v0.9.0` |
| `release/v0.9.0` | Release-candidate branch | Cut from a green `codex/v0.9.0`; accept release blockers only |
| `v0.9.0` | Final annotated release tag | Create from the accepted release branch; never move |

## Integration rules

1. Start new work from `codex/v0.9.0`, not a v0.8 worktree or historical lane.
2. Keep feature branches narrow and name them `codex/v0.9.0-<topic>`.
3. Require the desktop version/instance gates and relevant product tests before
   merging a feature lane.
4. Cut `release/v0.9.0` only after the full v0.9 build and acceptance gates pass.
5. Do not merge the source-reconciled v0.8 public history into active product
   branches. Bring forward an individual release-only change explicitly when it
   is still relevant.

## Product identity

The active build line uses:

- app version `0.9.0`;
- release line `v0.9`;
- instance namespace `v09`;
- development bundle IDs under `com.synth.desktop.v09.dev.*`;
- candidate bundle ID `com.synth.desktop.v09.candidate`.

Historical v0.8 receipts, storage migrations, immutable evaluation digests, and
the pinned `synth-mlx-rl-v08-*` compatibility directories retain their original
names because changing them would alter provenance rather than product identity.
