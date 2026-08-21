# Optimizers-beta deploy mechanism (write-only)

Phase A ends at hosted ladder rung 0. This records **how** `optimizers-beta` would be deployed. Nothing here was executed (D2).

## What exists today

- Repo: `synth-laboratories/optimizers-beta`. Frozen SHA for v0.7: `aaa262e` on `origin/main`.
- Image: `Dockerfile` (Linux rustc build → `optimizers-beta serve`). `Dockerfile.local` is the slot/dev variant.
- `railway.toml` (on some checkouts, not always on `origin/main`): Railpack build, start `./bin/optimizers-beta serve`, health `/healthz`.
- **No** GitHub Actions workflow and **no** `deploy.yml` equivalent. A git push does **not** deploy this service.

Backend (for contrast) **does** deploy on push: `.github/workflows/deploy.yml` maps git `staging` → Railway environment `dev` (product name “staging”) and git `main` → production. Backend `/version` on prod at freeze was still v0.6 (`128588f`).

## How a v0.7 beta deploy would actually run

1. Confirm D2 in writing.
2. Image: `docker build -f Dockerfile -t optimizers-beta:$SHA .` from `aaa262e` (or the freeze SHA). Do not `docker cp` slot patches; bake them.
3. Railway service `optimizers-beta-prod` (and the staging twin if one exists): set image/SHA, `OPTIMIZERS_BETA_BIND`, `TINKER_API_KEY` (service-held, never in Workshop), workspace root.
4. Probe (after deploy, not before):
   - `GET /healthz`
   - `GET /v1/training/capabilities` — today prod 404s (pre-CISPO binary)
   - `GET /v1/runtime-identity`
5. Backend `OPTIMIZERS_BETA_BASE_URL` + `OPTIMIZERS_BETA_SERVICE_TOKEN` on the matching Railway env must point at that SHA.

## Slot (rung 1) vs cloud

Slot compose (`synth-dev/local_dev/infra/docker-compose.local-stack.yaml`) builds beta from `OPTIMIZERS_BETA_REPO_ROOT`. Rebuild slot3 from frozen SHAs, baked images, no `docker cp`. Admission on current backend `v0.7` (L3 #1247) is `admission=validation_only` plus header `X-Synth-Training-Validation-Grant` (token `SYNTH_HOSTED_TRAINING_VALIDATION_GRANT_TOKEN`). Deprecated env flags still admit if both are set — do not rely on them. **Stop before any Tinker call** (D4).

## Commands that must not run in this handoff

```bash
# D2 — would deploy
railway up --service optimizers-beta-prod
git push origin main   # backend: this IS the prod deploy

# D4 — would spend
uv run python scripts/verify_cispo_parity.py
```
