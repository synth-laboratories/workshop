# understudy

> **The name is provisional.** `understudy` — a trained performer who plays the
> principal's part for real — is the working title from the design record, not a
> decision. It appears in exactly two places, the `name` field in `Cargo.toml`
> and this heading, so renaming is a two-line change. Everything else (binary
> name, `Server:` header, `/version` payload, usage text) is derived from
> `CARGO_PKG_NAME`. Container names, labels, the guest image tag and the state
> directory are deliberately *not* derived from it, so a rename cannot orphan a
> fleet of running guests or a slot's registered ssh keys.

An API-compatible local implementation of the exe.dev control plane.

Slots that need an exe.dev to talk to, without exe.dev credentials, point the
unmodified `ExeDevClient` at this binary. The code under test stays entirely
real: the real client, the real control-plane grammar, real Docker guests
running real sshd. This is the relationship MinIO has to S3 — a stand-in that is
checked against the thing it stands in for.

It is **not** a fake, and it is **not** an acceptance substitute. Proving that
this honours `rm` proves nothing about whether exe.dev does. The shape stays:
iterate locally for free, then one real-provider pass before those rows count as
green. The conformance suite is what keeps that final pass boring.

---

## Versioning, and why a slot must pin one

Semver from `0.1.0`, with a `CHANGELOG.md`. **A local slot pins a version.**

The whole reason this exists as its own repo is that its predecessor did not
have one: slots depended on the behaviour of a file inside the backend with no
pin, so any edit to it silently changed what every local slot exercised. Pinning
restores the property that a slot's behaviour changes only when somebody changes
the slot.

Pin it the way images are pinned — a tag, a release binary checksum, or the
versioned path from `scripts/install-local.sh`, recorded next to the slot
definition, never `main`. `GET /version` reports the running version so a slot
can assert what it actually got:

```json
{ "version": "0.1.0", "server": "understudy/0.1.0", "api": "exe-dev-control-plane",
  "guest_runtime": "docker", "local_extensions": ["local-guest-endpoint.v1"] }
```

While the version is `0.x`, the minor is the compatibility unit: `0.1.x` is
bug-fix compatible, `0.2.0` may change the wire contract.

---

## Install / pin a local binary

Slots that need a stable absolute path (without waiting on a GitHub Release)
build and pin with:

```sh
./scripts/install-local.sh
# → ~/.local/share/understudy/v0.1.0/understudy
# → ~/.local/bin/understudy  (symlink; pass --no-symlink to skip)
```

Record the pin path (or the `v0.1.0` tag once releases exist) next to the slot
definition. Override the share root with `PREFIX=…` or the symlink directory
with `BIN_DIR=…`. The script prints the absolute binary path, `understudy
version`, and the `GET /version` JSON shape.

## Quickstart

```sh
# 0. Optional: install a pinned release binary (see above).
./scripts/install-local.sh

# 1. Build the guest image (once).
docker build -t exe-dev-local-sshd:latest -f docker/Dockerfile.sshd docker

# 2. Run the control plane.
cargo run --release -- serve
# or: ~/.local/share/understudy/v0.1.0/understudy serve

# 3. Generate a client ssh identity. `ssh-key add` registers the public half,
#    and the control plane injects it into every guest.
ssh-keygen -t ed25519 -N "" -f ~/.exe-dev-local/id_ed25519
```

`serve --help` lists every flag. Two defaults are worth knowing:

- **It binds `127.0.0.1`, not `0.0.0.0`.** The Python reference bound every
  interface. This process holds an ssh key store and publishes guest ports, so
  the safe default is the one an operator widens on purpose. Slots running
  *inside* Docker need `--bind-host 0.0.0.0 --guest-host host.docker.internal`.
- **Any non-empty bearer is accepted** unless `--api-token` is given. That is a
  local convenience, not an authentication boundary, and the startup banner says
  so.

### Pointing a slot at it

```sh
EXE_DEV_API_ENDPOINT=http://host.docker.internal:8790
EXE_DEV_API_TOKEN=local                # any non-empty token unless --api-token
EXE_DEV_API_TRANSPORT=https
EXE_DEV_SSH_USER=exedev                # matches the guest image's login user
EXE_DEV_SSH_KEY_PATH=$HOME/.exe-dev-local/id_ed25519

# Guest endpoint resolution — dynamic, per-VM ports resolve automatically:
EXE_DEV_GUEST_PORT_MAP_URL=http://host.docker.internal:8790/guest
# ...or static, for a single known VM:
# EXE_DEV_SSH_HOST=host.docker.internal
# EXE_DEV_SSH_PORT=<port from `ls --json` or /guest>
```

For processes on the host itself, use `http://127.0.0.1:8790`.

Setting either guest-endpoint knob switches the client's guest exec from the
nested exe.dev control-plane hop to direct `ssh user@host -p <port>` (and
`scp -P <port>`). Leaving both unset keeps real exe.dev behaviour byte-for-byte
unchanged.

