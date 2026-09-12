#!/usr/bin/env bash
# Generate an Ed25519 MQ grant-signing keypair into files.
#
#   scripts/gen-grant-signing-key.sh <out-dir> [kid]
#
# Writes (all mode 0600, directory 0700, never overwrites):
#   <out-dir>/<kid>.pem          PKCS#8 private key  -> backend MQ_ISSUER_SIGNING_KEY_FILE
#   <out-dir>/<kid>.jwks.json    public JWK set      -> MQ MQ_JWT_JWKS_FILE
#   <out-dir>/<kid>.env          non-secret settings (kid and file names)
#
# Prints only file paths and the kid. Never prints key material.
# See docs/LOCAL_SLOT_GRANTS_SETUP.md and docs/WORKSHOP_GRANT_CONTRACT.md §9.
set -euo pipefail
umask 077

out_dir=${1:?usage: gen-grant-signing-key.sh <out-dir> [kid]}
kid=${2:-mq-grants-$(date -u +%Y%m%dT%H%M%SZ)}
if [[ ! "$kid" =~ ^[A-Za-z0-9._-]{1,64}$ ]]; then
  echo "kid must match [A-Za-z0-9._-]{1,64}" >&2
  exit 2
fi
if ! openssl genpkey -algorithm ed25519 -out /dev/null >/dev/null 2>&1; then
  echo "this openssl cannot generate Ed25519 keys (need OpenSSL 1.1.1+, not LibreSSL)" >&2
  exit 3
fi

mkdir -p "$out_dir"
chmod 700 "$out_dir"
private="$out_dir/$kid.pem"
jwks="$out_dir/$kid.jwks.json"
envfile="$out_dir/$kid.env"
for path in "$private" "$jwks" "$envfile"; do
  if [[ -e "$path" ]]; then
    echo "refusing to overwrite $path" >&2
    exit 4
  fi
done

openssl genpkey -algorithm ed25519 -out "$private" 2>/dev/null
chmod 600 "$private"
# The DER SubjectPublicKeyInfo of an Ed25519 key ends with the raw 32-byte key.
x=$(openssl pkey -in "$private" -pubout -outform DER 2>/dev/null | tail -c 32 | base64 | tr '+/' '-_' | tr -d '=\n')
if [[ ${#x} -ne 43 ]]; then
  rm -f "$private"
  echo "failed to derive the public key" >&2
  exit 5
fi
printf '{"keys":[{"kty":"OKP","crv":"Ed25519","x":"%s","kid":"%s","alg":"EdDSA","use":"sig"}]}\n' "$x" "$kid" >"$jwks"
chmod 600 "$jwks"
printf 'MQ_ISSUER_SIGNING_KID=%s\n# private key file: %s.pem  (backend only)\n# public JWKS file: %s.jwks.json  (MQ server and worker)\n' \
  "$kid" "$kid" "$kid" >"$envfile"
chmod 600 "$envfile"

echo "kid:          $kid"
echo "private key:  $private (0600, backend only)"
echo "public JWKS:  $jwks (0600)"
echo "settings:     $envfile (0600, no secrets)"
