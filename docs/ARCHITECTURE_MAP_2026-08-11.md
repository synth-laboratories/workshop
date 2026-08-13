# Workshop Desktop — current architecture map

**Date:** 2026-08-11  
**Repo:** `synth-laboratories/workshop`  
**Product:** Synth Desktop / Workshop (`apps/synth_desktop`)  
**Shell:** Tauri 2 + Rust (`CoreRuntime`) — **not** Electron  
**Companion law doc:** [`architecture.md`](../architecture.md) (session routing: Intern **or** Codex only)

This note is a **2D map of what exists today** for v0.2 refactor planning. Prefer code + `architecture.md` if this drifts.

---

## 1. Repo layout (what is product vs leftover)

```text
workshop/
├── apps/
│   ├── synth_desktop/          ★ PRODUCT (Vite React + src-tauri Rust)
│   ├── mock/                   Electron fixture only — not product
│   └── _ref_first_pass/        Electron scaffold reference — not product
├── packages/
│   ├── runtime-protocol/       Shared TS DTOs (Session, ExecutionTarget, …)
│   └── runtime-client/         Legacy /__runtime HTTP client (browser/test)
├── visuals/                    @synth/visuals — templates + registry (bundled)
├── services/
│   ├── laguna-daemon/          ★ Laguna Responses/MLX sidecar (bundled)
│   ├── local-runtime/          Legacy Python — Desktop must NOT start
│   └── local-inference/        Older :7332 path — not primary Laguna
├── scripts/                    desktop.sh · desktop-instance.sh · gates
├── contracts/research-v1.json  Backend/research enums (experiments live HERE, not UI)
├── docs/launch/                Friends / public ops
└── architecture.md             Authoritative topology + routing law
```

**Cargo:** one crate — `apps/synth_desktop/src-tauri` (`synth-desktop`)  
plus bins: `synth-visuals-mcp`, `synth-containers-mcp`, `synth-optimizers-mcp`, `synth_trace_import`.

---

## 2. Process topology (one macOS window)

```text
┌───────────────────────────── macOS window ─────────────────────────────┐
│  Tauri 2 host  (synth-desktop Mach-O)                                  │
│                                                                        │
│   ┌─ WebView ───────────────────────────────────────────────────────┐  │
│   │  Vite/React renderer                                            │  │
│   │  App · Composer · Inventory · Visuals · Optimizers · Account    │  │
│   │  desktopBridge.ts  ──invoke / events──►                        │  │
│   └─────────────────────────────────────────────────────────────────┘  │
│                              │                                         │
│   ┌──────────────────────────▼──────────────────────────────────────┐  │
│   │  CoreRuntime                                                    │  │
│   │  SQLite · CAS · journal · inventory · visuals · optimizers      │  │
│   │  account / device_auth / synth_config                           │  │
│   │                                                                 │  │
│   │  ┌──────────────┐  ┌────────────────┐  ┌─────────────────────┐  │  │
│   │  │ CodexManager │  │ CredentialBroker│  │ LagunaManager      │  │  │
│   │  │  (stdio NDJSON)│ │  (loopback HTTP)│  │  (daemon lifecycle)│  │  │
│   │  └──────┬───────┘  └────────┬───────┘  └──────────┬──────────┘  │  │
│   │         │                   │                     │             │  │
│   │  ┌──────┴───────┐  ┌────────┴───────┐  ┌──────────┴──────────┐  │  │
│   │  │ TerminalMgr  │  │ Visuals IPC    │  │ EvalDriver (dev)    │  │  │
│   │  │ (user PTYs)  │  │ (loopback HTTP)│  │                     │  │  │
│   │  └──────────────┘  └────────────────┘  └─────────────────────┘  │  │
│   └─────────────────────────────────────────────────────────────────┘  │
└──────────────────────────────────┬─────────────────────────────────────┘
                                   │
          ┌────────────────────────┼────────────────────────┐
          ▼                        ▼                        ▼
   codex app-server         Laguna daemon            Synth backend /
   (child, stdio)           :LAGUNA_PORT             Responses gateway
                            /v1/responses            (profile-selected)
```

---

## 3. Named-instance layout (`desktop:dev`)

`npm run desktop:dev` → `scripts/desktop-instance.sh`

```text
~/.synth-desktop/
├── shared-cargo-target/v02/          ★ default shared Cargo target
│                                      (SYNTH_DESKTOP_SHARED_CARGO_TARGET=1)
└── instances/v02/<name>/
    ├── data/                         SYNTH_DESKTOP_DATA_ROOT
    │   ├── *.sqlite / CAS / journal
    │   ├── .env (0600)               SYNTH_API_KEY after device pair
    │   ├── visuals-ipc.json
    │   └── eval-driver.json          (debug/named only)
    ├── workspace/
    ├── instance.json                 manifest + provenance digests
    └── build/target/                 only if shared Cargo disabled

Ports (stable per name via cksum):
  Vite   = 14200 + cksum(name) % 1000
  Laguna = 17300 + cksum(name) % 600

Identity:
  SYNTH_DESKTOP_INSTANCE=<name>
  bundle id com.synth.desktop.v02.dev.<name>
```

