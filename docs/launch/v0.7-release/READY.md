# v0.7 readiness

Status: **not ready — release candidate held** (D6: the RC waits for the artifact → inference → Eval flow).

Last revised 2026-08-20 at workshop `origin/v0.7` `701b483e`.

## What is true now

- Merged on `v0.7`: GEPA workbench (#41), training sidecar with instance-scoped model roots (#42), v0.7 instance line + 0.2.15 sidecar pin + training taxonomy (#47), hosted CISPO bound to retained SFT state (#50). optimizers 0.2.15 is on PyPI (`d3c9edd`); experiment layer merged separately (#44). backend `v0.7` contains staging (#1244); synth-mlx-rl `v0.7` has the real-only backend (#2); evals `dev` has the CUA lanes (#280).
- Open in the stack (in merge order): #45 managed artifacts → #46 artifact inference → #48 training-event adapter → #49 Eval provisioning → #51 local MLX GSM8K + dropout refusal → #52 typed agent capabilities; #43 UI artifact-first workspace rebases on the stack.
- Not deployed, not spent, not notarized: see `KNOWN_ISSUES.md` K8, K10, K24 and decisions D2/D4/D8.

## Ready when

1. `ACCEPTANCE.md` per-lane table has a verdict in every row with receipt paths, and no row is **no-go**.
2. `TEST_REPORT.md` carries the integrator's rows for every changed repo at the frozen SHAs.
3. `PROVENANCE.md` has no `TBD` in the desktop-artifact section; `PACKAGE.md` records the ZIP SHA-256, bytes, CDHash.
4. `COMMIT_MAP.md` names the verified release head per repo and the promotion SHAs.
5. `RELEASE_NOTES.md` core table has its **proven** column filled for every lane that ships as core; anything still pending is moved to opt-in or deferred before tagging.
6. Josh has answered D2 (deploy), D4 (spend), D8 (notarization), and D3 (catalog wording) or the defaults in register §10 are recorded here as applied.

## Promotion

`v07/<lane>` → `v0.7` (merge commits, stack order, tree-identity checks) → `main` → annotated tag `v0.7.0` → GitHub release → frontend catalog PR → `POST_RELEASE.md`.
