# Internal development notes

## Connecting Synth Desktop to an Intern backend

Synth Desktop keeps backend routing and credentials separate:

- Routing is stored in `~/.synth-desktop/config.toml`.
- Secrets default to `~/.synth-desktop/.env` and are written with mode `0600`.
- The TOML file may point to a different env file for a checkout, slot, or local test environment.
- Secrets remain in the native Tauri host. The renderer receives only configured status, credential source, and a short fingerprint.
- Intern requests use the configured Synth API key as Bearer authentication.

### Settings or Computer Use

Open **Settings → Account → Synth API**:

1. Select **Local**.
2. Set **Backend API** to the local API, normally `http://127.0.0.1:8000`.
3. Enter the API key accepted by that backend.
4. Click **Save and reconnect**.

The same flow can be driven with Computer Use. Entering or replacing an API
key creates persistent access and must follow the Computer Use confirmation
policy. Saving updates TOML/the private env file and restarts the compatibility
runtime with the resolved configuration.

### Configure files directly

```toml
# ~/.synth-desktop/config.toml
[intern]
profile = "local"
env_file = "~/.synth-desktop/.env"
api_key_env = "SYNTH_API_KEY"
worker_key_env = "SMR_WORKER_API_KEY"

[intern.endpoints]
prod = "https://api.usesynth.ai"
staging = "https://api-dev.usesynth.ai"
local = "http://127.0.0.1:8000"
```

```dotenv
# ~/.synth-desktop/.env
SYNTH_API_KEY=your-local-key
```

Protect a manually created env file:

```bash
chmod 600 ~/.synth-desktop/.env
```

Restart Synth Desktop after editing the files directly. If the app is already
open, **Save and reconnect** in Settings applies the configuration and restarts
the runtime.

An API key is required even for the local profile. This keeps Intern fail-closed
and prevents the desktop from silently treating an unauthenticated endpoint as
a configured mailbox.

### Resolution precedence

The native host resolves configuration in this order:

1. Process overrides such as `SYNTH_BACKEND_URL`, `SYNTH_API_KEY`, and
   `SYNTH_INTERN_PROFILE`.
2. Secrets from the configured env file.
3. The selected TOML profile and its `[intern.endpoints]` entry.
4. Built-in production, staging, and local defaults.

Settings displays the effective backend and credential source. If a process
environment value is present, it continues to override a value saved to the env
file; the UI calls this out after saving.

### Internal Codex activity

The public Synth API key enables the Sync/Async Intern mailbox. For the richer,
worker-only Codex execution SSE, also provide a worker credential:

```dotenv
SMR_WORKER_API_KEY=your-worker-key
```

Slot environments may project the same credential as
`SYNTH_EVAL_EXEC_WORKER_API_KEY`. The Rust host consumes it without exposing it
to the renderer. Codex activity appears in the Cloud Activity pane as a separate,
non-authoritative lane; mailbox events remain authoritative for messages,
receipts, controls, checkpoints, and completion.

### Auth model rationale

This follows the patterns already used by the native integrations:

- Codex configures a provider `env_key` in a session-specific `CODEX_HOME` and
  injects the credential into the native process rather than application state.
- Laguna generates or loads a private local key, uses Bearer authentication, and
  injects the key into its sidecar/client without asking the renderer to retain it.
- Synth backend credentials likewise cross the renderer boundary only when a user
  explicitly saves a replacement, and are never read back into the UI.
