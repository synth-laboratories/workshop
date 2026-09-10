# Agent guide

Workshop is a Rust/Tauri and React/TypeScript macOS application. Begin with
`./scripts/install.sh --check`, install dependencies with `./scripts/install.sh`,
and build locally with `./scripts/workshop.sh build-and-run`.

Keep generated renderer bindings synchronized with the Rust command surface.
Do not edit generated protocol bindings by hand; regeneration and the private
verification suite belong to the release engineering checkout.

Do not add secrets, machine-specific paths, release evidence, test corpora, or
private release tooling to this public source export. Provider credentials are
not needed to build. For provider exercises, use an authorized project-local
`.env` and Workshop's ephemeral secrets proxy; never use macOS Keychain.

v0.10 has one production feature envelope. Local builds and official downloads
are ad-hoc signed and not Apple-notarized. Never claim otherwise.
