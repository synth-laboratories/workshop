# Local-slot configuration for Workshop grants

This is the recipe for running Workshop device enrollment, grants and grant
credentials (docs/WORKSHOP_GRANT_CONTRACT.md, version 2) on a synth-dev local
slot. It only documents configuration. This repo does **not** change synth-dev
or any slot config. §5 lists what the slot manager and compose file must
change before grants can work on a slot.

Facts about the current slot stack, read from synth-dev `3c78f087`:
- **Compose file:** `local_dev/infra/docker-compose.local-stack.yaml`.
  Services pass through **only** the variables listed in their `environment:`
  block (`mq-server`, `mq-worker`, the `x-mq-client-env` anchor merged into
  backend-api, and the SMR services).
- **Where values come from:** the slot manager (`slot-manager-rs`,
  `src/artifacts.rs`) writes `<slot state_dir>/compose.env`. It takes a fixed
  key table: `MQ_PORT` (allocated host port for `manderqueue`, default 8088),
  `MANDERQUEUE_HTTP_URL=http://mq-server:8088`,
  `MQ_BRIDGE_BASE_URL=http://backend-api:8000`,
  `LOCAL_MQ_SERVER_IMAGE=manderqueue-manderqueue:latest` and
  `APP_ENVIRONMENT=local`. It adds allowlisted keys imported from
  `backend/.env`, `backend/.env.local`, `synth-ai/.env` and
  `config/instances/<slot>.env`.
- **Legacy HS256 secret:** `MQ_JWT_SECRET` is derived from `JWT_SECRET_KEY`.

## 1. Generate the signing keypair (files only, 0600)

```bash
# From this repo. Choose a directory outside every git checkout.
scripts/gen-grant-signing-key.sh ~/.synth/slots/<slot-id>/mq-grants slot-<slot-id>-k1
```

The script writes `<kid>.pem` (the private key, which only the backend reads),
`<kid>.jwks.json` (the public JWK set, which MQ reads) and `<kid>.env` (the
kid, no secrets). All three are mode `0600`, in a `0700` directory. It refuses
to overwrite existing files and prints only paths and the kid. Keep one
keypair per slot, and never commit or paste these files. To check the
permissions without printing the contents, run
`stat -f '%Sp %N' ~/.synth/slots/<slot-id>/mq-grants/*`
(on Linux: `stat -c '%A %n'`).

Also generate a dedicated delivery secret. The worker needs it to sign bridge
deliveries and the backend needs it to verify them. The compose file does not
set it today.

```bash
( umask 077; openssl rand -hex 32 > ~/.synth/slots/<slot-id>/mq-grants/delivery-secret )
```

## 2. MQ (`mq-server` and `mq-worker`)

Both processes boot the same auth configuration, so **both** need the JWKS.

| Variable | mq-server | mq-worker | Value on a slot |
| --- | --- | --- | --- |
| `DATABASE_URL`, `REDIS_URL`, `MQ_BIND` | ✓ | ✓ (no bind) | unchanged |
| `MQ_AUTH` | ✓ | ✓ | `jwt` |
| `MQ_JWT_JWKS_FILE` | ✓ | ✓ | `/run/mq-grants/<kid>.jwks.json` (read-only mount of the public JWKS) |
| `MQ_JWT_SECRET` | ✓ | ✓ | unchanged (legacy HS256 for existing SMR/Intern callers; the grant credentials never use it) |
| `MQ_PROFILE` | optional | optional | leave unset (`deployed`: requires `DATABASE_URL` and `MQ_WRITE_BUFFER=off`, which the slot already satisfies) |
| `MQ_DELIVERY_JWT_SECRET` | – | ✓ **required** | contents of `delivery-secret` (≥ 32 bytes). The worker refuses to start without it. |
| `MQ_BRIDGE_BASE_URL` | – | ✓ | `http://backend-api:8000` (unchanged) |
| `MQ_JWT_JWKS_RELOAD_SECS` | optional | optional | reload interval for the JWKS file (default 30; `0` disables) |

Set exactly one of `MQ_JWT_JWKS_FILE` and inline `MQ_JWT_JWKS`; setting both
refuses to boot. The image must be built from a commit that contains
migrations `20260913000000_enrollments_and_grants` and
`20260913010000_enrollment_revocation`. MQ applies them itself on start. The
slot's `LOCAL_MQ_SERVER_IMAGE` must point at such a build.

## 3. Backend (`backend-api`)

| Variable | Value on a slot | Notes |
| --- | --- | --- |
| `MANDERQUEUE_HTTP_URL` | `http://mq-server:8088` | unchanged; used by the backend for its own MQ calls |
| `MANDERQUEUE_PUBLIC_URL` | `http://127.0.0.1:<MQ_PORT>` | The endpoint handed to Workshop on the host. Plain `http` is accepted **only** for loopback hosts. Use the slot's allocated `MQ_PORT`, not 8088, unless that is what was allocated. |
| `MQ_ISSUER_SIGNING_KEY_FILE` | `/run/mq-grants/<kid>.pem` | read-only mount of the private key, into backend-api **only** |
| `MQ_ISSUER_SIGNING_KID` | `<kid>` | from `<kid>.env` |
| `MQ_ISSUER_ADDITIONAL_JWKS` | unset | set only during a rotation overlap (public keys only) |
| `MQ_DELIVERY_JWT_SECRET` | contents of `delivery-secret` | the same value as the worker; the bridge fails closed (503) without it |
| `WORKSHOP_BACKEND_ORIGIN` | see below | desktop identity |
| `WORKSHOP_BACKEND_ID` | a fixed, non-nil UUID per slot | e.g. `uuidgen` once, stored with the slot |
| `WORKSHOP_PROFILE_ID` | a fixed, non-nil UUID per slot | e.g. `uuidgen` once, stored with the slot |

