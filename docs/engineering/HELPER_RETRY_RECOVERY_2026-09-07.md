# Helper verification retry recovery

## Evidence

Pausing the development Workshop process reduced unconditional security-ticket
checks from 1,570 per ten seconds to zero. The process had repeatedly spawned
codesign. The computer-use status routes call refresh_grants, which attempted
helper launch on every failure; development launch checked Workshop's designated
requirement before checking whether the helper existed.

## Fix

ComputerUseService now checks helper existence before invoking signature tools.
Launch attempts, including errors and cancelled attempts, are limited to once per
30 seconds under the existing service mutex. Errors are retained for status and
stale identity/grant state is cleared. Every actual launch still receives full
signature verification; no successful verification cache or Gatekeeper bypass.

## Validation

- Two isolated RetryGate Rust tests passed, including 29,999 rejected rapid retries.
- `cargo check --lib -j 2` passed (existing warnings).
- `cargo build --bin synth-desktop -j 2` passed in 11m16s (existing warnings).
- Relaunched the rebuilt binary, PID 4043, with the existing scale-native-store
  data/config paths. Gracefully stopped the older packaged instance using the
  same data directory; no data deleted.
- Live local API: 60 computer-use status calls in 19 seconds, all correctly
  reporting not_installed. Final ten-second window: zero unconditional ticket
  checks, seven ticket fetches. Workshop CPU snapshot 0%; syspolicyd 102.4%.

This confirms suppression of this missing-helper polling storm, not elimination
of every source of macOS security-service CPU use. Repeated failure of an installed
helper is covered by the retry-gate tests, not a signed-helper live integration test.
