# v0.7 post-release verification (skeleton)

Verified: TBD.

## Public release

- GitHub release: `https://github.com/synth-laboratories/workshop/releases/tag/v0.7.0` — TBD
- Download page: `https://www.usesynth.ai/download` — advertises 0.7.0: TBD
- Stable manifest: `https://www.usesynth.ai/releases/stable/latest.json` — version: TBD
- Public artifact: `https://www.usesynth.ai/releases/v0.7.0/<asset>` — HTTP status, content-type, bytes: TBD
- Public artifact SHA-256: TBD (equals `PROVENANCE.md` and GitHub's asset digest)
- Public `PROVENANCE.json` SHA-256: TBD

## Installed-artifact resilience (exact round-trip bytes at `/Applications/Synth Workshop.app`)

- Provider smokes (ChatGPT subscription, OpenRouter, managed local Laguna): TBD
- Forced app-process kill → relaunch → durable optimizer visual reopens with candidate/frontier/terminal state intact: TBD
- GEPA sidecar killed outside the app → next request relaunches it: TBD
- Local MLX run cancelled → service restart → durable reopen: TBD
- Hosted run reconciled after restart (if rung 3 shipped): TBD

## Hosted lane (only if D2 was given)

- backend `/version` on prod: TBD · optimizers-beta `/v1/training/capabilities` and `/v1/runtime-identity`: TBD
- Catalog disposition for hosted CISPO as shipped (D3): TBD

## Branch continuation

- v0.8 integration branches created from the reconciled released revisions (workshop, frontend, backend, optimizers, containers, synth-mlx-rl): TBD
- The `v0.7.0` tag stays bound to the released bytes; post-release docs never retag or alter the artifact.
