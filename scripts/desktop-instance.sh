#!/usr/bin/env bash
# Isolated named Synth Desktop development instances.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMAND="${1:-dev}"
NAME="${2:-${SYNTH_DESKTOP_INSTANCE:-codex}}"
RELEASE_LINE="${SYNTH_DESKTOP_RELEASE_LINE:-v0.3}"
APP_VERSION="${SYNTH_DESKTOP_APP_VERSION:-0.3.0}"

if [[ "$RELEASE_LINE" != "v0.3" ]]; then
  echo "[desktop:$NAME] invalid release line; this branch only builds v0.3 instances" >&2
  exit 2
fi
RELEASE_SLUG="v03"

usage() {
  cat <<'EOF'
Usage: ./scripts/desktop-instance.sh <command> [name]

  dev [name]       Run an isolated foreground Tauri/Vite development instance
  cua [name]       Build and run a named debug .app for Computer Use
  cua-build [name] Build and sign the named debug .app without launching it
  cua-run [name]   Run the existing signed CUA app without rebuilding
  status [name]    Show the exact process and instance paths
  stage [name]     Stage protected-folder-free runtime inputs without launching
  stop [name]      Stop only the named instance
  clean [name]     Stop and move the named instance data to Trash
  print [name]     Print the resolved instance contract without launching

Names must match [a-z][a-z0-9-]{0,31}. The default name is "codex".

Optimizer services use the immutable installed plugin runtime by default.
Set SYNTH_OPTIMIZER_USE_LOCAL_SOURCE=1 only when intentionally testing a
reviewed local synth-optimizers checkout.
EOF
}

if [[ ! "$NAME" =~ ^[a-z][a-z0-9-]{0,31}$ ]]; then
  echo "[desktop:$NAME] invalid instance name; expected [a-z][a-z0-9-]{0,31}" >&2
  exit 2
fi

INSTANCE_ROOT="${SYNTH_DESKTOP_INSTANCES_ROOT:-$HOME/.synth-desktop/instances}/$RELEASE_SLUG/$NAME"
DATA_ROOT="$INSTANCE_ROOT/data"
WORKSPACE="$INSTANCE_ROOT/workspace"
GENERATED_ROOT="$INSTANCE_ROOT/generated"
TARGET_ROOT="$INSTANCE_ROOT/build/target"
CONFIG="$GENERATED_ROOT/tauri.instance.json"
MANIFEST="$INSTANCE_ROOT/instance.json"
ICON_PNG="$GENERATED_ROOT/icon.png"
ICON_ICNS="$GENERATED_ROOT/icon.icns"
EXE="$TARGET_ROOT/debug/synth-desktop"
APP_TITLE="Synth Workshop $RELEASE_LINE · $NAME"
CUA_EXE="$TARGET_ROOT/debug/bundle/macos/$APP_TITLE.app/Contents/MacOS/synth-desktop"
BUNDLE_ID="com.synth.desktop.$RELEASE_SLUG.dev.$NAME"
CHECKSUM="$(printf '%s' "$NAME" | cksum | awk '{print $1}')"
VITE_PORT=$((14200 + CHECKSUM % 1000))
# Every instance owns a stable Laguna port derived from its name. Instances
# share one models directory of read-only weights and nothing else.
LAGUNA_PORT=$((17300 + CHECKSUM % 600))
SOURCE_REVISION="$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || printf 'unknown')"
if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
  SOURCE_REVISION="$SOURCE_REVISION-dirty"
fi

case "$NAME" in
  alpha) ICON_LABEL="1" ;;
  beta) ICON_LABEL="2" ;;
  gamma) ICON_LABEL="3" ;;
  delta) ICON_LABEL="4" ;;
  epsilon) ICON_LABEL="5" ;;
  test-[1-5]) ICON_LABEL="${NAME#test-}" ;;
  *) ICON_LABEL="$(printf '%s' "$NAME" | cut -c1 | tr '[:lower:]' '[:upper:]')" ;;