The grant endpoints (`/api/v1/mq/...`) verify every caller through the
desktop cloud identity (`services/desktop_cloud_identity.py`). That requires
all three `WORKSHOP_*` variables. `WORKSHOP_BACKEND_ORIGIN` must be a
canonical **`https`** origin: no path or query, and port 443 or none. Without
them, every grant endpoint returns `503 desktop_cloud_identity_not_configured`.
`/api/v1/mq/jwks.json` needs only the signing key.

**Local slots serve plain HTTP, so there is no compliant value today.** One of
these is needed (§5, item 4):
- **(a)** Put a TLS terminator in front of the slot backend on 443, with a
  locally trusted CA, and set `WORKSHOP_BACKEND_ORIGIN=https://<slot-host>`;
  or
- **(b)** Make a backend change that accepts a loopback `http` origin when
  `APP_ENVIRONMENT=local`. It has not been made and needs its own review,
  because it relaxes the identity-origin check.

Workshop must be pointed at the same origin as `WORKSHOP_BACKEND_ORIGIN`.

## 4. Checking a configured slot (no secrets printed)

```bash
# The published JWKS must list the kid and only public members.
curl -s http://127.0.0.1:<BACKEND_PORT>/api/v1/mq/jwks.json | python3 -c \
 'import json,sys; k=json.load(sys.stdin)["keys"]; print([x["kid"] for x in k]); assert all("d" not in x for x in k)'
# The MQ boot log names the auth mode; expect auth=jwt-eddsa+hs256-legacy.
docker compose ... logs mq-server | grep 'mq-server listening'
```

After that, go through contract §3: enroll, create a grant, get a credential,
read `/history` at `MANDERQUEUE_PUBLIC_URL`.

## 5. Changes the slot manager and compose file would need

None of these are made here.

1. **Pass the new variables through compose.**
   - `mq-server` and `mq-worker`: add `MQ_JWT_JWKS_FILE` and
     `MQ_JWT_JWKS_RELOAD_SECS`.
   - `mq-worker` also needs `MQ_DELIVERY_JWT_SECRET`, which is missing today,
     so the current worker panics at start.
   - `backend-api`: `MQ_ISSUER_SIGNING_KEY_FILE`, `MQ_ISSUER_SIGNING_KID`,
     `MQ_ISSUER_ADDITIONAL_JWKS`, `MANDERQUEUE_PUBLIC_URL`,
     `MQ_DELIVERY_JWT_SECRET`, `WORKSHOP_BACKEND_ORIGIN`,
     `WORKSHOP_BACKEND_ID` and `WORKSHOP_PROFILE_ID`. The current
     `x-mq-client-env` anchor carries only `MANDERQUEUE_HTTP_URL`, `MQ_AUTH`,
     `MQ_JWT_SECRET` and `MQ_BRIDGE_BASE_URL`.
2. **Mount the key files read-only.** Mount `<kid>.jwks.json` into `mq-server`
   and `mq-worker`, and `<kid>.pem` into `backend-api` only. The private key
   must not be mounted into MQ, SMR or Intern containers.
3. **Materialize per-slot values in `compose.env`.**
   - Generate the keypair and delivery secret once per slot, under the slot
     state directory, at 0600.
   - Write `MQ_ISSUER_SIGNING_KID`, the file paths and
     `MANDERQUEUE_PUBLIC_URL=http://127.0.0.1:${MQ_PORT}` from the allocated
     port.
   - Write stable `WORKSHOP_BACKEND_ID` and `WORKSHOP_PROFILE_ID` UUIDs.
   - Record fingerprints, not values, as it already does for `MQ_JWT_SECRET`.
   - Do not route the private key through `compose.env` inline. Pass file
     paths.
4. **Provide an `https` origin for the desktop identity** (§3 option a), or
   get option (b) reviewed and landed in the backend.
5. **Build `LOCAL_MQ_SERVER_IMAGE` from an MQ commit that has both grant
   migrations.** The slot manager pins `manderqueue-manderqueue:latest`.

## 6. Rotation on a slot

1. Generate `k2` with the script.
2. Make MQ's JWKS file contain both keys. Merge the public key sets into a new
   0600 file and atomically replace the mounted file:
   `jq -s '{keys: map(.keys[]) }' k1.jwks.json k2.jwks.json`.
   MQ reloads it within `MQ_JWT_JWKS_RELOAD_SECS`.
3. Switch the backend to `k2` (`MQ_ISSUER_SIGNING_KEY_FILE`,
   `MQ_ISSUER_SIGNING_KID`), with `MQ_ISSUER_ADDITIONAL_JWKS` set to the
   contents of `k1.jwks.json` during the overlap.
4. Wait at least 300 s, then replace MQ's JWKS file with `k2` only, and unset
   `MQ_ISSUER_ADDITIONAL_JWKS`. Credentials under `k1` now refuse.
