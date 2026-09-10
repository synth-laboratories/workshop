# Four agents, one shared world

Workshop example: **Woodland sprint**, two teams of two scripted actors chopping
trees and communicating. This adapts RuneBench; it is not the Runite Race task
or a claim about model performance. It never connects to official game servers.

## Start here

From the Workshop repository after `scripts/install.sh`:

```bash
bash examples/runebench/example.sh setup
bash examples/runebench/example.sh sample
bash examples/runebench/example.sh review
```

Open http://127.0.0.1:8128/viewer. The bundled, checksum-verified sample is a real
September 7 recording: four actors, zero model calls. Reviewing it needs neither
Docker nor provider credentials. It is retained evidence, not a fresh run.

For a fresh episode, stop the review server with Ctrl-C, run
`bash examples/runebench/example.sh run`, then start `review` again.
`run` is **scripted by default**, four actors,
25 seconds after readiness, three decisions per actor, zero model calls. Image
download and startup take longer than the episode. The public AMD64 game image
is about 4 GB and uses emulation on Apple Silicon. Docker Desktop or OrbStack
must be running; setup does not install or start either without your involvement.
Setup installs an isolated, hash-locked Python environment and ffmpeg if missing.

The inspector starts on All agents: focus one actor, compare a second, follow
messages and actions, query unsuccessful chops or same-tree attempts, and open
the exact source record and recorded game frame. Recording startup gaps are
labeled. Same-tree attempts are not proof of causal interference.

Results are retained in `results/`, using a unique directory for each run. The
runner checks all-player readiness, cross-actor capability rejection, authoritative
cutoff, rejection of late actions, and an unchanged cutoff receipt. It stops only
its own example container. A per-checkout lock prevents overlapping runs, and
an occupied controller port fails before launch. Ctrl-C stops the review server.

## Use from a Workshop conversation

Give Workshop [PROMPT.md](PROMPT.md). `viewer.tsx` uses Workshop's shared trace
components. `build_web.mjs` also produces `visual-request.json`, the source and
bindings for native visual creation. Treat that generated file as local/private:
it contains a local annotation capability, not a provider credential. Never
commit it or distribute it with the sample.

## Optional model-driven run

Only after scripted review works, explicitly authorize the provider budget:

```bash
bash examples/runebench/example.sh run --provider openrouter --env-file /absolute/private.env
```

The file must contain `OPENROUTER_API_KEY=...`; keep it mode 600 and out of Git.
ChatGPT login for the Workshop conversation does not authenticate these game
policies. This example supports Luna/Terra policy IDs, not Astra game policies.
No keys enter Docker and no Keychain importer is used. OpenRouter requests disable
provider fallbacks. Default batch reservations persist with a $5/128-call ceiling;
an unknown-cost call stops further admission. Never rename a batch to evade a cap.
No paid acceptance run is included in this example's verification.

## Reproducibility and attribution

`manifest.json` identifies the immutable upstream image and trace schema.
Container adapter code comes from public `synth-containers==0.4.3`, not a sibling
source checkout. Visuals come from this Workshop checkout and its npm lockfile.
No private evals framework is required. `requirements.lock` pins Python packages
and hashes. Game RNG is unseeded; repeats are descriptive, not paired-seed trials.

Built on [RuneBench](https://github.com/MaxBittker/runebench),
[rs-sdk](https://github.com/MaxBittker/rs-sdk), and LostCity. Upstream code and
game assets retain their respective notices and terms. See `THIRD_PARTY.md`.
