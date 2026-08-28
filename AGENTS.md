# Agent guide

Workshop is a Rust/Tauri and React/TypeScript macOS application. Begin with
`./scripts/doctor.sh`, use `./scripts/bootstrap.sh` to install repository
dependencies, and use `./scripts/build.sh` for an unsigned release build.

Keep generated renderer bindings synchronized with the Rust command surface:

```bash
cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml \
  --lib regenerate_protocol_bindings -- --ignored
```

Do not edit `apps/synth_desktop/src/renderer/src/generated/protocol.ts` by
hand. Do not add secrets, machine-specific paths, release evidence, tests, or
private release tooling to the public repository. Provider credentials are
not needed to build. For local provider exercises, use a project-local `.env`
and Workshop's ephemeral secrets proxy; never use macOS Keychain.

The default build tier is stable. A runtime setting may narrow the compiled
feature envelope but must never broaden it.
