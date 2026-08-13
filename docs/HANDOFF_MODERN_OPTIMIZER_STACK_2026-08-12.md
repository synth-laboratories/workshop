# Handoff — modern optimizer / Workshop completion branches

Start with [`MODERN_OPTIMIZER_STACK_STATUS_2026-08-12.md`](./MODERN_OPTIMIZER_STACK_STATUS_2026-08-12.md).
The implementation is local-only and split across four branches/worktrees listed there.

## Merge order

1. Containers (`agent/aug12-harbor-dock-modern`): checkpoint inference consumer and
   Harbor/Dock fixtures.
2. optimizers-beta SFT (`agent/aug12-sft-runtime-completion`): hosted sampler and honest
   accelerator/campaign isolation.
3. optimizers-beta GELO (`agent/aug12-gelo-native-containers`): native Containers child
   lifecycle and projection.
4. Workshop (`agent/aug12-modern-stack-completion`): MCP parity, receipt driver, and
   documentation.

This is a dependency order, not permission to push. Keep commits local until review.

## Cross-repo checkpoint sampling contract

The optimizer owns provider access. Containers receives an inference target containing
`provider_endpoint_id`, `provider`, `auth_bearer`, `run_id`, `checkpoint_id`, and
`base_model`. It POSTs messages and `max_tokens` to the allowlisted endpoint and expects
`text` plus `usage`. The bearer is opaque and per run. The Tinker/provider key must
never cross into Containers.

The Containers client accepts loopback or an explicitly allowlisted origin, has no local
model fallback, keeps 409/unavailable results as a typed null score, and redacts auth in
debug output and events.

## Review traps

- Do not copy mixed root `Cargo.toml`, MAPO, tunnel, or Dockerfile changes into the GELO
  branch; only the focused `synth_go_ex` and projection changes belong.
- Do not revive a `live.dock_harbor.v1` template. Dock is private content over
  `live.harbor_eval.v1`, not a public fold or stream.
- Do not start a prepared rollout by editing the receipt. Workshop checks the current
  visual revision and exact binding again.
- Do not restart the shared SFT server, Docker daemon, or old Harbor jobs during review.
- Do not treat a credential absence as a test skip that passes acceptance.

## Local verification

```bash
# Workshop
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml --bin synth-optimizers-mcp
python3 -m unittest scripts.tests.test_modern_stack_dogfood -v

# Containers
uv run --with pytest python -m pytest \
  tests/test_banking77_platform.py tests/test_dock_eval_extension.py -q

# optimizers-beta SFT
cargo test -p synth_sft

# optimizers-beta GELO
cargo test -p synth_go_ex
```

After those are green, use the driver for a fixture run. Paid, Docker, dig.bench, kill,
and auth-rotation drills require the external prerequisites named in the status doc and
must produce their own receipt directories.