esac

instance_processes() {
  ps -axo pid=,args= | awk -v exe="$EXE" -v cua_exe="$CUA_EXE" '
    {
      pid=$1
      sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "", $0)
      if ($0 == exe || $0 == cua_exe) print pid "\t" $0
    }
  '
}

# Named Workshop instances are disposable test clients. Refresh their provider
# credentials from the developer's private machine profile on every launch so
# CUA/eval runs do not begin in a misleading signed-out state. Only the three
# allowlisted values are copied; instance routing and other settings remain
# isolated. The destination is private and is never printed.
stage_test_credentials() {
  local source_env="${SYNTH_DESKTOP_TEST_CREDENTIALS_FILE:-$HOME/.synth-desktop/.env}"
  local destination_env="$DATA_ROOT/.env"
  python3 - "$source_env" "$destination_env" <<'PY'
import os
import re
import sys
from pathlib import Path

source, destination = map(Path, sys.argv[1:])
allowed = ("SYNTH_API_KEY", "OPENROUTER_API_KEY")

def parse(path):
    values = {}
    if not path.is_file():
        return values
    for raw in path.read_text().splitlines():
        line = raw.strip()
        if line.startswith("export "):
            line = line[7:].lstrip()
        if "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        if key in allowed:
            values[key] = value.strip().strip("'\"")
    return values

seed = parse(source)
for key in allowed:
    if os.environ.get(key, "").strip():
        seed[key] = os.environ[key].strip()

existing = destination.read_text().splitlines() if destination.is_file() else []
kept = [line for line in existing if not re.match(r"^\s*(?:export\s+)?(?:SYNTH_API_KEY|OPENROUTER_API_KEY)\s*=", line)]
for key in allowed:
    value = seed.get(key)
    if value:
        escaped = value.replace("'", "'\"'\"'")
        kept.append(f"{key}='{escaped}'")

destination.parent.mkdir(parents=True, exist_ok=True)
temporary = destination.with_suffix(".env.tmp")
temporary.write_text("\n".join(kept).rstrip() + "\n")
temporary.chmod(0o600)
temporary.replace(destination)
PY
}

