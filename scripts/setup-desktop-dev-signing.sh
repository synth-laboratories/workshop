#!/usr/bin/env bash
# Create the stable local signing identity used by named CUA Workshop builds.
#
# Ad-hoc signatures use the executable CDHash as their designated requirement,
# so macOS Keychain treats every rebuild as a new application. A persistent
# certificate makes the requirement stable without adding a developer secret to
# the repository or granting broad access to the login keychain.
set -euo pipefail

# A caller-named identity (for example an "Apple Development: …" certificate
# already in the login keychain) takes precedence over the self-signed mint.
# In that mode there is no dedicated keychain, so nothing is printed on
# stdout and the caller signs against the default search list.
if [[ -n "${SYNTH_DESKTOP_SIGNING_IDENTITY:-}" ]]; then
  if security find-identity -v -p codesigning 2>/dev/null \
    | grep -F "\"$SYNTH_DESKTOP_SIGNING_IDENTITY\"" >/dev/null; then
    echo "[desktop-signing] using existing identity: $SYNTH_DESKTOP_SIGNING_IDENTITY" >&2
    exit 0
  fi
  echo "[desktop-signing] identity not found in keychain search list: $SYNTH_DESKTOP_SIGNING_IDENTITY" >&2
  exit 1
fi

SIGNING_ROOT="${SYNTH_DESKTOP_DEV_SIGNING_ROOT:-$HOME/.synth-desktop/dev-signing}"
KEYCHAIN="$SIGNING_ROOT/synth-workshop-dev.keychain-db"
PASSWORD_FILE="$SIGNING_ROOT/keychain-password"
IDENTITY="${SYNTH_DESKTOP_DEV_SIGNING_IDENTITY:-Synth Workshop Development}"
CERTIFICATE_FILE="$SIGNING_ROOT/synth-workshop-dev-certificate.pem"

mkdir -p "$SIGNING_ROOT"
chmod 700 "$SIGNING_ROOT"

if [[ ! -f "$PASSWORD_FILE" ]]; then
  openssl rand -hex 32 >"$PASSWORD_FILE"
  chmod 600 "$PASSWORD_FILE"
fi
KEYCHAIN_PASSWORD="$(<"$PASSWORD_FILE")"

KEYCHAIN_CREATED=false
if [[ ! -f "$KEYCHAIN" ]]; then
  security create-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"
  # Keychain lifetime is bootstrap configuration. Reapplying it on every
  # rebuild invokes SecKeychainSetSettings and can surface a macOS password
  # dialog even though this dedicated keychain already has its own password.
  security set-keychain-settings -lut 21600 "$KEYCHAIN"
  KEYCHAIN_CREATED=true
fi
security unlock-keychain -p "$KEYCHAIN_PASSWORD" "$KEYCHAIN"

IDENTITY_IMPORTED=false
if ! security find-certificate -c "$IDENTITY" "$KEYCHAIN" >/dev/null 2>&1; then
  TEMP_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/synth-workshop-signing.XXXXXX")"
  trap 'rm -rf "$TEMP_ROOT"' EXIT
  openssl req -new -newkey rsa:2048 -x509 -sha256 -days 3650 -nodes \
    -keyout "$TEMP_ROOT/private-key.pem" \
    -out "$TEMP_ROOT/certificate.pem" \
    -subj "/CN=$IDENTITY/O=Synth Workshop Development/" \
    -addext "keyUsage=critical,digitalSignature" \
    -addext "extendedKeyUsage=codeSigning" 2>/dev/null
  # Apple's `security import` does not accept OpenSSL 3's default PKCS#12
  # protection algorithms on all supported macOS releases. `-legacy` changes
  # only the temporary transport envelope used for this local import.
  openssl pkcs12 -export -legacy \
    -inkey "$TEMP_ROOT/private-key.pem" \
    -in "$TEMP_ROOT/certificate.pem" \
    -name "$IDENTITY" \
    -out "$TEMP_ROOT/identity.p12" \
    -passout "pass:$KEYCHAIN_PASSWORD" 2>/dev/null
  security import "$TEMP_ROOT/identity.p12" \
    -k "$KEYCHAIN" \
    -P "$KEYCHAIN_PASSWORD" \
    -T /usr/bin/codesign >/dev/null
  IDENTITY_IMPORTED=true
  cp "$TEMP_ROOT/certificate.pem" "$CERTIFICATE_FILE"
  chmod 600 "$CERTIFICATE_FILE"
fi

if [[ ! -f "$CERTIFICATE_FILE" ]]; then
  security find-certificate -c "$IDENTITY" -p "$KEYCHAIN" >"$CERTIFICATE_FILE"
  chmod 600 "$CERTIFICATE_FILE"
fi

if ! security find-identity -v -p codesigning "$KEYCHAIN" 2>/dev/null | grep -F "\"$IDENTITY\"" >/dev/null; then
  # This is the only one-time macOS authorization in the workflow. It trusts
  # one certificate whose private key remains in the dedicated keychain; it
  # does not grant Workshop access to unrelated login-keychain items.
  echo "[desktop-signing] One-time macOS authorization: trust the local Workshop development signer." >&2
  security add-trusted-cert \
    -r trustRoot \
    -k "$KEYCHAIN" \
    "$CERTIFICATE_FILE"
fi

if [[ "$IDENTITY_IMPORTED" == "true" || "$KEYCHAIN_CREATED" == "true" ]]; then
  security set-key-partition-list \
    -S apple-tool:,apple: \
    -s \
    -k "$KEYCHAIN_PASSWORD" \
    "$KEYCHAIN" >/dev/null
fi

# `codesign --keychain` narrows certificate lookup but does not make a custom
# keychain part of trust evaluation on every macOS release. Add this one
# dedicated keychain to the user search list while preserving every existing
# entry and the existing default keychain.
if ! security list-keychains -d user | grep -F "\"$KEYCHAIN\"" >/dev/null; then
  EXISTING_KEYCHAINS=()
  while IFS= read -r line; do
    line="${line#*\"}"
    line="${line%\"*}"
    [[ -n "$line" ]] && EXISTING_KEYCHAINS+=("$line")
  done < <(security list-keychains -d user)
  security list-keychains -d user -s "$KEYCHAIN" "${EXISTING_KEYCHAINS[@]}"
fi

security find-identity -v -p codesigning "$KEYCHAIN" | grep -F "\"$IDENTITY\"" >/dev/null
printf '%s\n' "$KEYCHAIN"