Canonical install (CUA / friends path): `/Applications/Synth Desktop.app` via `scripts/desktop.sh` (clean git tree required for artifact commands).

---

## 4. Session routing law (non-negotiable)

```text
                    ┌──────────────┐
   Renderer turn ──►│ Tauri host   │
                    └──────┬───────┘
                           │
              exactly one of
                    ┌──────┴──────┐
                    ▼             ▼
            ┌─────────────┐  ┌─────────────┐
            │ Codex       │  │ Intern      │
            │ app-server  │  │ sync/async  │
            └──────┬──────┘  └──────┬──────┘
                   │                │
                   ▼                ▼
            Responses API      Backend Intern APIs
            (Laguna / OR /     (UI CloudDesk dormant
             Synth gateway)     in v0.1 friends chrome)


FORBID: Desktop ──► /v1/chat/completions | MLX direct | model SDK
```

Composer targets today (`EXECUTION_TARGETS`):  
`local-laguna` · `openrouter-luna` · `openrouter-laguna-s` · `openrouter-muse-spark` · `synth-cloud-laguna-s` · `intern-sync` · `intern-async` (Intern chrome dormant).

---

## 5. Auth → account → metered turn

```text
  Browser (Clerk @ usesynth.ai)
            │  approve device
            ▼
  Workshop web   /api/auth/device/{init,complete,token}
            │
            ▼
  device_auth.rs ──write──► data/.env (0600)  SYNTH_API_KEY
            │                 (renderer never sees key/code)
            ▼
  CoreRuntime reload (fail-closed if misconfigured)
            │
     ┌──────┴──────────────────┐
     ▼                         ▼
  account_cloud             Codex / Intern session
  GET .../account-snapshot
  plan · allowance
            │
            │  Synth Cloud model turn
            ▼
  CredentialBroker (loopback)
      lease token ──► real SYNTH_API_KEY
            │
            ▼
  source-owned Responses GATEWAY
  (NOT main backend /v1/responses)
            │
            ▼
  backend settlement  (workshop_spend lives on backend —
                       no Desktop module by that name)
```

Local Laguna / OpenRouter do **not** require account. Cloud Intern + Synth Cloud models do.

---

## 6. Inference / provider paths

```text
                         Codex app-server
                                │
              ┌─────────────────┼─────────────────┐
              ▼                 ▼                 ▼
       Local Laguna      OpenRouter          Synth Cloud
       :LAGUNA_PORT      (user OR key)       via CredentialBroker
       /v1/responses           │                    │
              │                │                    ▼
              ▼                ▼             Responses gateway
       laguna-daemon     Luna / Laguna S     (synth_config profile)
       (MLX worker)      Muse Spark                │
                                                   ▼
                                            Laguna S (cloud)
```

| Model path | Desktop status |
| --- | --- |
| Laguna XS 2.1 (local) | First-class — daemon + Codex |
| Laguna S / Luna / Muse Spark | OpenRouter targets + tariffs |
| Synth Cloud Laguna S | Gateway + broker |
| Nemotron | In `contracts/research-v1.json` only — **not** a composer target |
| Intern sync/async | Rust client live; **UI dormant** |

---

## 7. Product surfaces (Rust ↔ UI)

```text
┌────────────────────────────────────────────────────────────────────┐
│ Renderer pages                                                     │
│  Sessions/Chat │ Inventory │ Visuals │ Optimizers │ Account/Usage  │
└────────┬─────────────┬───────────┬──────────┬──────────────────────┘
         │             │           │          │
         ▼             ▼           ▼          ▼
┌────────────────────────────────────────────────────────────────────┐
│ Rust                                                               │
│  codex / intern │ inventory │ visuals/* │ optimizers/* │ account*  │
│                 │ traces    │ visuals_ipc│ recipes     │ device_*  │
│                 │ containers│            │             │ synth_cfg │
└────────────────────────────────────────────────────────────────────┘
         │             │
         │             ├──► loopback HTTP containers (Craftax etc.)
         │             └──► synth-trace CLI (Trace V5 ingest)
         │
         └──► MCP bins (stdio to Codex): visuals · containers · optimizers
```

**Not Desktop UI today:** Experiments, UML (backend/research contract only).  
**No in-repo SynthTunnel client** — containers are registered loopback HTTP URLs.

---

## 8. External edges (Desktop ↔ world)