write_contract() {
	local old_runtime="" manifest_tmp="$MANIFEST.tmp"
  mkdir -p "$DATA_ROOT" "$WORKSPACE" "$GENERATED_ROOT" "$TARGET_ROOT"
  chmod 700 "$INSTANCE_ROOT" "$DATA_ROOT" "$WORKSPACE"

  if [[ ! -e "$DATA_ROOT/config.toml" ]]; then
    if [[ "${SYNTH_DESKTOP_SEED_GLOBAL_CONFIG:-0}" == "1" && -f "$HOME/.synth-desktop/config.toml" ]]; then
      cp "$HOME/.synth-desktop/config.toml" "$DATA_ROOT/config.toml"
    else
      local profile="${SYNTH_INTERN_PROFILE:-local-slot1}"
      local backend_url="${SYNTH_BACKEND_URL:-http://127.0.0.1:41109}"
      cat >"$DATA_ROOT/config.toml" <<EOF
[intern]
profile = "$profile"
env_file = "$DATA_ROOT/.env"
api_key_env = "SYNTH_API_KEY"

[intern.endpoints]
$profile = "$backend_url"
EOF
    fi
  fi
  if [[ ! -e "$DATA_ROOT/.env" ]]; then
    if [[ "${SYNTH_DESKTOP_SEED_GLOBAL_CONFIG:-0}" == "1" && -f "$HOME/.synth-desktop/.env" ]]; then
      cp "$HOME/.synth-desktop/.env" "$DATA_ROOT/.env"
      chmod 600 "$DATA_ROOT/.env"
    else
      : >"$DATA_ROOT/.env"
      chmod 600 "$DATA_ROOT/.env"
    fi
  fi
  stage_test_credentials

  if [[ ! -f "$ICON_PNG" || ! -f "$ICON_ICNS" ]]; then
    python3 "$ROOT/scripts/generate-desktop-instance-icon.py" \
      --source "$ROOT/apps/synth_desktop/resources/icon.png" \
      --png "$ICON_PNG" \
      --icns "$ICON_ICNS" \
      --release-label "$RELEASE_LINE" \
      --instance-label "$ICON_LABEL"
  fi

  cat >"$CONFIG.tmp" <<EOF
{
  "productName": "$APP_TITLE",
  "version": "$APP_VERSION",
  "identifier": "$BUNDLE_ID",
  "build": {
    "beforeDevCommand": "npm run frontend:dev -- --port $VITE_PORT --strictPort",
    "devUrl": "http://127.0.0.1:$VITE_PORT"
  },
  "app": {
    "windows": [{
      "label": "main",
      "title": "$APP_TITLE",
      "width": 1280,
      "height": 840,
      "minWidth": 960,
      "minHeight": 640,
      "visible": false,
      "backgroundColor": "#f3f5f8",
      "titleBarStyle": "Overlay",
      "hiddenTitle": true,
      "trafficLightPosition": { "x": 20, "y": 22 }
    }]
  },
  "bundle": {
    "targets": ["app"],
    "icon": ["$ICON_PNG", "$ICON_ICNS"],
    "macOS": {
      "minimumSystemVersion": "14.0"
    }
  }
}
EOF
  if [[ -f "$CONFIG" ]] && cmp -s "$CONFIG.tmp" "$CONFIG"; then
    rm "$CONFIG.tmp"
  else
    mv "$CONFIG.tmp" "$CONFIG"
  fi

  if [[ -f "$MANIFEST" ]]; then
    old_runtime="$(jq -c '.runtime // empty' "$MANIFEST" 2>/dev/null || true)"
  fi
  cat >"$manifest_tmp" <<EOF
{
  "schemaVersion": "synth.desktop-instance.v1",
  "mode": "development",
  "product": "workshop",
  "releaseLine": "$RELEASE_LINE",
  "appVersion": "$APP_VERSION",
  "name": "$NAME",
  "displayName": "$APP_TITLE",
  "bundleId": "$BUNDLE_ID",
  "iconLabel": "$ICON_LABEL",
  "icon": "$ICON_PNG",
  "dataRoot": "$DATA_ROOT",
  "workspace": "$WORKSPACE",
  "cargoTargetDir": "$TARGET_ROOT",
  "executable": "$EXE",
  "sourceRoot": "$ROOT",
  "sourceRevision": "$SOURCE_REVISION",
  "viteUrl": "http://127.0.0.1:$VITE_PORT",
  "config": "$CONFIG",
  "hotReload": {
    "renderer": true,
    "rust": true,
    "viteUrl": "http://127.0.0.1:$VITE_PORT"
  }
}
EOF
  if [[ -n "$old_runtime" ]]; then
    jq --argjson runtime "$old_runtime" '.runtime = $runtime' "$manifest_tmp" >"$manifest_tmp.merged"
    mv "$manifest_tmp.merged" "$manifest_tmp"
  fi
  mv "$manifest_tmp" "$MANIFEST"
}

executable_digest() {
  if [[ -f "$EXE" ]]; then
    shasum -a 256 "$EXE" | awk '{print "sha256:" $1}'
  else
    printf ''
  fi
}