### Self-test (Docker e2e)

```sh
# From the checkout (so docker/Dockerfile.sshd is findable). Builds the guest
# image if missing. Opt-in — not part of `cargo test` or CI.
cargo run --release -- self-test
# alias: cargo run --release -- e2e
```

Starts an ephemeral control plane in-process, then drives the real HTTP grammar
plus host `ssh`/`scp`: create a uniquely named VM → register a key → resolve
`/guest` → `ssh … true` → scp a file and `cat` it back → delete. Prints
`PASS`/`FAIL` per step; non-zero exit on any failure. Guests keep the
`exe-dev-local` label so the usual cleanup still works.

### Cleanup

Guest containers carry the `exe-dev-local` label:

```sh
docker rm -f $(docker ps -aq -f label=exe-dev-local)
docker rmi exe-dev-local-sshd:latest       # optional
```

Registered public keys persist in `~/.exe-dev-local/authorized_keys`
(`--state-dir` to relocate, `--state-dir=` for memory-only).

---

## API surface

The control plane is one POSIX-quoted command line POSTed as `text/plain` with a
bearer token — exactly what `ExeDevClient` emits:

| Command | Status |
|---|---|
| `new --name=<vm> [--disk --cpu --memory --image --comment --tag --no-email --setup-script]` | implemented; `--cpu`/`--memory`/`--image` are applied to the guest, `--setup-script` is executed in it |
| `ls [<vm>] [--json]` | implemented |
| `rm <vm>` | implemented |
| `restart <vm>` | implemented; ssh keys are replayed |
| `cp <source> <target>` | implemented, via a filesystem snapshot |
| `resize <vm> --cpu --memory` | **implemented** — actually reconfigures the guest |
| `resize <vm> --disk` | **refused, 501** — see divergences |
| `ssh-key add [--tag <tag>] <key>` | implemented, idempotent, fans across the fleet |
| `ssh-keygen` | **refused, 501** — see divergences |
| `billing capacity` / `billing plan` | implemented from measured host capacity |
| `share set-public` / `share port` | **refused, 501** — see divergences |

Plus `GET /healthz` and `GET /version`.

Errors carry both the prose the client's classifier greps and a stable machine
code: `vm not found: "slot-a" does not exist\ncode=vm_not_found`. The prose half
is a wire contract, not decoration — the client decides "this VM is simply
absent" by substring — and there is a test asserting every error code falls on
the intended side of that classifier.

### `GET /guest?vm=<name>` is a local-only extension

**It is this service's own contract addition, not an exe.dev API surface.** Real
exe.dev guests are always `{vm}.exe.xyz:22`; the real provider has no such route
and never needs one. It exists here only because local guests share one host and
therefore need per-VM ports. Its response says so in band:

```json
{ "vm": "slot-a", "host": "127.0.0.1", "port": 42256,
  "extension": "local-guest-endpoint.v1" }
```

This is enforced by construction rather than by convention — see below.

---

## Conformance: one suite, two targets

The suite lives in `src/conformance/` and is the reason this is a separate,
versioned repo. Without it, "it speaks the exe.dev grammar" is an assertion
nobody checks, and provider drift surfaces as a mystery acceptance failure
instead of a red build here.

**Assertions are written against what the *client* would conclude**, not against
our response shapes — `conformance::client_view` is a faithful port of
`_control_response_is_missing_vm` and `_extract_vm` from `client.py`. A suite
that asserted on our own shapes would only prove we agree with ourselves.

**The local extension cannot leak into the shared suite.** `ControlPlaneTarget`
exposes exactly one capability, "post a command line and read the answer", which
is the entire exe.dev control-plane API. `GET /guest` lives on a separate
`GuestEndpointExtension` trait that the real target does not implement. So "no
shared assertion requires the local extension" is not a rule to remember while
writing a case — it is a compile error. `conformance::tests::
the_shared_suite_needs_nothing_but_the_exe_dev_api` runs the whole shared suite
against a wrapper that implements only the shared trait, which is the proof.

```sh
cargo run -- conformance                       # in-process server, no Docker needed
cargo run -- conformance --target 127.0.0.1:8790   # a running instance
cargo run -- conformance --target real         # real exe.dev; defaults to --no-lifecycle
cargo run -- conformance --target real --lifecycle  # creates a billed VM; opt-in only
cargo run -- divergences                       # print the divergence registry

# Operator helper: extract EXE_DEV_API_TOKEN from a slot compose.env (never sources the whole file)
./scripts/run-real-conformance.sh /path/to/compose.env              # --no-lifecycle
./scripts/run-real-conformance.sh /path/to/compose.env --lifecycle  # billed VM; clean up
```

**Real target env** (same names the exe.dev client reads):

| var | required | notes |
|---|---|---|
| `EXE_DEV_API_TOKEN` | yes | absent → suite reports `SKIP` with reason, never a silent pass |
| `EXE_DEV_API_ENDPOINT` | no | defaults to `https://exe.dev/exec`; non-HTTPS is refused |

