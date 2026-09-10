# Workshop

Workshop is a local-first desktop workbench for agent conversations, visual
artifacts, container evaluations, and optimization workflows.

Official downloads and checksums: [usesynth.ai/download](https://www.usesynth.ai/download).
Product documentation: [docs.usesynth.ai](https://docs.usesynth.ai).

## ChatGPT, Astra, and OpenRouter

After building, open **Settings → Models → ChatGPT subscription** and complete
the browser login. If the callback port is occupied, paste the full redirect URL
into the manual callback field. Cancel abandons that attempt. “Connected” means
signed in, not guaranteed access to every model; reconnect if the session expires.

Select **ChatGPT → GPT-6 Astra** (`gpt-6-astra`) to use your subscription.
Alternatively, use **OpenRouter → API credits** in Settings to select an absolute
path to a private env file containing `OPENROUTER_API_KEY=your-key`. Restrict it
with `chmod 600 /absolute/path/to/private.env`; never commit it or paste the key
into the path field. Preserve existing Synth variables: this env-file setting is
shared. This setup does not import credentials into Keychain.

Select OpenRouter Astra (`openai/gpt-6-astra`) explicitly. OpenRouter credits are
separate from ChatGPT allowance; failed ChatGPT requests never automatically
switch to a paid API. Key detection does not verify credit or model entitlement.
Astra offers Low through Max reasoning (Medium default), text/image input, a
1,050,000-token context, conservative 250,000-token auto-compaction, and Standard
service. Access still depends on the chosen account/provider.

Source builds bundle native Codex 0.153.0, with no global Codex or Node required
when launching from Finder. Remove an obsolete `SYNTH_CODEX_BIN` override if the
runtime check fails. These changes apply to builds from this source, not older
downloaded binaries.

## Install or build locally

The v0.10 macOS app is **ad-hoc signed and not Apple-notarized**. It does not
require Apple Developer credentials. Verify the published archive checksum
before opening a download. macOS may require approval for that exact app in
System Settings → Privacy & Security → Open Anyway. Do not disable Gatekeeper
globally. Updates are manual through the official download page.

The supported desktop target is Apple silicon running macOS 14 or later.
Intel Macs, Windows, and Linux desktop packages are not provided by this release.
Local models require additional memory, storage, and separately downloaded weights.
Cloud models and account features require network access and their own credentials.

From a source checkout on an Apple silicon Mac:

```bash
./scripts/install.sh --bootstrap && ./scripts/workshop.sh build-and-run
```

Bootstrap installs Homebrew if absent, Node 20/npm, Python 3.12, Rust stable,
uv, jq, ripgrep, Git, and locked npm dependencies. Homebrew's
[official interactive installer](https://brew.sh) may request administrator
approval. Apple command-line tools must provide Swift 6+: if missing, the script
opens Apple's installer and asks you to finish it and rerun. Older tools require
an update or selecting Xcode 16+. Apple prompts/licenses are not automated.
Existing `.env`, shell profiles, and the global Rust default are preserved;
the build scripts locate the Homebrew tools without shell configuration.
If prerequisites are already installed, use `./scripts/install.sh` instead.
`./scripts/install.sh --check` checks prerequisites without changing local
configuration; `--dry-run` previews project setup without installing anything.

The build fetches exact public Containers, Optimizers, and MLX source revisions
into `work/build-sources`. No sibling repositories or release credentials are
needed. It produces a separate **Synth Workshop Local.app** and DMG; the script
prints their locations. Use `./scripts/workshop.sh build` to build without
launching and `./scripts/workshop.sh run` to open an existing local build.
Local builds are also ad-hoc signed and unnotarized; they are not official
redistributable release artifacts.

**TBLite is for evaluations/testing only. It is not required to build or use
Workshop in production.** Runtime staging verifies the pinned dependencies.
Provider-backed evaluations and model calls may incur charges; inspect their
provider, model, limits, and cost controls before starting them.

The supported source-build entrypoints are [install.sh](scripts/install.sh)
and [workshop.sh](scripts/workshop.sh). Private release/test orchestration is not
included in this public checkout.

## Try four-agent review

After installing dependencies, start with the bundled RuneBench recording:

```bash
bash examples/runebench/example.sh setup && bash examples/runebench/example.sh sample && bash examples/runebench/example.sh review
```

Open `http://127.0.0.1:8128/viewer`. Focus one agent, compare a teammate,
inspect messages and unsuccessful actions, and follow source records and game
frames. This recorded sample needs neither Docker nor AI credentials.
The [example guide](examples/runebench/README.md) covers a fresh, free scripted
run using Docker. Give Workshop the [starter prompt](examples/runebench/PROMPT.md)
to guide setup and native visual review. These game-specific views are an
example built with Workshop's shared inspector, not automatic interpretation
of arbitrary logs.

## Connect an agent through MCP

Start Workshop and find its **Data root** in Settings → About. A connection grants
access to that entire local instance, including shared conversations and visuals;
it is not restricted to one chat. Select the intended instance explicitly.

For an official app installed in Applications:

```bash
"/Applications/Synth Workshop.app/Contents/MacOS/workshop" connect codex --data-root "/absolute/path/to/instance/data"
"/Applications/Synth Workshop.app/Contents/MacOS/workshop" connect claude --data-root "/absolute/path/to/instance/data"
```

For a local build, use the `workshop` executable in that app's `Contents/MacOS`
directory instead. From source, `npm run workshop --` builds/runs the matching
CLI; for example:

```bash
npm run workshop -- doctor --data-root "/absolute/path/to/instance/data"
```

Connection setup verifies the running instance and preserves unrelated client
configuration. It refuses to overwrite conflicting custom Workshop entries.
Restart the client after configuration changes and inspect its MCP connection.
The packaged CLI itself does not require Node or Rust to be installed.

Other MCP clients can launch the same executable using stdio and arguments
`mcp --data-root /absolute/path/to/instance/data`.

Ask the connected agent to discover the tools, then try:

> Create a Mermaid diagram showing Input → Analysis → Result. Open it full
> screen, capture it, and inspect the image.

Browser and Computer Use retain their opt-in controls in Settings → Context.
Agents cannot grant their own access or resolve their own human permission
requests. A presentation acknowledgement is not rendering proof: capture and
inspect the actual pixels. Some templates do not provide semantic observations.

## Host an ACP agent

ACP hosting runs an installed agent adapter inside Workshop. It is separate from
connecting an external agent through MCP. Plain `codex` or `claude` executables
are not necessarily ACP servers: install and pin an appropriate adapter.

Create `agent-backends.json` in the selected instance's data directory:

```json
[
  {
    "id": "my-agent",
    "command": "/absolute/path/to/installed/acp-agent",
    "args": [],
    "workspace": "/absolute/path/to/project",
    "envFile": "/absolute/path/to/project/.env",
    "maxSessions": 1,
    "maxTurnSeconds": 180
  }
]
```

Make the registry private with `chmod 600`. Set `envFile` to `null` if it is not
needed. A supplied environment file must belong to the configured workspace.
Workshop loads it directly; this does not import keys into the Keychain-backed
Secrets registry. Configure authentication through an authorized mechanism
before starting the adapter. Do not put credentials in MCP configuration.

The backend is a local executable running with your OS user's permissions.
**Its working directory is not an OS sandbox.** Register only programs you trust.
MCP cannot register arbitrary executables or expand this registry.

Use `workshop backends --data-root "/absolute/path/to/instance/data"` to inspect
the configuration. In Settings → Context → Hosted agents, start a task, send a
prompt, inspect its journal, answer permission requests, cancel, close, or
explicitly resume a retained task. Adapter capabilities determine resume support.
Connection loss fails pending work instead of replaying uncertain prompts.

## Runtime ownership and removal

The native runtime owns the database and managed work. Closing its window does
not necessarily stop the runtime. Use the matching CLI and explicit data root:

```bash
workshop runtime status --data-root "/absolute/path/to/instance/data"
workshop runtime attach --data-root "/absolute/path/to/instance/data"
workshop runtime detach --data-root "/absolute/path/to/instance/data"
workshop runtime stop --data-root "/absolute/path/to/instance/data"
```

Here `workshop` means the full path to the packaged executable shown above unless
you have placed that executable on PATH. Stop shuts down managed work, including
hosted ACP processes. Native capture requires a desktop-capable OS session.

Use `workshop disconnect codex` or `workshop disconnect claude` with the same
`--data-root` to remove an unchanged matching MCP registration. Restart the
client to close existing connections. For upgrades, keep the app and CLI on the
same revision, restart them, and rerun `doctor`. When moving paths, disconnect
the old registration before connecting the new one. Do not delete the data root
merely to update the application.

## Release integrity and limitations

Public package CI builds this exported checkout independently from pinned public
sources. Official publication uses the exact CI archive accepted on a native Mac,
not a fresh tag rebuild. The distribution manifest records source revision,
archive size, SHA-256, signing status, and notarization status. Source code or a
green compilation check alone does not prove a provider-backed workflow ran.

Retained/offline replay is not a live provider evaluation. Visual annotations
are analysis projections; they do not replace engine or verifier results. Model
credentials, provider availability, quotas, and optional runtimes remain
workflow-specific requirements.

Owned by [synth-laboratories](https://github.com/synth-laboratories).
See [LICENSE](LICENSE) for licensing terms.