# Capture rev+dirty before a build, revalidate after, and record the executable
# digest so eval manifests can bind app provenance without stale pointers.
revalidate_provenance() {
  local phase="${1:-post-build}" expected="${2:-$SOURCE_REVISION}"
  local current digest manifest_tmp="$MANIFEST.provenance.tmp"
  current="$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || printf 'unknown')"
  if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
    current="$current-dirty"
  fi
  if [[ "$current" != "$expected" ]]; then
    echo "[desktop:$NAME] ERROR provenance drift ($phase): expected $expected got $current" >&2
    return 1
  fi
  SOURCE_REVISION="$current"
  digest="$(executable_digest)"
  [[ -f "$MANIFEST" ]] || write_contract
  jq \
    --arg sourceRevision "$SOURCE_REVISION" \
    --arg executable "$EXE" \
    --arg executableDigest "$digest" \
    --arg validatedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    --arg phase "$phase" \
    '.provenance = ((.provenance // {}) + {
      sourceRevision: $sourceRevision,
      executable: $executable,
      executableDigest: (if $executableDigest == "" then null else $executableDigest end),
      validatedAt: $validatedAt,
      phase: $phase
    })
    | .sourceRevision = $sourceRevision
    | .executableDigest = (if $executableDigest == "" then null else $executableDigest end)' \
    "$MANIFEST" >"$manifest_tmp"
  mv "$manifest_tmp" "$MANIFEST"
}

mark_runtime() {
  local status="$1" pid="${2:-}" manifest_tmp="$MANIFEST.runtime.tmp"
  local digest
  [[ -f "$MANIFEST" ]] || write_contract
  digest="$(executable_digest)"
  jq \
    --arg status "$status" \
    --arg pid "$pid" \
    --arg executable "$EXE" \
    --arg executableDigest "$digest" \
    --arg sourceRevision "$SOURCE_REVISION" \
    --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '.runtime = ((.runtime // {}) + {
      status: $status,
      pid: (if $pid == "" then null else ($pid | tonumber) end),
      executable: $executable,
      executableDigest: (if $executableDigest == "" then null else $executableDigest end),
      sourceRevision: $sourceRevision,
      checkedAt: $checkedAt
    })' "$MANIFEST" >"$manifest_tmp"
  mv "$manifest_tmp" "$MANIFEST"
}

print_contract() {
  write_contract
  cat "$MANIFEST"
}

stop_instance() {
  local rows pids
  rows="$(instance_processes)"
  if [[ -z "$rows" ]]; then
    mark_runtime "stopped"
    echo "[desktop:$NAME] stopped"
    return
  fi
  printf '%s\n' "$rows" | sed "s/^/[desktop:$NAME] stopping /"
  pids="$(printf '%s\n' "$rows" | awk '{print $1}')"
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if [[ -z "$(instance_processes)" ]]; then
      mark_runtime "stopped"
      return
    fi
    sleep 0.25
  done
  echo "[desktop:$NAME] process did not stop cleanly" >&2
  return 1
}

status_instance() {
  local rows process_count pid
  write_contract
  rows="$(instance_processes)"
  if [[ -z "$rows" ]]; then
    mark_runtime "stopped"
    echo "[desktop:$NAME] stopped"
  else
    process_count="$(printf '%s\n' "$rows" | wc -l | tr -d ' ')"
    if [[ "$process_count" -ne 1 ]]; then
      printf '%s\n' "$rows" | sed "s/^/[desktop:$NAME] ERROR duplicate /" >&2
      return 1
    fi
    pid="$(printf '%s\n' "$rows" | awk 'NR == 1 {print $1}')"
    mark_runtime "running" "$pid"
    printf '%s\n' "$rows" | sed "s/^/[desktop:$NAME] running /"
  fi
  echo "[desktop:$NAME] data $DATA_ROOT"
  echo "[desktop:$NAME] workspace $WORKSPACE"
  echo "[desktop:$NAME] vite http://127.0.0.1:$VITE_PORT"
  echo "[desktop:$NAME] laguna http://127.0.0.1:$LAGUNA_PORT"
  echo "[desktop:$NAME] identity $APP_TITLE · badge $ICON_LABEL · $BUNDLE_ID"
  echo "[desktop:$NAME] executable $EXE"
  echo "[desktop:$NAME] manifest $MANIFEST"
}

