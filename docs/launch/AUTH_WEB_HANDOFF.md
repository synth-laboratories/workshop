# Auth, web, download, and account handoff (WP6)

Friends-release still uses **production Clerk**. `+clerk_test` / `424242` are forbidden on the candidate.

## Device-init / pairing

- Desktop begins sign-in via `account_begin_sign_in` and polls `account_poll_sign_in`.
- The browser must return **JSON** device-init, not a redirect. A redirect regression is a Gate F no-go.
- Exercise: expiry, denial, wrong browser profile, offline, backend 5xx, duplicate callback, app close mid-pair.
- Sign-out copy is “this device,” not “delete my work.” Local threads/files remain.

## Download / site

- usesynth.ai desktop + one mobile width: desktop-only copy on mobile, version, checksum, requirements, pricing, privacy, support, known issues.
- No Intern, no staging hosts, no fixture-as-live claims.
- Download object SHA256 must match the signed artifact receipt.

## Upgrade deep link

Isolate frontend commit `bfd2d5a3` (follow-up `4638f3d7` on `frontend-desktop-upgrade` opens the plan sheet when Desktop hands off an upgrade). Do **not** merge unrelated Intern dirt from that worktree.

Desktop `account_open_billing` → backend `/checkout-session` must return `mode=provider` with a Stripe/Autumn URL for Starter and Pro. Fallback `hosted_web` is not Gate F.

## Account snapshot

Backend `feat/desktop-account-snapshot` @ `ac9ae580f` plus this pass’s Autumn checkout adapter + fake-Autumn. Allowance cents come from entitlements (`smr_spend` 0 / 2000 / 20000), never from a hardcoded catalog when Autumn is configured.

## Rehearsal

Follow [CLEAN_USER_REHEARSAL.md](./CLEAN_USER_REHEARSAL.md). Record artifact SHA, account ids (not keys), and a short screen recording.
