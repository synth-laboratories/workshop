# Workshop test-instance login contract

**Updated:** 2026-08-13
**Applies to:** every named Workshop development, CUA, and evaluation instance

## Operator rule

Starting a testing instance must require only the instance command:

```bash
./scripts/desktop-instance.sh cua-run benjamin
```

Do not ask the operator to prefix that command with flags, credential paths, or
environment-variable assignments. Do not document such prefixes as the normal
testing workflow.

## Required startup state

Every named testing instance starts with the machine's existing developer
authentication for:

- ChatGPT subscription / Codex
- OpenRouter
- Synth

The launcher resolves these automatically from standard private machine-local
settings. OpenRouter and Synth credentials are refreshed into the private
per-instance settings file; only those allowlisted credential fields are
copied. ChatGPT authentication remains file-backed and passwordless in debug
instances. Testing instances must not create, read, or modify a macOS Keychain
credential.

Per-instance routing, history, databases, Codex homes, and non-secret settings
remain isolated. No credential value may be printed in launcher output, logs,
manifests, generated shell snapshots, or test receipts.

## Acceptance criteria

A testing-instance change passes only when:

1. A plain `cua-run <name>` starts without a password or Keychain modal.
2. ChatGPT, OpenRouter, and Synth are already configured in the app.
3. No credential-related command prefix is required.
4. Re-running the launcher refreshes provider credentials without overwriting
   unrelated instance-local settings.
5. `./scripts/test-desktop-instance.sh` passes.

If automatic resolution fails, fix the launcher or machine-local settings.
Do not work around it by handing the operator another launch flag or exported
environment variable.