stage_gepa_runtime() {
  local runtime_root="$INSTANCE_ROOT/runtime/gepa"
  local cookbook_target="$runtime_root/banking77_container"
  local cookbook_source="${SYNTH_BANKING77_GEPA_COOKBOOK_SOURCE:-$(dirname "$ROOT")/synth-cookbooks-public/cookbooks/optimizers/gepa/banking77_container}"
  local optimizer_target="$runtime_root/optimizer-project"
  local optimizer_source="${SYNTH_OPTIMIZER_PROJECT_SOURCE:-$(dirname "$ROOT")/optimizers-g1}"
  local use_local_optimizer="${SYNTH_OPTIMIZER_USE_LOCAL_SOURCE:-0}"
  local secret_target="$DATA_ROOT/banking77-secret.env"
  local secret_source="${SYNTH_BANKING77_SECRET_ENV_SOURCE:-$(dirname "$ROOT")/synth-ai/.env}"

  if [[ ! -f "$cookbook_source/gepa.toml" || ! -f "$cookbook_source/synth_service_app.py" ]]; then
    echo "[desktop:$NAME] ERROR GEPA cookbook source is unavailable: $cookbook_source" >&2
    exit 1
  fi
  mkdir -p "$cookbook_target"
  rsync -a --delete \
    --exclude '.venv' \
    --exclude '__pycache__' \
    --exclude 'runs' \
    "$cookbook_source/" "$cookbook_target/"
  export SYNTH_BANKING77_GEPA_COOKBOOK_ROOT="$cookbook_target"

  if [[ "$use_local_optimizer" == "1" ]]; then
    if [[ ! -f "$optimizer_source/pyproject.toml" || ! -f "$optimizer_source/rust/crates/synth_gepa/Cargo.toml" ]]; then
      echo "[desktop:$NAME] ERROR optimizer project source is unavailable: $optimizer_source" >&2
      exit 1
    fi
    mkdir -p "$optimizer_target"
    rsync -a --delete \
      --exclude '.git' \
      --exclude '.venv' \
      --exclude 'target' \
      --exclude '.out' \
      --exclude '.pytest_cache' \
      --exclude '.ruff_cache' \
      --exclude '__pycache__' \
      "$optimizer_source/" "$optimizer_target/"
    export SYNTH_OPTIMIZER_PROJECT_ROOT="$optimizer_target"
  elif [[ -n "${SYNTH_OPTIMIZER_PROJECT_ROOT:-}" ]]; then
    echo "[desktop:$NAME] using caller-provided optimizer project root: $SYNTH_OPTIMIZER_PROJECT_ROOT"
  else
    echo "[desktop:$NAME] optimizer runtime=immutable installed plugin"
  fi

  # Finder-launched apps do not inherit shell secrets. Stage only the one
  # allowlisted key inside the mode-0700 instance data root so the app never
  # probes protected source folders at runtime.
  if [[ ! -s "$secret_target" && -f "$secret_source" ]]; then
    local secret_tmp="$secret_target.tmp"
    umask 077
    awk '/^[[:space:]]*(export[[:space:]]+)?OPENAI_API_KEY=/{print; exit}' "$secret_source" >"$secret_tmp"
    if [[ -s "$secret_tmp" ]]; then
      mv "$secret_tmp" "$secret_target"
    else
      rm -f "$secret_tmp"
    fi
  fi
  export SYNTH_BANKING77_SECRET_ENV_FILE="$secret_target"
}

stage_instance() {
  write_contract
  stage_gepa_runtime
  echo "[desktop:$NAME] staged GEPA runtime under $INSTANCE_ROOT/runtime/gepa"
  echo "[desktop:$NAME] app runtime requires no Documents-folder paths"
}