> **Real half status.** No-lifecycle against live exe.dev has been measured
> (see CHANGELOG Proved). `--target real` defaults to `--no-lifecycle` so a
> casual run does not create a billed VM. Lifecycle cases (`new` → observe →
> `rm`) are opt-in via `--lifecycle` / the script's `--lifecycle` flag; do not
> leave orphans. Unconfigured runs still `SKIP` loudly
> (`EXE_DEV_API_TOKEN is unset...`). The helper never `source`s a full slot
> compose.env (those files contain unquoted SSH keys).

### Known divergences

Every place this knowingly differs from real exe.dev is registered in
`src/conformance/divergence.rs` with what the real provider does, what this does
instead, and why. When the target correctly refuses a registered surface, the
suite reports `PASS -- refused (<id>)` (still counted in PASS, with the registry
id on the note). Accepting that surface with 2xx is a hard `FAIL`. Deleting a
registry entry without implementing the surface (or removing the case) is how
the gap record is lost — do not do that.

| id | surface | here |
|---|---|---|
| `share-over-http` | `share set-public`, `share port` | refused 501 — the real client routes `share` over the account SSH identity and never posts it, so there is no honest local implementation |
| `ssh-keygen-over-http` | `ssh-keygen` | refused 501 — no account key material to generate; no exe.dev client path emits it over the control plane |
| `resize-disk` | `resize --disk` | refused 501 — a container has no private volume; `--cpu`/`--memory` are implemented |
| `new-disk-ignored` | `new --disk` | accepted, not acted on, reported back in `ignored_options` (refusing it would reject the client's default command line) |
| `ls-ssh-port-field` | `ls --json` | each entry carries a local-only `ssh_port` |
| `guest-endpoint-extension` | `GET /guest` | served; no such route on real exe.dev |
| `billing-plan-is-local-capacity` | `billing` | measured host CPU/memory plus a declared `--pooled-disk-gb` |

The first three are the fidelity gaps the design record called out in the Python
reference, where each was acknowledged with a plausible 200 that did nothing.
An acknowledgement that does nothing is the worst option: the caller believes
the effect happened. They now fail loudly.

---

## CI

`.github/workflows/ci.yml` runs on every push and pull request:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo run -- conformance          # in-process / memory path — no Docker
```

No Docker daemon and no exe.dev credentials are required for the gate.

## Release binaries

`.github/workflows/release.yml` triggers on `v*` tags, builds `--release`
binaries, uploads them as workflow artifacts, and attaches them to a GitHub
Release. Native runners only (no cross-compile):

| runner | artifact triple | notes |
|---|---|---|
| `macos-14` | `aarch64-apple-darwin` | darwin-arm64 |
| `ubuntu-latest` | `x86_64-unknown-linux-gnu` | linux-x86_64 |
| `ubuntu-24.04-arm` | `aarch64-unknown-linux-gnu` | linux-aarch64 |

There is no darwin-x86_64 matrix entry; Intel Mac slots use
`scripts/install-local.sh`. Until this repo has a git remote and a tag is
pushed, the workflow files are landed but no GitHub Release exists — pin via
the local install script.

## Development

```sh
cargo fmt
cargo clippy --all-targets -- -D warnings    # pedantic, warnings are errors
cargo test                                   # no Docker, no network, no credentials
cargo run -- conformance                     # same gate CI runs
cargo run --release -- self-test             # Docker e2e; opt-in, builds image if missing
```

Style is `Synth Style` (`tigerstyle.md`): 100 columns, 4-space indent, pedantic
clippy as errors, `#[must_use]`, explicit limits on every loop and buffer,
assertions for programmer error and `Result` for operating error.

Layout:

| module | responsibility |
|---|---|
| `grammar` | trust boundary — command line in, typed `ControlCommand` out |
| `control_plane` | what commands mean, and which this target refuses |
| `http_edge` | routing and error rendering; translation happens exactly once, here |
| `guest` | the container-layer trait — everything above it is daemon-free |
| `docker_guests` / `memory_guests` | the two guest fleets |
| `self_test` | Docker e2e: create → ssh → scp → delete over real HTTP + host ssh/scp |
| `conformance` | the suite, the two targets, the oracle, the divergence registry |
| `error` | the stable error vocabulary and its wire contract |

Three dependencies, each load-bearing: `serde`/`serde_json` for the response
encoding, and `shlex` because matching Python's `shlex.split`/`shlex.join` byte
for byte is a correctness requirement — that is the wire format. The HTTP
server, the CLI parser and the CRC-32 used for port derivation are hand-written
rather than pulled in: the surface is small, and the deliverable is a single
static binary that runs beside a slot.

`cargo test` requires no Docker. The container layer sits behind
`guest::GuestRuntime`; the tests drive `MemoryGuestRuntime`, and the conformance
suite runs against a real HTTP server over that same in-memory fleet, so the
grammar, the edge and the refusals are all exercised end to end without a daemon.
