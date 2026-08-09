# Synth Desktop architecture

## Non-negotiable execution model

Synth Desktop is an agent workbench, not a chat client. Every interactive session MUST be backed by exactly one of these agent runtimes:

1. **Synth Intern / cloud agent**, in sync or async mode.
2. **Codex app-server**, for local or configured-provider coding-agent sessions.

The renderer, Tauri host, and local runtime MUST NOT expose a direct model-chat path. In particular, the desktop MUST NOT send conversation turns directly to `/v1/chat/completions`, a model SDK, MLX, or a configured inference provider.

Local Laguna XS is a model used by Codex app-server. It is not an independent chat-session implementation.

```text
Forbidden
─────────

Desktop ───────────────► /v1/chat/completions ───────────────► model
Desktop ───────────────► MLX
Desktop ───────────────► configured model API


Required
────────

Desktop ─► local-runtime ─┬─► Synth Intern / cloud agent
                          │
                          └─► Codex app-server
                                  │
                                  └─► Responses-compatible provider
                                            │
                                            ├─► Laguna Responses shim ─► MLX sidecar
                                            └─► configured Responses API
```

## System topology

```text
┌──────────────────────────────────────────────────────────────────────────────┐
│                         SYNTH DESKTOP — TAURI 2                              │
│                                                                              │
│  ┌──────────────┐  ┌───────────────────────────┐  ┌───────────────────────┐  │
│  │ Conversations│  │ Workbench                 │  │ Files / Diff / Visual │  │
│  │ Sessions     │  │ Agents · Runs · Evaluation│  │ Artifacts · Traces    │  │
│  └──────────────┘  └───────────────────────────┘  └───────────────────────┘  │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │ Terminal panel: user PTYs · cwd · process state · persistent output    │  │
│  └────────────────────────────────────────────────────────────────────────┘  │
│                                    │                                         │
│                   typed Tauri commands + event streams                       │
└────────────────────────────────────┬─────────────────────────────────────────┘
                                     │
┌────────────────────────────────────▼─────────────────────────────────────────┐
│                         TAURI / RUST HOST LAYER                              │
│                                                                              │
│  workspace/files/git     terminal/PTY manager       sidecar supervisor       │
└────────────────────────────────────┬─────────────────────────────────────────┘
                                     │
                                     ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                    SYNTH LOCAL RUNTIME — PYTHON :8765                        │
│                                                                              │
│  session routing · persistence · normalized events · traces · inventory      │
│                                                                              │
│           ┌───────────────────────┴───────────────────────┐                  │
│           │                                               │                  │
│           ▼                                               ▼                  │
│  ┌──────────────────────┐                      ┌──────────────────────────┐  │
│  │ Codex session adapter│                      │ Intern/cloud adapter     │  │
│  └──────────┬───────────┘                      │ sync or async agents     │  │
│             │ NDJSON JSON-RPC / stdio          └─────────────┬────────────┘  │
└─────────────┼────────────────────────────────────────────────┼───────────────┘
              │                                                │
              ▼                                                ▼
┌────────────────────────────────────────┐          ┌─────────────────────────┐
│ CODEX APP-SERVER                       │          │ SYNTH INTERN / CLOUD    │
│ `codex app-server --listen stdio://`   │          │ agent runtime           │
│                                        │          │                         │
│ thread/start · turn/start · item/*     │          │ sync · async · mailbox  │
│ approvals · shell · files · patches    │          └─────────────────────────┘
│ optional MCP tools                     │
└───────────────────┬────────────────────┘
                    │ OpenAI Responses protocol
                    ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                  RESPONSES-COMPATIBLE MODEL PROVIDERS                        │
│                                                                              │
│       ┌────────────────────────────────┐   ┌──────────────────────────────┐  │
│       │ Local Laguna Responses server │   │ Configured Responses API     │  │
│       │ :7333 /v1/responses           │   │ provider/base URL/model      │  │
│       │ Responses ↔ MLX translation   │   │ credentials from secure cfg │  │
│       └───────────────┬────────────────┘   └──────────────────────────────┘  │
└───────────────────────┼──────────────────────────────────────────────────────┘
                        ▼
┌──────────────────────────────────────────────────────────────────────────────┐
│                   LAGUNA XS 2.1 MLX SIDECAR                                 │
│                                                                              │
│  Apple Silicon / Metal · local model lifecycle · streaming token generation │
└──────────────────────────────────────────────────────────────────────────────┘
```

## Session routing

The session record MUST identify its agent runtime explicitly. There is no fallback `chat` runtime.

```text
AgentSession
  runtime: "intern" | "codex"

  when runtime == "intern":
    mode: "sync" | "async"
    cloud session/job identity

  when runtime == "codex":
    codexThreadId
    cwd
    provider configuration
    model identifier
```

Unknown or unavailable runtimes fail closed with a visible configuration or readiness error. They MUST NOT silently fall back to direct model chat.

## Local Laguna request path

```text
User turn
   │
   ▼
Synth Desktop
   │
   ▼
local-runtime
   │ thread/start or turn/start
   ▼
Codex app-server
   │ POST /v1/responses
   ▼
Laguna Responses server :7333
   │ translate Responses input/tools to the MLX backend
   ▼
Laguna XS 2.1 MLX sidecar
   │ streamed inference
   ▼
Responses SSE events
   │
   ▼
Codex item/* events
   │
   ▼
local-runtime normalized and persisted events
   │
   ▼
Synth Desktop
```

Codex requires the Responses wire protocol. The Laguna Responses shim therefore remains in the local path even when its underlying MLX implementation uses a different internal request format.

## Configured model APIs

Configured model APIs are also consumed through Codex app-server. A provider configuration supplies a Responses-compatible base URL, model identifier, and credentials to the isolated Synth Codex home. The desktop does not hold a parallel provider-specific chat implementation.

If an upstream model service only supports chat completions, compatibility translation belongs in a dedicated Responses gateway beside the Laguna shim—not in the renderer, Tauri host, session adapter, or Codex client.

## Component ownership

```text
Tauri/Rust host    windows, permissions, filesystem access, PTYs, sidecars
local-runtime      session routing, persistence, normalized events, inventory
Codex app-server   coding-agent turns, tool use, patches, shell, approvals
Intern/cloud       managed cloud-agent execution in sync or async mode
Responses servers model-provider compatibility and streaming translation
MLX sidecar        local model loading and inference
```

User terminal sessions and Codex shell-tool executions are distinct processes. The UI may project both into a common activity surface, but it must preserve their ownership and clearly label whether a command was initiated by the user or by an agent.