dev_instance() {
  write_contract
  if [[ -n "$(instance_processes)" ]]; then
    echo "[desktop:$NAME] already running; use desktop:instance:stop first" >&2
    exit 1
  fi

  # Capture provenance before any compile so a mid-build dirty tree fails closed.
  local pre_build_revision="$SOURCE_REVISION"
  revalidate_provenance "pre-build" "$pre_build_revision"

  # The daemon's data directory holds its api key, pid files, response store,
  # logs, and selected model. Sharing it across instances shares all of those.
  local laguna_home="${SYNTH_LAGUNA_HOME:-$DATA_ROOT/laguna}"
  mkdir -p "$laguna_home"
  export SYNTH_LAGUNA_HOME="$laguna_home"
  export SYNTH_LAGUNA_PORT="${SYNTH_LAGUNA_PORT:-$LAGUNA_PORT}"
  export SYNTH_LAGUNA_BASE_URL="${SYNTH_LAGUNA_BASE_URL:-http://127.0.0.1:$SYNTH_LAGUNA_PORT}"
  if [[ -z "${SYNTH_LAGUNA_API_KEY:-}" && -f "$laguna_home/api_key" ]]; then
    export SYNTH_LAGUNA_API_KEY
    SYNTH_LAGUNA_API_KEY="$(tr -d '\n' <"$laguna_home/api_key")"
  fi

  # Port derivation is a hash, so a collision is unlikely but not impossible,
  # and a shared port is invisible until turns start failing in one instance
  # for reasons that live in another. Fail here, by name, instead.
  local port_holder
  port_holder="$(lsof -nP -iTCP:"$SYNTH_LAGUNA_PORT" -sTCP:LISTEN -t 2>/dev/null | head -1 || true)"
  if [[ -n "$port_holder" ]]; then
    if lsof -p "$port_holder" 2>/dev/null | rg -F "$laguna_home/" >/dev/null; then
      echo "[desktop:$NAME] reusing owned Laguna daemon pid $port_holder"
    else
      echo "[desktop:$NAME] ERROR Laguna port $SYNTH_LAGUNA_PORT is held by pid $port_holder:" >&2
      ps -p "$port_holder" -o command= >&2 || true
      echo "[desktop:$NAME] stop that process, or set SYNTH_LAGUNA_PORT to a free port for this instance" >&2
      exit 1
    fi
  fi

  export SYNTH_DESKTOP_INSTANCE="$NAME"
  export SYNTH_DESKTOP_DATA_ROOT="$DATA_ROOT"
  export SYNTH_DESKTOP_CONFIG="$DATA_ROOT/config.toml"
  export SYNTH_CODEX_HOME="$DATA_ROOT/codex"
  export SYNTH_DESKTOP_WORKSPACE="$WORKSPACE"
  export SYNTH_DESKTOP_APP_NAME="$APP_TITLE"
  export SYNTH_DESKTOP_INSTANCE_MANIFEST="$MANIFEST"
  export SYNTH_DESKTOP_SOURCE_REVISION="$SOURCE_REVISION"
  export SYNTH_DESKTOP_VITE_URL="http://127.0.0.1:$VITE_PORT"
  # Debug instances use the existing Codex file as a seed and never touch
  # Keychain. Refreshed credentials live in one private machine-local cache so
  # rebuilds and differently named instances reuse a still-valid session.
  local shared_oauth_root="${SYNTH_DESKTOP_SHARED_ROOT:-$HOME/.synth-desktop/shared}/oauth"
  mkdir -p "$shared_oauth_root"
  chmod 700 "$shared_oauth_root"
  if [[ -z "${SYNTH_DESKTOP_DEV_OAUTH_FILE:-}" && -f "$HOME/.codex/auth.json" ]]; then
    SYNTH_DESKTOP_DEV_OAUTH_FILE="$HOME/.codex/auth.json"
  fi
  [[ -z "${SYNTH_DESKTOP_DEV_OAUTH_FILE:-}" ]] || export SYNTH_DESKTOP_DEV_OAUTH_FILE
  if [[ -z "${SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE:-}" ]]; then
    SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE="$shared_oauth_root/codex.json"
  fi
  export SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE
  export CARGO_TARGET_DIR="$TARGET_ROOT"
  stage_gepa_runtime

  # Export profile/account-backend routing from the instance TOML. Responses
  # gateway routing is source-owned by Rust and has no launcher override.
  if [[ -z "${SYNTH_INTERN_PROFILE:-}" && -f "$DATA_ROOT/config.toml" ]]; then
    SYNTH_INTERN_PROFILE="$(python3 - <<'PY' "$DATA_ROOT/config.toml"
import sys, tomllib
from pathlib import Path
data = tomllib.loads(Path(sys.argv[1]).read_text())
print((data.get("intern") or {}).get("profile") or "")
PY
)"
    export SYNTH_INTERN_PROFILE
  fi
  if [[ -z "${SYNTH_BACKEND_URL:-}" && -f "$DATA_ROOT/config.toml" ]]; then
    SYNTH_BACKEND_URL="$(python3 - <<'PY' "$DATA_ROOT/config.toml"
