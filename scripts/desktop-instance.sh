#!/usr/bin/env bash
# Isolated named Synth Desktop development instances.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMAND="${1:-dev}"
NAME="${2:-${SYNTH_DESKTOP_INSTANCE:-codex}}"

usage() {
  cat <<'EOF'
Usage: ./scripts/desktop-instance.sh <command> [name]

  dev [name]       Run an isolated foreground Tauri/Vite development instance
  status [name]    Show the exact process and instance paths
  stop [name]      Stop only the named instance
  clean [name]     Stop and move the named instance data to Trash
  print [name]     Print the resolved instance contract without launching

Names must match [a-z][a-z0-9-]{0,31}. The default name is "codex".
EOF
}

if [[ ! "$NAME" =~ ^[a-z][a-z0-9-]{0,31}$ ]]; then
  echo "[desktop:$NAME] invalid instance name; expected [a-z][a-z0-9-]{0,31}" >&2
  exit 2
fi

INSTANCE_ROOT="${SYNTH_DESKTOP_INSTANCES_ROOT:-$HOME/.synth-desktop/instances}/$NAME"
DATA_ROOT="$INSTANCE_ROOT/data"
WORKSPACE="$INSTANCE_ROOT/workspace"
GENERATED_ROOT="$INSTANCE_ROOT/generated"
TARGET_ROOT="$INSTANCE_ROOT/build/target"
CONFIG="$GENERATED_ROOT/tauri.instance.json"
MANIFEST="$INSTANCE_ROOT/instance.json"
ICON_PNG="$GENERATED_ROOT/icon.png"
ICON_ICNS="$GENERATED_ROOT/icon.icns"
EXE="$TARGET_ROOT/debug/synth-desktop"
APP_TITLE="Synth Desktop · $NAME"
BUNDLE_ID="com.synth.desktop.dev.$NAME"
CHECKSUM="$(printf '%s' "$NAME" | cksum | awk '{print $1}')"
VITE_PORT=$((14200 + CHECKSUM % 1000))
# Every instance owns a stable Laguna port derived from its name. Instances
# share one models directory of read-only weights and nothing else.
LAGUNA_PORT=$((17300 + CHECKSUM % 600))
SOURCE_REVISION="$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || printf 'unknown')"
if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
  SOURCE_REVISION="$SOURCE_REVISION-dirty.$(git -C "$ROOT" diff --no-ext-diff --binary HEAD | shasum -a 256 | awk '{print substr($1,1,12)}')"
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
  ps -axo pid=,args= | awk -v exe="$EXE" '
    {
      pid=$1
      sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "", $0)
      if ($0 == exe) print pid "\t" $0
    }
  '
}

write_contract() {
	local old_runtime="" manifest_tmp="$MANIFEST.tmp"
  mkdir -p "$DATA_ROOT" "$WORKSPACE" "$GENERATED_ROOT" "$TARGET_ROOT"
  chmod 700 "$INSTANCE_ROOT" "$DATA_ROOT" "$WORKSPACE"

  if [[ ! -e "$DATA_ROOT/config.toml" && -f "$HOME/.synth-desktop/config.toml" ]]; then
    cp "$HOME/.synth-desktop/config.toml" "$DATA_ROOT/config.toml"
  fi
  if [[ ! -e "$DATA_ROOT/.env" && -f "$HOME/.synth-desktop/.env" ]]; then
    cp "$HOME/.synth-desktop/.env" "$DATA_ROOT/.env"
    chmod 600 "$DATA_ROOT/.env"
  fi

  python3 "$ROOT/scripts/generate-desktop-instance-icon.py" \
    --source "$ROOT/apps/synth_desktop/resources/icon.png" \
    --png "$ICON_PNG" \
    --icns "$ICON_ICNS" \
    --label "$ICON_LABEL"

  cat >"$CONFIG" <<EOF
{
  "productName": "$APP_TITLE",
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
    "icon": ["$ICON_PNG", "$ICON_ICNS"]
  }
}
EOF

  if [[ -f "$MANIFEST" ]]; then
    old_runtime="$(jq -c '.runtime // empty' "$MANIFEST" 2>/dev/null || true)"
  fi
  cat >"$manifest_tmp" <<EOF
{
  "schemaVersion": "synth.desktop-instance.v1",
  "mode": "development",
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
    current="$current-dirty.$(git -C "$ROOT" diff --no-ext-diff --binary HEAD | shasum -a 256 | awk '{print substr($1,1,12)}')"
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
    echo "[desktop:$NAME] ERROR Laguna port $SYNTH_LAGUNA_PORT is held by pid $port_holder:" >&2
    ps -p "$port_holder" -o command= >&2 || true
    echo "[desktop:$NAME] stop that process, or set SYNTH_LAGUNA_PORT to a free port for this instance" >&2
    exit 1
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
  export CARGO_TARGET_DIR="$TARGET_ROOT"

  # Debug/eval instances must carry the same Trace V5 format authority used by
  # the runner. Provision and probe it before compilation or any rollout work.
  # trace_ingest.rs resolves this exact target-relative location.
  "$ROOT/scripts/prepare-synth-trace-cli.sh" \
    "$TARGET_ROOT/Resources/bin/synth-trace"
  export SYNTH_TRACE_CLI="$TARGET_ROOT/Resources/bin/synth-trace"

  # The adapter prebuild compiles the shared desktop library and therefore
  # runs Tauri code generation too. Give it the same overlay as `tauri dev`;
  # otherwise Cargo can cache the canonical bundle identifier and the named
  # process is mistaken for a second canonical app by the single-instance
  # plugin.
  export TAURI_CONFIG
  TAURI_CONFIG="$(<"$CONFIG")"

  echo "[desktop:$NAME] building embedded-agent MCP adapters"
  cargo build \
    --manifest-path "$ROOT/apps/synth_desktop/src-tauri/Cargo.toml" \
    --bin synth-containers-mcp \
    --bin synth-visuals-mcp \
    --bin synth-optimizers-mcp

  revalidate_provenance "post-build" "$pre_build_revision"
  export SYNTH_DESKTOP_SOURCE_REVISION="$SOURCE_REVISION"

  echo "[desktop:$NAME] launching $APP_TITLE"
  echo "[desktop:$NAME] data=$DATA_ROOT vite=$VITE_PORT laguna=$SYNTH_LAGUNA_BASE_URL home=$laguna_home"
  echo "[desktop:$NAME] provenance $SOURCE_REVISION digest=$(executable_digest)"
  cd "$ROOT/apps/synth_desktop"
  exec npx tauri dev --config "$CONFIG"
}

clean_instance() {
  stop_instance
  [[ "$INSTANCE_ROOT" == "${SYNTH_DESKTOP_INSTANCES_ROOT:-$HOME/.synth-desktop/instances}/$NAME" ]] || {
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
  dev) dev_instance ;;
  status) status_instance ;;
  stop) stop_instance ;;
  clean) clean_instance ;;
  print) print_contract ;;
  *) usage; exit 2 ;;
esac