```text
                 ┌─────────────────── Desktop ───────────────────┐
                 │                                               │
   FE device API │◄── pairing                                    │
   (Vercel/FE)   │                                               │
                 │──► main backend URL (profile)                 │
                 │      account-snapshot · billing · Intern      │
                 │                                               │
                 │──► Responses gateway (source-owned, profile)  │
                 │      Synth Cloud inference only               │
                 │                                               │
                 │──► OpenRouter (optional user key)             │
                 │                                               │
                 │──► Laguna daemon (local HTTP)                 │
                 │                                               │
                 │──► loopback containers (inventory)            │
                 └───────────────────────────────────────────────┘

Profiles (examples): local-slot1 · staging · prod
  → backend URL + gateway URL from synth_config (fail-closed if unknown)
```

---

## 9. Build & friends ship path

```text
  dirty tree OK ──────────► named desktop:dev instances
  clean tree REQUIRED ────► desktop:build / verify / install / friends ZIP

  desktop:check   →  tsc + cargo check (parallel)
  desktop:build   →  tauri build --bundles app
  desktop:verify  →  Rust tests + instance test + Playwright
  desktop:install →  /Applications + copy MCP bins + ad-hoc codesign

  Friends ZIP
  ───────────
  public path stays v0.1.0 (CFBundle may be 0.2.0)
  PROVENANCE.md binds ZIP SHA ↔ workshop SHA ↔ Mach-O digests
  FE: SYNTH_DESKTOP_STABLE_ARTIFACT_SHA256 + STABLE_VERSION=0.1.0
  Signing: ad-hoc, unnotarized (Open Anyway)
```

Bundled in `.app`: Laguna daemon tree + `visuals/`.  
MCP adapters: **copied** into `Contents/MacOS/` by install script (not true Tauri sidecars) — known launch debt.

---

## 10. Debt / seams (refactor targets)

```text
  KEEP AS PRODUCT                          CUT OR QUARANTINE
  ────────────────                         ─────────────────
  apps/synth_desktop                       apps/mock · _ref_first_pass · out/
  services/laguna-daemon                   services/local-runtime · local-inference
  visuals/ · MCP bins                      Electron-era handoff-package topology
  device_auth + account_cloud              AUTH_BILLING_FLOW.md "not implemented" banner
                                           (AUTH_FLOW.md + code are authority)

  DORMANT BUT LIVE UNDERNEATH
  ───────────────────────────
  Intern Rust + CloudDesk.tsx (unmounted) — v0.2 re-entry candidate

  DUPLICATED / FRAGILE
  ───────────────────
  usage_ledger (legacy) + usage_records
  MCP install copy vs real sidecars
  muse:serve → missing scripts/muse/
  README instance path missing /v02/
```

---

## 11. Key file index

| Path | Role |
| --- | --- |
| `architecture.md` | Routing law + topology SoT |
| `scripts/desktop-instance.sh` | Named instances, ports, shared Cargo |
| `scripts/desktop.sh` | Canonical build / install |
| `apps/synth_desktop/src-tauri/src/lib.rs` | Tauri command surface |
| `…/core_runtime.rs` | Composition root |
| `…/codex.rs` | Codex app-server manager |
| `…/credential_broker.rs` | Loopback key custody |
| `…/synth_config.rs` | Profiles + gateways |
| `…/device_auth.rs` | Browser device pairing |
| `…/account.rs` / `account_cloud.rs` | Summary + snapshot/billing |
| `…/inventory.rs` | Containers + traces + usage |
| `…/trace_ingest.rs` | Trace V5 via `synth-trace` |
| `…/visuals/` · `visuals_ipc.rs` | Visual registry + IPC |
| `…/optimizers/` | Recipes + cloud reconcile |
| `…/laguna.rs` | Local Laguna control |
| `src/renderer/…/App.tsx` | Shell / routing |
| `…/desktopBridge.ts` | invoke/events (+ legacy fallback) |
| `…/nativeCodex.ts` | Start/turn mapping |
| `packages/runtime-protocol/` | Shared TS contracts |
| `AUTH_FLOW.md` · `LAGUNA_GATEWAY_DESKTOP.md` | Auth + gateway tables |
| `apps/synth_desktop/PROVENANCE.md` | Friends ZIP binding |
| `docs/launch/LAUNCH_OPS.md` | Friends/public ops |

---

## 12. How to read this for v0.2

1. **Refactor (stream A)** happens inside `apps/synth_desktop` + MCP bins; quarantine Electron/legacy Python, don’t redesign around them.  
2. **Green means** Desktop Playwright/Bombadil + friends-bar QA ([testing#27](https://github.com/synth-laboratories/testing/pull/27) runbook).  
3. **Experiments / UML / Nemotron / `workshop_spend`** are mostly **backend or missing UI** — productize deliberately, don’t assume Desktop surfaces exist.  
4. **Stable public flag stays `0.1.0`** until deliberate v0.2 publish + PROVENANCE + FE hash flip.