import sys, tomllib
from pathlib import Path
data = tomllib.loads(Path(sys.argv[1]).read_text())
intern = data.get("intern") or {}
profile = intern.get("profile") or ""
endpoints = intern.get("endpoints") or {}
print(endpoints.get(profile) or "")
PY
)"
    [[ -n "$SYNTH_BACKEND_URL" ]] && export SYNTH_BACKEND_URL
  fi
  echo "[desktop:$NAME] profile=${SYNTH_INTERN_PROFILE:-} backend=${SYNTH_BACKEND_URL:-} gateway=source-owned"

  if [[ "$COMMAND" == "cua-run" ]]; then
    if [[ ! -x "$CUA_EXE" ]]; then
      echo "[desktop:$NAME] signed CUA app is missing; run cua-build first" >&2
      exit 1
    fi
    codesign --verify --deep --strict "$(dirname "$(dirname "$(dirname "$CUA_EXE")")")"
    echo "[desktop:$NAME] launching existing signed CUA app from $INSTANCE_ROOT"
    cd "$INSTANCE_ROOT"
    exec "$CUA_EXE"
  fi

  # The adapter prebuild compiles the shared desktop library and therefore
  # runs Tauri code generation too. Give it the same overlay as `tauri dev`;
  # otherwise Cargo can cache the canonical bundle identifier and the named
  # process is mistaken for a second canonical app by the single-instance
  # plugin.
  export TAURI_CONFIG
  TAURI_CONFIG="$(<"$CONFIG")"

  local adapters_ready=1 adapter
  for adapter in synth-containers-mcp synth-visuals-mcp synth-optimizers-mcp; do
    [[ -x "$TARGET_ROOT/debug/$adapter" ]] || adapters_ready=0
  done
  if [[ "$adapters_ready" == "0" || "${SYNTH_DESKTOP_REBUILD_ADAPTERS:-0}" == "1" ]]; then
    echo "[desktop:$NAME] building embedded-agent MCP adapters"
    cargo build \
      --manifest-path "$ROOT/apps/synth_desktop/src-tauri/Cargo.toml" \
      --bin synth-containers-mcp \
      --bin synth-visuals-mcp \
      --bin synth-optimizers-mcp
  else
    echo "[desktop:$NAME] reusing embedded-agent MCP adapters (set SYNTH_DESKTOP_REBUILD_ADAPTERS=1 to refresh)"
  fi

  revalidate_provenance "post-build" "$pre_build_revision"
  export SYNTH_DESKTOP_SOURCE_REVISION="$SOURCE_REVISION"

  if [[ "$COMMAND" == "cua-build" ]]; then
    echo "[desktop:$NAME] building $APP_TITLE without launch"
  else
    echo "[desktop:$NAME] launching $APP_TITLE"
  fi
  echo "[desktop:$NAME] data=$DATA_ROOT vite=$VITE_PORT laguna=$SYNTH_LAGUNA_BASE_URL home=$laguna_home"
  echo "[desktop:$NAME] provenance $SOURCE_REVISION digest=$(executable_digest)"
  cd "$ROOT/apps/synth_desktop"
  if [[ "$COMMAND" == "cua" || "$COMMAND" == "cua-build" ]]; then
    # Raw `tauri dev` binaries have no LaunchServices app identity, so macOS
    # accessibility clients cannot address a named instance reliably. A debug
    # bundle preserves the isolated environment and registers the unique ID.
    # Build only the runnable .app. A DMG adds time and has no use in the
    # local CUA loop.
    npx tauri build --debug --bundles app --config "$CONFIG"
    local app_bundle="$CARGO_TARGET_DIR/debug/bundle/macos/$APP_TITLE.app"
    local app_executable="$CUA_EXE"
    if [[ ! -x "$app_executable" ]]; then
      echo "[desktop:$NAME] expected CUA bundle executable missing: $app_executable" >&2
      exit 1
    fi
    # Local test builds must never open a Keychain/password dialog. Ad-hoc
    # signing requires no secret. Stable certificate signing remains an
    # explicit opt-in for workflows that need a persistent CUA identity.
    if [[ "${SYNTH_DESKTOP_USE_DEV_SIGNER:-0}" == "1" ]]; then
      local dev_signing_identity="${SYNTH_DESKTOP_DEV_SIGNING_IDENTITY:-Synth Workshop Development}"
      local dev_signing_keychain
      dev_signing_keychain="$("$ROOT/scripts/setup-desktop-dev-signing.sh")"
      security unlock-keychain \
        -p "$(<"${SYNTH_DESKTOP_DEV_SIGNING_ROOT:-$HOME/.synth-desktop/dev-signing}/keychain-password")" \
        "$dev_signing_keychain"
      codesign --force --deep --sign "$dev_signing_identity" \
        --keychain "$dev_signing_keychain" \
        --identifier "com.synth.desktop.v03.dev.shared" "$app_bundle"
    else
      codesign --force --deep --sign - --identifier "$BUNDLE_ID" "$app_bundle"
    fi
    codesign --verify --deep --strict "$app_bundle"
    echo "[desktop:$NAME] CUA bundle $app_bundle"
    echo "[desktop:$NAME] CUA target $BUNDLE_ID"
    if [[ "$COMMAND" == "cua-build" ]]; then
      echo "[desktop:$NAME] build complete; app was not launched"
      return
    fi
    # Never launch the packaged app with the source checkout as its current
    # directory. On macOS, a cwd under ~/Documents attributes source-tree
    # traversal to the app and triggers an unnecessary Files & Folders prompt.
    # Runtime data and workspaces already live under this isolated instance.
    cd "$INSTANCE_ROOT"
    exec "$app_executable"
  fi
  exec npx tauri dev --config "$CONFIG"
}

clean_instance() {
  stop_instance
  [[ "$INSTANCE_ROOT" == "${SYNTH_DESKTOP_INSTANCES_ROOT:-$HOME/.synth-desktop/instances}/$RELEASE_SLUG/$NAME" ]] || {
    echo "[desktop:$NAME] refusing unresolved cleanup target" >&2
    exit 1
  }
  if [[ ! -e "$INSTANCE_ROOT" ]]; then
    echo "[desktop:$NAME] no instance data"
    return
  fi
  local trash="$HOME/.Trash/Synth Desktop instance $NAME $(date '+%Y%m%d-%H%M%S')"
  mv "$INSTANCE_ROOT" "$trash"
  echo "[desktop:$NAME] moved to $trash (recoverable from Trash)"
}

case "$COMMAND" in
  dev|cua|cua-build|cua-run) dev_instance ;;
  status) status_instance ;;
  stage) stage_instance ;;
  stop) stop_instance ;;
  clean) clean_instance ;;
  print) print_contract ;;
  *) usage; exit 2 ;;
esac
