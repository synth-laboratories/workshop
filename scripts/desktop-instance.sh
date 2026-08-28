#!/usr/bin/env bash
# Isolated named Synth Desktop development instances.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=scripts/mcp-adapters.sh
source "$ROOT/scripts/mcp-adapters.sh"
REPO_SIBLING_ROOT="$(dirname "$ROOT")"
GIT_COMMON_DIR="$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir 2>/dev/null || true)"
if [[ -n "$GIT_COMMON_DIR" ]]; then
  PRIMARY_REPO_SIBLING_ROOT="$(dirname "$(dirname "$GIT_COMMON_DIR")")"
  if [[ -d "$PRIMARY_REPO_SIBLING_ROOT/synth-cookbooks-public" ]]; then
    REPO_SIBLING_ROOT="$PRIMARY_REPO_SIBLING_ROOT"
  fi
fi
COMMAND="dev"
if [[ $# -gt 0 ]]; then
  COMMAND="$1"
  shift
fi
VERBOSE=0
NAME=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --verbose) VERBOSE=1 ;;
    --help|-h)
      # usage is defined below; a second pass after functions would be
      # later. Print here only after NAME is known — defer via flag.
      SHOW_HELP=1
      ;;
    -*)
      echo "[desktop] unknown option: $1" >&2
      exit 2
      ;;
    *)
      if [[ -n "$NAME" ]]; then
        echo "[desktop] unexpected extra argument: $1" >&2
        exit 2
      fi
      NAME="$1"
      ;;
  esac
  shift
done

WORKTREE="$(git -C "$ROOT" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$ROOT")"
WORKTREE_HASH="$(printf '%s' "$WORKTREE" | shasum -a 256 | awk '{print substr($1,1,8)}')"
DEFAULT_NAME="codex-$WORKTREE_HASH"
NAME="${NAME:-$DEFAULT_NAME}"
RELEASE_LINE="${SYNTH_DESKTOP_RELEASE_LINE:-v0.8}"
APP_VERSION="${SYNTH_DESKTOP_APP_VERSION:-0.8.0}"
BOOT_EPOCH="inst_$(uuidgen | tr -d '-' | tr '[:upper:]' '[:lower:]')"
PROCESS_START_TIME="$(ps -p $$ -o lstart= | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"

if [[ "$RELEASE_LINE" != "v0.8" ]]; then
	  echo "[desktop:$NAME] invalid release line; this branch only builds v0.8 instances" >&2
	  exit 2
fi
RELEASE_SLUG="v08"

usage() {
  cat <<EOF
Usage: ./scripts/desktop-instance.sh <command> [name] [--verbose]

  dev [name]       Run an isolated foreground Tauri/Vite development instance
  cua [name]       Build and run a named debug .app for Computer Use
  cua-build [name] Build and sign the named debug .app without launching it
  cua-run [name]   Run the existing signed CUA app without rebuilding
  rebuild-run [name]  Build, bundle, sign, record, verify, launch, wait for health
  assert-identity [name]  Verify the built app's signing identity and record it
  status [name]    Show the exact process and instance paths
                   --verbose also prints the operation-lock owner
  stage [name]     Stage protected-folder-free runtime inputs without launching
  stop [name]      Stop only the named instance
  clean [name]     Stop and move the named instance data to Trash
  print [name]     Print the resolved instance contract without launching

Names must match [a-z][a-z0-9-]{0,31}. The default name is
codex-<worktree-hash> so two checkouts cannot collide without intent.

Optimizer services use the immutable installed plugin runtime by default.
Set SYNTH_OPTIMIZER_USE_LOCAL_SOURCE=1 only when intentionally testing a
reviewed local synth-optimizers checkout.
EOF
}

if [[ "${SHOW_HELP:-0}" == "1" ]]; then
  usage
  exit 0
fi

if [[ ! "$NAME" =~ ^[a-z][a-z0-9-]{0,31}$ ]]; then
  echo "[desktop:$NAME] invalid instance name; expected [a-z][a-z0-9-]{0,31}" >&2
  exit 2
fi

INSTANCE_ROOT="${SYNTH_DESKTOP_INSTANCES_ROOT:-$HOME/.synth-desktop/instances}/$RELEASE_SLUG/$NAME"
OPERATION_LOCK="$INSTANCE_ROOT/operation.lock"
DATA_ROOT="$INSTANCE_ROOT/data"
WORKSPACE="$INSTANCE_ROOT/workspace"
GENERATED_ROOT="$INSTANCE_ROOT/generated"
TARGET_ROOT="$INSTANCE_ROOT/build/target"
CONFIG="$GENERATED_ROOT/tauri.instance.json"
# Packaged resources (cookbooks, Computer Use helper, visuals) live in the
# packaging overlay, never in the base tauri.conf.json, so `cargo check` and
# library tests need no staged resources. The overlay merges first; the
# instance overlay adds its own resources on top.
PACKAGE_CONFIG="src-tauri/tauri.package.json"
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

# Legacy cleanup only. Older builds marked children with this variable; current
# builds derive identity from the bundle descriptor and never export it.
instance_env_pids() {
  ps -axwwE -o pid=,command= 2>/dev/null | awk -v name="$NAME" -v self="$$" '
    BEGIN { needle = "SYNTH_WORKSHOP_INSTANCE_ID=" name }
    {
      pid=$1
      if (pid == self) next
      if (pid !~ /^[0-9]+$/) next
      rest = $0
      sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "", rest)
      idx = index(rest, needle)
      if (idx == 0) next
      after = substr(rest, idx + length(needle))
      if (after == "" || substr(after, 1, 1) == " ") print pid
    }
  '
}

# Return only the PID named by this instance's optimizer lease after proving
# that the PID still has the recorded process-start identity. This lets the
# launcher clean up a sidecar even when the desktop process cannot run its
# normal shutdown handler, without relying on broad process-name matching.
optimizer_lease_pid() {
  python3 - "$DATA_ROOT/optimizers/runtime-lease.json" "$NAME" <<'PY'
import json
import subprocess
import sys
from pathlib import Path

path = Path(sys.argv[1])
instance_id = sys.argv[2]
try:
    lease = json.loads(path.read_text())
    pid = int(lease["pid"])
    expected_identity = str(lease["processStartIdentity"]).strip()
except (FileNotFoundError, KeyError, TypeError, ValueError, json.JSONDecodeError):
    raise SystemExit(0)

if lease.get("schemaVersion") != "workshop.optimizer-runtime-lease.v1":
    raise SystemExit(0)
if lease.get("instanceId") != instance_id or pid <= 1 or not expected_identity:
    raise SystemExit(0)

result = subprocess.run(
    ["/bin/ps", "-p", str(pid), "-o", "lstart="],
    check=False,
    capture_output=True,
    text=True,
)
actual_identity = f"ps-lstart:{result.stdout.strip()}"
if result.returncode == 0 and actual_identity == expected_identity:
    print(pid)
PY
}

format_lock_owner() {
  python3 - "$1" <<'PY'
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
if not path.is_file():
    raise SystemExit(1)
try:
    record = json.loads(path.read_text())
except json.JSONDecodeError:
    raise SystemExit(1)
keys = ("instance", "pid", "process_start_time", "worktree", "repo_revision", "operation", "created_at")
parts = ["%s=%s" % (key, record.get(key, "")) for key in keys]
sys.stdout.write("owner " + " ".join(parts) + "\n")
PY
}

operation_lock_helper() {
  local action="$1"
  SYNTH_LOCK_INSTANCE="$NAME" \
  SYNTH_LOCK_PID="$$" \
  SYNTH_LOCK_START="$PROCESS_START_TIME" \
  SYNTH_LOCK_WORKTREE="$WORKTREE" \
  SYNTH_LOCK_REVISION="$SOURCE_REVISION" \
  SYNTH_LOCK_OPERATION="${2:-$COMMAND}" \
  SYNTH_LOCK_CREATED="$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  python3 - "$OPERATION_LOCK" "$action" <<'PY'
import fcntl, json, os, subprocess, sys

lock_path, action = sys.argv[1], sys.argv[2]
keys = ("instance", "pid", "process_start_time", "worktree", "repo_revision", "operation", "created_at")

def start_time(pid):
    try:
        out = subprocess.check_output(
            ["/bin/ps", "-p", str(pid), "-o", "lstart="],
            stderr=subprocess.DEVNULL,
        )
    except (subprocess.CalledProcessError, OSError):
        return None
    text = out.decode().strip()
    return text or None

def alive(pid, recorded):
    current = start_time(pid)
    return current is not None and current == recorded

def owner_line(record):
    return "owner " + " ".join("%s=%s" % (key, record.get(key, "")) for key in keys)

def read_record(fd):
    os.lseek(fd, 0, os.SEEK_SET)
    raw = os.read(fd, 1 << 16).decode("utf-8", "replace").strip()
    if not raw:
        return None
    try:
        return json.loads(raw)
    except json.JSONDecodeError:
        return None

def write_record(fd, record):
    body = (json.dumps(record, indent=2) + "\n").encode()
    os.lseek(fd, 0, os.SEEK_SET)
    os.ftruncate(fd, 0)
    os.write(fd, body)
    os.fsync(fd)

if action == "read":
    if not os.path.isfile(lock_path):
        raise SystemExit(1)
    record = json.loads(open(lock_path).read())
    sys.stdout.write(owner_line(record) + "\n")
    raise SystemExit(0)

fd = os.open(lock_path, os.O_RDWR | os.O_CREAT, 0o644)
try:
    fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    record = read_record(fd) or {}
    sys.stderr.write(owner_line(record) + "\n")
    raise SystemExit(3)

record = read_record(fd)
our_pid = int(os.environ["SYNTH_LOCK_PID"])
if action == "release":
    if record and int(record.get("pid") or 0) == our_pid:
        os.close(fd)
        try:
            os.unlink(lock_path)
        except OSError:
            pass
        raise SystemExit(0)
    os.close(fd)
    raise SystemExit(0)

# acquire
if record:
    pid = record.get("pid")
    start = record.get("process_start_time")
    if pid and start and alive(int(pid), start) and int(pid) != our_pid:
        sys.stderr.write(owner_line(record) + "\n")
        os.close(fd)
        raise SystemExit(3)

new_record = {
    "instance": os.environ["SYNTH_LOCK_INSTANCE"],
    "pid": our_pid,
    "process_start_time": os.environ["SYNTH_LOCK_START"],
    "worktree": os.environ["SYNTH_LOCK_WORKTREE"],
    "repo_revision": os.environ["SYNTH_LOCK_REVISION"],
    "operation": os.environ["SYNTH_LOCK_OPERATION"],
    "created_at": os.environ["SYNTH_LOCK_CREATED"],
}
write_record(fd, new_record)
os.close(fd)
PY
}

acquire_operation_lock() {
  local operation="$1" output status
  mkdir -p "$INSTANCE_ROOT"
  set +e
  output="$(operation_lock_helper acquire "$operation" 2>&1)"
  status=$?
  set -e
  if [[ "$status" -ne 0 ]]; then
    echo "[desktop:$NAME] ERROR instance operation locked" >&2
    if [[ -n "$output" ]]; then
      echo "[desktop:$NAME] $output" >&2
    fi
    exit 1
  fi
  trap release_operation_lock EXIT
}

release_operation_lock() {
  operation_lock_helper release "$COMMAND" >/dev/null 2>&1 || true
}

release_operation_lock_before_exec() {
  trap - EXIT
  release_operation_lock
}

print_operation_lock_status() {
  local owner=""
  if [[ ! -f "$OPERATION_LOCK" ]]; then
    echo "[desktop:$NAME] operation.lock none"
    return
  fi
  set +e
  owner="$(format_lock_owner "$OPERATION_LOCK" 2>/dev/null)"
  set -e
  if [[ -z "$owner" ]]; then
    echo "[desktop:$NAME] operation.lock unreadable"
    return
  fi
  echo "[desktop:$NAME] $owner"
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
	local old_runtime="" old_signing="" old_provenance="" old_executable="" old_executable_digest="" manifest_tmp="$MANIFEST.$$.tmp"
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
  # Durable instance authority for signed debug/CUA bundles. LaunchServices
  # does not inherit the shell launcher's environment.
  if [[ "$COMMAND" == "cua" || "$COMMAND" == "cua-build" || "$COMMAND" == "cua-run" || "$COMMAND" == "rebuild-run" ]]; then
    cat >"$DATA_ROOT/eval-admission.toml" <<'EOF'
[target_admission.local_pinned_digest]
enabled = true
source = "instance_config"
EOF
    chmod 600 "$DATA_ROOT/eval-admission.toml"
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
    "resources": {},
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
    old_signing="$(jq -c '.signing // empty' "$MANIFEST" 2>/dev/null || true)"
    old_provenance="$(jq -c '.provenance // empty' "$MANIFEST" 2>/dev/null || true)"
    old_executable="$(jq -r '.executable // empty' "$MANIFEST" 2>/dev/null || true)"
    old_executable_digest="$(jq -r '.executableDigest // empty' "$MANIFEST" 2>/dev/null || true)"
  fi
  cat >"$manifest_tmp" <<EOF
{
  "schemaVersion": "synth.desktop-instance.v1",
  "mode": "development",
  "product": "workshop",
  "releaseLine": "$RELEASE_LINE",
  "releaseSlug": "$RELEASE_SLUG",
  "appVersion": "$APP_VERSION",
  "name": "$NAME",
  "displayName": "$APP_TITLE",
  "bundleId": "$BUNDLE_ID",
  "iconLabel": "$ICON_LABEL",
  "icon": "$ICON_PNG",
  "instanceRoot": "$INSTANCE_ROOT",
  "dataRoot": "$DATA_ROOT",
  "workspace": "$WORKSPACE",
  "cargoTargetDir": "$TARGET_ROOT",
  "executable": "$EXE",
  "appBundle": "$(dirname "$(dirname "$(dirname "$CUA_EXE")")")",
  "sourceRoot": "$ROOT",
  "sourceRevision": "$SOURCE_REVISION",
  "worktree": "$WORKTREE",
  "worktreeHash": "$WORKTREE_HASH",
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
  if [[ -n "$old_signing" ]]; then
    jq --argjson signing "$old_signing" '.signing = $signing' "$manifest_tmp" >"$manifest_tmp.merged"
    mv "$manifest_tmp.merged" "$manifest_tmp"
  fi
  if [[ -n "$old_provenance" ]]; then
    jq --argjson provenance "$old_provenance" '.provenance = $provenance' "$manifest_tmp" >"$manifest_tmp.merged"
    mv "$manifest_tmp.merged" "$manifest_tmp"
  fi
  if [[ -n "$old_executable_digest" ]]; then
    jq --arg executableDigest "$old_executable_digest" '.executableDigest = $executableDigest' "$manifest_tmp" >"$manifest_tmp.merged"
    mv "$manifest_tmp.merged" "$manifest_tmp"
  fi
  if [[ -n "$old_executable" ]]; then
    jq --arg executable "$old_executable" '.executable = $executable' "$manifest_tmp" >"$manifest_tmp.merged"
    mv "$manifest_tmp.merged" "$manifest_tmp"
  fi
  mv "$manifest_tmp" "$MANIFEST"
}

write_bundle_descriptor() {
  local app_bundle="$1"
  local dest="$app_bundle/Contents/Resources/instance.json"
  mkdir -p "$(dirname "$dest")"
  jq -n \
    --arg schemaVersion "synth.desktop.instance-descriptor.v1" \
    --arg instance_id "$NAME" \
    --arg instance_root "$INSTANCE_ROOT" \
    --arg config_path "$DATA_ROOT/config.toml" \
    --arg data_root "$DATA_ROOT" \
    --arg bundle_id "$BUNDLE_ID" \
    --arg release_line "$RELEASE_LINE" \
    --arg source_revision "$SOURCE_REVISION" \
    --arg generated_at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '{
      schemaVersion: $schemaVersion,
      instance_id: $instance_id,
      instance_root: $instance_root,
      config_path: $config_path,
      data_root: $data_root,
      bundle_id: $bundle_id,
      release_line: $release_line,
      source_revision: $source_revision,
      generated_at: $generated_at
    }' >"$dest.tmp"
  mv "$dest.tmp" "$dest"
}

executable_digest() {
  local executable="${1:-$EXE}"
  if [[ -f "$executable" ]]; then
    shasum -a 256 "$executable" | awk '{print "sha256:" $1}'
  else
    printf ''
  fi
}

bundle_cdhash() {
  /usr/bin/codesign -dvvv "$1" 2>&1 | awk -F= '/^CDHash=/ && !found {print $2; found=1}'
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

record_packaged_provenance() {
  local app_bundle="$1" digest manifest_tmp="$MANIFEST.packaged-provenance.tmp"
  [[ -x "$CUA_EXE" ]] || {
    echo "[desktop:$NAME] ERROR packaged executable is missing: $CUA_EXE" >&2
    return 1
  }
  digest="sha256:$(shasum -a 256 "$CUA_EXE" | awk '{print $1}')"
  jq \
    --arg executable "$CUA_EXE" \
    --arg executableDigest "$digest" \
    --arg bundle "$app_bundle" \
    --arg cdHash "$(bundle_cdhash "$app_bundle")" \
    --arg validatedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '.executable = $executable
    | .executableDigest = $executableDigest
    | .provenance = ((.provenance // {}) + {
        phase: "bundle-signed",
        executable: $executable,
        executableDigest: $executableDigest,
        appBundle: $bundle,
        cdHash: $cdHash,
        validatedAt: $validatedAt
      })' "$MANIFEST" >"$manifest_tmp"
  mv "$manifest_tmp" "$MANIFEST"
}

verify_packaged_provenance() {
  local app_bundle actual_digest recorded_digest actual_cdhash recorded_cdhash recorded_revision phase
  app_bundle="$(dirname "$(dirname "$(dirname "$CUA_EXE")")")"
  [[ -x "$CUA_EXE" ]] || {
    echo "[desktop:$NAME] bundle was not produced by cua-build; run desktop-instance.sh rebuild-run $NAME" >&2
    return 1
  }
  codesign --verify --deep --strict "$app_bundle"
  actual_digest="$(executable_digest "$CUA_EXE")"
  recorded_digest="$(jq -r '.provenance.executableDigest // .executableDigest // empty' "$MANIFEST")"
  actual_cdhash="$(bundle_cdhash "$app_bundle")"
  recorded_cdhash="$(jq -r '.provenance.cdHash // empty' "$MANIFEST")"
  recorded_revision="$(jq -r '.provenance.sourceRevision // empty' "$MANIFEST")"
  phase="$(jq -r '.provenance.phase // empty' "$MANIFEST")"
  [[ "$phase" == "bundle-signed" ]] || {
    echo "[desktop:$NAME] bundle was not produced by cua-build; run desktop-instance.sh rebuild-run $NAME" >&2
    return 1
  }
  [[ "$recorded_revision" == "$SOURCE_REVISION" ]] || {
    echo "[desktop:$NAME] ERROR packaged source revision drift: $recorded_revision != $SOURCE_REVISION" >&2
    return 1
  }
  [[ "$recorded_digest" == "$actual_digest" ]] || {
    echo "[desktop:$NAME] ERROR packaged executable digest drift: $recorded_digest != $actual_digest" >&2
    return 1
  }
  [[ -n "$recorded_cdhash" && "$recorded_cdhash" == "$actual_cdhash" ]] || {
    echo "[desktop:$NAME] ERROR packaged CDHash drift: $recorded_cdhash != $actual_cdhash" >&2
    return 1
  }
}

mark_runtime() {
  local status="$1" pid="${2:-}" manifest_tmp="$MANIFEST.runtime.$$.tmp"
  local digest runtime_executable="$EXE"
  [[ -f "$MANIFEST" ]] || write_contract
  if [[ -n "$pid" ]]; then
    runtime_executable="$(lsof -a -p "$pid" -d txt -Fn 2>/dev/null | sed -n 's/^n//p' | head -1)"
    [[ -f "$runtime_executable" ]] || runtime_executable="$EXE"
  fi
  digest="$(executable_digest "$runtime_executable")"
  jq \
    --arg status "$status" \
    --arg pid "$pid" \
    --arg executable "$runtime_executable" \
    --arg executableDigest "$digest" \
    --arg sourceRevision "$SOURCE_REVISION" \
    --arg bootEpoch "$BOOT_EPOCH" \
    --arg processStartIdentity "$PROCESS_START_TIME" \
    --arg checkedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '.runtime = ((.runtime // {}) + {
      status: $status,
      pid: (if $pid == "" then null else ($pid | tonumber) end),
      executable: $executable,
      executableDigest: (if $executableDigest == "" then null else $executableDigest end),
      sourceRevision: $sourceRevision,
      bootEpoch: $bootEpoch,
      processStartIdentity: $processStartIdentity,
      checkedAt: $checkedAt
    })' "$MANIFEST" >"$manifest_tmp"
  mv "$manifest_tmp" "$MANIFEST"
}

print_contract() {
  write_contract
  cat "$MANIFEST"
}

stop_instance() {
  local rows pids env_pids lease_pid all_pids
  rows="$(instance_processes)"
  env_pids="$(instance_env_pids)"
  lease_pid="$(optimizer_lease_pid)"
  all_pids="$(printf '%s\n%s\n%s\n' "$(printf '%s\n' "$rows" | awk '{print $1}')" "$env_pids" "$lease_pid" | awk 'NF && !seen[$0]++')"
  if [[ -z "$all_pids" ]]; then
    rm -f "$DATA_ROOT/eval-driver.json"
    mark_runtime "stopped"
    echo "[desktop:$NAME] stopped"
    return
  fi
  if [[ -n "$rows" ]]; then
    printf '%s\n' "$rows" | sed "s/^/[desktop:$NAME] stopping /"
  fi
  if [[ -n "$env_pids" ]]; then
    printf '%s\n' "$env_pids" | sed "s/^/[desktop:$NAME] stopping env-pid /"
  fi
  if [[ -n "$lease_pid" ]]; then
    printf '%s\n' "$lease_pid" | sed "s/^/[desktop:$NAME] stopping optimizer lease-pid /"
  fi
  # shellcheck disable=SC2086
  kill $all_pids 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    if [[ -z "$(instance_processes)" && -z "$(instance_env_pids)" && -z "$(optimizer_lease_pid)" ]]; then
      rm -f "$DATA_ROOT/eval-driver.json"
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
  if [[ "$VERBOSE" == "1" ]]; then
    print_operation_lock_status
  fi
}

stage_gepa_runtime() {
  local runtime_root="$INSTANCE_ROOT/runtime/gepa"
  local optimizer_target="$runtime_root/optimizer-project"
  local optimizer_source="${SYNTH_OPTIMIZER_PROJECT_SOURCE:-$REPO_SIBLING_ROOT/optimizers-g1}"
  local use_local_optimizer="${SYNTH_OPTIMIZER_USE_LOCAL_SOURCE:-0}"
  local secret_target="$DATA_ROOT/gepa-secret.env"
  local secret_source="${SYNTH_GEPA_SECRET_ENV_SOURCE:-$REPO_SIBLING_ROOT/synth-ai/.env}"

  unset SYNTH_BANKING77_GEPA_COOKBOOK_ROOT SYNTH_CRAFTAX_GEPA_COOKBOOK_ROOT

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
      --exclude 'temp' \
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
  export SYNTH_GEPA_SECRET_ENV_FILE="$secret_target"
}

# A packaged CUA bundle must not inherit the parent Workshop process's
# environment.  This is more than defense in depth: macOS lets multiple
# development bundles share one login session, and a caller's
# SYNTH_DESKTOP_DATA_ROOT used to make the correctly named bundle attach to
# the caller's database and provider proxy.  Keep this allowlist deliberately
# small. Provider credentials live in the named instance's private .env and
# are loaded by the app, never inherited here.
exec_isolated_cua_bundle() {
  local launch_mode="${1:-foreground}"
  local oauth_file="${SYNTH_DESKTOP_DEV_OAUTH_FILE:-}"
  local oauth_state="${SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE:-}"
  local sft_train_jsonl="${SYNTH_MLX_SFT_TRAIN_JSONL:-}"
  local sft_eval_jsonl="${SYNTH_MLX_SFT_EVAL_JSONL:-}"
  # stage_gepa_runtime rewrites an explicitly reviewed local optimizer source
  # into this instance-owned directory. Preserve only that resolved path across
  # the env -i boundary; passing the caller's source path would defeat packaged
  # isolation, while dropping it silently falls back to the immutable plugin.
  local optimizer_project_root="${SYNTH_OPTIMIZER_PROJECT_ROOT:-}"
  local optimizer_wheel_file="${SYNTH_OPTIMIZER_WHEEL_FILE:-}"
  local mlx_rl_url="${SYNTH_MLX_RL_URL:-}"
  local home_dir="${HOME:?HOME must be set to launch a CUA bundle}"
  local user_name="${USER:-$(id -un)}"
  local logname="${LOGNAME:-$user_name}"
  local temp_dir="${TMPDIR:-/tmp}"

  mark_runtime "launching" "$$"
  release_operation_lock_before_exec
  local isolated_env=(env -i \
    PATH="$PATH" \
    HOME="$home_dir" \
    USER="$user_name" \
    LOGNAME="$logname" \
    TMPDIR="$temp_dir" \
    PWD="$INSTANCE_ROOT" \
    SYNTH_DESKTOP_DATA_ROOT="$DATA_ROOT" \
    SYNTH_DESKTOP_CONFIG="$DATA_ROOT/config.toml" \
    SYNTH_CODEX_HOME="$DATA_ROOT/codex" \
    SYNTH_DESKTOP_WORKSPACE="$WORKSPACE" \
    SYNTH_DESKTOP_SOURCE_REVISION="$SOURCE_REVISION" \
    SYNTH_DESKTOP_VITE_URL="http://127.0.0.1:$VITE_PORT" \
    SYNTH_EVAL_ALLOW_LOCAL_PINNED_TARGETS=1 \
    SYNTH_LAGUNA_HOME="$SYNTH_LAGUNA_HOME" \
    SYNTH_LAGUNA_PORT="$SYNTH_LAGUNA_PORT" \
    SYNTH_LAGUNA_BASE_URL="$SYNTH_LAGUNA_BASE_URL" \
    SYNTH_COMPUTER_USE_PARENT_REQUIREMENT="$SYNTH_COMPUTER_USE_PARENT_REQUIREMENT" \
    SYNTH_DESKTOP_DEV_OAUTH_FILE="$oauth_file" \
    SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE="$oauth_state" \
    SYNTH_MLX_SFT_TRAIN_JSONL="$sft_train_jsonl" \
    SYNTH_MLX_SFT_EVAL_JSONL="$sft_eval_jsonl" \
    SYNTH_OPTIMIZER_PROJECT_ROOT="$optimizer_project_root" \
    SYNTH_OPTIMIZER_WHEEL_FILE="$optimizer_wheel_file" \
    SYNTH_MLX_RL_URL="$mlx_rl_url" \
    "$CUA_EXE")
  if [[ "$launch_mode" == "background" ]]; then
    "${isolated_env[@]}" &
  else
    exec "${isolated_env[@]}"
  fi
}

stage_instance() {
  write_contract
  stage_gepa_runtime
  echo "[desktop:$NAME] staged GEPA runtime under $INSTANCE_ROOT/runtime/gepa"
  echo "[desktop:$NAME] app runtime requires no Documents-folder paths"
}

# Stable certificate signing is the default: an ad-hoc signature's designated
# requirement is the executable CDHash, so every rebuild becomes a new TCC and
# Keychain principal and previously granted permissions silently vanish.
# SYNTH_DESKTOP_USE_DEV_SIGNER=0 opts back into ad-hoc for machines that must
# never run the one-time trust authorization in setup-desktop-dev-signing.sh.
resolve_signing_identity() {
  if [[ "${SYNTH_DESKTOP_USE_DEV_SIGNER:-1}" == "1" ]]; then
    printf '%s' "${SYNTH_DESKTOP_SIGNING_IDENTITY:-${SYNTH_DESKTOP_DEV_SIGNING_IDENTITY:-Synth Workshop Development}}"
  else
    printf '%s' "-"
  fi
}

# Fail closed before a named CUA compile borrows helpers from another
# checkout or signs with a missing identity. Does not notarize or publish.
packaging_preflight() {
  local helper="$ROOT/helpers/synth-computer-use/target/bundle/Synth Computer Use.app"
  local identity avail_kb

  if [[ ! -d "$helper" ]]; then
    echo "[desktop:$NAME] ERROR missing Computer Use helper bundle: $helper" >&2
    echo "[desktop:$NAME] run: ./scripts/build-computer-use-helper.sh ensure-dev" >&2
    exit 1
  fi
  if [[ "$SOURCE_REVISION" == *-dirty ]]; then
    echo "[desktop:$NAME] ERROR dirty source tree; cua-build requires a clean checkout" >&2
    exit 1
  fi
  avail_kb="$(df -k "$ROOT" | awk 'NR==2 {print $4}')"
  if [[ "${avail_kb:-0}" -lt 5242880 ]]; then
    echo "[desktop:$NAME] ERROR insufficient disk (${avail_kb:-0} KiB free; need 5 GiB)" >&2
    exit 1
  fi
  identity="$(resolve_signing_identity)"
  if [[ "$identity" != "-" ]] && ! security find-identity -v -p codesigning 2>/dev/null | rg -F "$identity" >/dev/null; then
    echo "[desktop:$NAME] ERROR signing identity not in keychain: $identity" >&2
    echo "[desktop:$NAME] run: ./scripts/setup-desktop-dev-signing.sh" >&2
    exit 1
  fi
}

sign_cua_bundle() {
  local app_bundle="$1"
  local identity keychain_args=() dev_signing_keychain nested adapter
  identity="$(resolve_signing_identity)"
  if [[ "$identity" != "-" ]]; then
    dev_signing_keychain="$("$ROOT/scripts/setup-desktop-dev-signing.sh")"
    if [[ -n "$dev_signing_keychain" ]]; then
      security unlock-keychain \
        -p "$(<"${SYNTH_DESKTOP_DEV_SIGNING_ROOT:-$HOME/.synth-desktop/dev-signing}/keychain-password")" \
        "$dev_signing_keychain"
      keychain_args=(--keychain "$dev_signing_keychain")
    fi
  fi
  # Adapters ship inside the bundle so the packaged app never executes (and
  # macOS never attributes permissions to) freshly relinked target/debug
  # binaries carrying their own throwaway ad-hoc identities.
  for adapter in "${SYNTH_MCP_ADAPTERS[@]}"; do
    if [[ ! -x "$TARGET_ROOT/debug/$adapter" ]]; then
      echo "[desktop:$NAME] adapter binary is missing: $TARGET_ROOT/debug/$adapter" >&2
      exit 1
    fi
    /usr/bin/ditto "$TARGET_ROOT/debug/$adapter" "$app_bundle/Contents/MacOS/$adapter"
  done
  # Sign inside-out: every nested Mach-O first under its own stable
  # identifier, then the bundle. `--deep` is deprecated and stamps the outer
  # identifier onto nested code, which is exactly the identity collision the
  # instance contract forbids.
  while IFS= read -r nested; do
    [[ "$(basename "$nested")" == "synth-desktop" ]] && continue
    codesign --force --sign "$identity" \
      ${keychain_args[@]+"${keychain_args[@]}"} \
      --identifier "$BUNDLE_ID.$(basename "$nested")" "$nested"
  done < <(find "$app_bundle/Contents/MacOS" -maxdepth 1 -type f -perm -111 | LC_ALL=C sort)
  codesign --force --sign "$identity" \
    ${keychain_args[@]+"${keychain_args[@]}"} \
    --identifier "$BUNDLE_ID" "$app_bundle"
  if [[ "$identity" == "-" ]]; then
    echo "[desktop:$NAME] WARNING ad-hoc signature: TCC/Keychain grants will not survive a rebuild" >&2
  else
    assert_bundle_identity "$app_bundle" "$identity"
  fi
}

signing_requirement() {
  # codesign output differs across macOS versions: some prefix this line with
  # "# ", while current versions print it without the marker.
  codesign -d -r- "$1" 2>/dev/null | sed -n 's/^#* *designated => //p'
}

signing_authority() {
  # Do not exit the consumer early: with pipefail, codesign observes SIGPIPE
  # and turns a successful identity check into status 141.
  codesign -dvv "$1" 2>&1 | awk -F= '/^Authority=/{if (!found) print $2; found=1}'
}

signing_identifier() {
  codesign -dv "$1" 2>&1 | sed -n 's/^Identifier=//p'
}

# TCC and Keychain key permissions off the designated requirement. A stable
# identity means expected explicit identifiers, one shared Authority, and a
# requirement anchored to the certificate rather than a per-build cdhash.
assert_bundle_identity() {
  local app_bundle="$1" expected_authority="${2:-}"
  local host_requirement host_authority nested name expected failures=0
  local manifest_tmp="$MANIFEST.signing.tmp"
  host_requirement="$(signing_requirement "$app_bundle")"
  host_authority="$(signing_authority "$app_bundle")"
  if [[ -z "$host_requirement" || "$host_requirement" == *cdhash* ]]; then
    echo "[desktop:$NAME] ERROR bundle designated requirement is cdhash-anchored (ad-hoc); rebuilds will not keep permissions" >&2
    failures=1
  fi
  if [[ "$(signing_identifier "$app_bundle")" != "$BUNDLE_ID" ]]; then
    echo "[desktop:$NAME] ERROR bundle identifier mismatch: expected $BUNDLE_ID got $(signing_identifier "$app_bundle")" >&2
    failures=1
  fi
  if [[ -n "$expected_authority" && "$host_authority" != "$expected_authority" ]]; then
    echo "[desktop:$NAME] ERROR bundle authority mismatch: expected $expected_authority got ${host_authority:-none}" >&2
    failures=1
  fi
  while IFS= read -r nested; do
    name="$(basename "$nested")"
    expected="$BUNDLE_ID.$name"
    [[ "$name" == "synth-desktop" ]] && expected="$BUNDLE_ID"
    if [[ "$(signing_identifier "$nested")" != "$expected" ]]; then
      echo "[desktop:$NAME] ERROR $name identifier mismatch: expected $expected got $(signing_identifier "$nested")" >&2
      failures=1
    fi
    if [[ "$(signing_requirement "$nested")" == *cdhash* ]]; then
      echo "[desktop:$NAME] ERROR $name designated requirement is cdhash-anchored" >&2
      failures=1
    fi
    if [[ "$name" != "synth-desktop" && "$(signing_authority "$nested")" != "$host_authority" ]]; then
      echo "[desktop:$NAME] ERROR $name authority differs from host: $(signing_authority "$nested")" >&2
      failures=1
    fi
  done < <(find "$app_bundle/Contents/MacOS" -maxdepth 1 -type f -perm -111 | LC_ALL=C sort)
  [[ "$failures" -eq 0 ]] || exit 1
  [[ -f "$MANIFEST" ]] || write_contract
  jq \
    --arg identity "${host_authority:-adhoc}" \
    --arg requirement "$host_requirement" \
    --arg verifiedAt "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    '.signing = {identity: $identity, designatedRequirement: $requirement, verifiedAt: $verifiedAt}' \
    "$MANIFEST" >"$manifest_tmp"
  mv "$manifest_tmp" "$MANIFEST"
  echo "[desktop:$NAME] signing identity=${host_authority:-adhoc}"
  echo "[desktop:$NAME] signing requirement=$host_requirement"
}

# Non-identity runtime paths for development launches. Instance name and bundle
# identity come only from the embedded descriptor and Info.plist.
export_instance_env() {
  export SYNTH_DESKTOP_DATA_ROOT="$DATA_ROOT"
  export SYNTH_DESKTOP_CONFIG="$DATA_ROOT/config.toml"
  export SYNTH_CODEX_HOME="$DATA_ROOT/codex"
  export SYNTH_DESKTOP_WORKSPACE="$WORKSPACE"
  export SYNTH_DESKTOP_SOURCE_REVISION="$SOURCE_REVISION"
  export SYNTH_DESKTOP_VITE_URL="http://127.0.0.1:$VITE_PORT"
  # Named development instances may execute an operator-pinned image already
  # present in the local OCI daemon. The Rust admission check still requires a
  # full sha256 identity; release builds ignore this development-only lane.
  export SYNTH_EVAL_ALLOW_LOCAL_PINNED_TARGETS=1
  # Debug instances use the existing Codex file as a seed and never touch
  # Keychain. Refreshed credentials live in one private machine-local cache so
  # rebuilds and differently named instances reuse a still-valid session.
  local shared_oauth_root="${SYNTH_DESKTOP_SHARED_ROOT:-$HOME/.synth-desktop/shared}/oauth"
  mkdir -p "$shared_oauth_root"
  chmod 700 "$shared_oauth_root"
  if [[ -z "${SYNTH_DESKTOP_DEV_OAUTH_FILE:-}" && -f "$HOME/.codex/auth.json" ]]; then
    SYNTH_DESKTOP_DEV_OAUTH_FILE="$HOME/.codex/auth.json"
  fi
  if [[ -n "${SYNTH_DESKTOP_DEV_OAUTH_FILE:-}" ]]; then
    if [[ ! -s "$SYNTH_DESKTOP_DEV_OAUTH_FILE" ]]; then
      echo "[desktop:$NAME] ERROR ChatGPT auth is required but missing: $SYNTH_DESKTOP_DEV_OAUTH_FILE" >&2
      return 1
    fi
    export SYNTH_DESKTOP_DEV_OAUTH_FILE
  else
    echo "[desktop:$NAME] ERROR ChatGPT auth is required for local Workshop launches; expected $HOME/.codex/auth.json" >&2
    return 1
  fi
  if [[ -z "${SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE:-}" ]]; then
    SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE="$shared_oauth_root/codex.json"
  fi
  export SYNTH_DESKTOP_DEV_OAUTH_STATE_FILE
  export CARGO_TARGET_DIR="$TARGET_ROOT"

  # Profile/account-backend routing is instance-owned: it comes from the
  # instance TOML, never from the shell that happened to launch the app.
  # (write_contract seeds a new TOML from the shell once, at creation.)
  # Responses gateway routing is source-owned by Rust and has no override.
  unset SYNTH_BACKEND_URL SYNTH_INTERN_PROFILE
  if [[ -f "$DATA_ROOT/config.toml" ]]; then
    SYNTH_INTERN_PROFILE="$(python3 - <<'PY' "$DATA_ROOT/config.toml"
import sys, tomllib
from pathlib import Path
data = tomllib.loads(Path(sys.argv[1]).read_text())
print((data.get("intern") or {}).get("profile") or "")
PY
)"
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
    if [[ -n "$SYNTH_INTERN_PROFILE" ]]; then
      export SYNTH_INTERN_PROFILE
    else
      unset SYNTH_INTERN_PROFILE
    fi
    if [[ -n "$SYNTH_BACKEND_URL" ]]; then
      export SYNTH_BACKEND_URL
    else
      unset SYNTH_BACKEND_URL
    fi
  fi

  # Named local CUA bundles are ad-hoc or development signed and therefore
  # cannot satisfy the production helper's Apple-team requirement. Keep the
  # weaker requirement explicit and confined to this development launcher;
  # release builds do not receive this environment override.
  export SYNTH_COMPUTER_USE_PARENT_REQUIREMENT="identifier \"$BUNDLE_ID\" or identifier \"com.synth.desktop.v05.dev.shared\""
}

# Test hook: exercise the environment contract without compiling, signing,
# or launching anything. Prints variable names only, never values.
dry_run_operation() {
  export_instance_env
  write_bundle_descriptor "$GENERATED_ROOT/descriptor-preview.app"
  mark_runtime "dry-run" "$$"
  echo "[desktop:$NAME] dry-run operation=$COMMAND"
  echo "[desktop:$NAME] dry-run env_names=$(compgen -e | rg '^(SYNTH_|CARGO_TARGET_DIR$)' | LC_ALL=C sort | paste -sd, -)"
  echo "[desktop:$NAME] dry-run complete; nothing was built or launched"
  if [[ "${SYNTH_DESKTOP_OPERATION_LOCK_HOLD:-0}" == "1" ]]; then
    echo "[desktop:$NAME] dry-run holding operation lock"
    while true; do
      sleep 1
    done
  fi
}

assert_identity_command() {
  local app_bundle
  app_bundle="$(dirname "$(dirname "$(dirname "$CUA_EXE")")")"
  if [[ ! -d "$app_bundle" ]]; then
    echo "[desktop:$NAME] signed CUA app is missing; run cua-build first" >&2
    exit 1
  fi
  write_contract
  assert_bundle_identity "$app_bundle"
  jq '.signing' "$MANIFEST"
}

dev_instance() {
  write_contract
  if [[ "${SYNTH_DESKTOP_OPERATION_DRY_RUN:-0}" == "1" ]]; then
    dry_run_operation
    return
  fi
  if [[ -n "$(instance_processes)" ]]; then
    echo "[desktop:$NAME] already running; use desktop:instance:stop first" >&2
    exit 1
  fi

  # Capture provenance before any compile so a mid-build dirty tree fails closed.
  # A run-only launch must validate the already-signed bundle instead of
  # replacing its receipt with the unsigned raw target's identity.
  local pre_build_revision="$SOURCE_REVISION"
  if [[ "$COMMAND" == "cua" || "$COMMAND" == "cua-build" ]]; then
    packaging_preflight
  fi
  if [[ "$COMMAND" == "cua-run" ]]; then
    verify_packaged_provenance
  else
    revalidate_provenance "pre-build" "$pre_build_revision"
  fi

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

  export_instance_env
  stage_gepa_runtime
  echo "[desktop:$NAME] profile=${SYNTH_INTERN_PROFILE:-} backend=${SYNTH_BACKEND_URL:-} gateway=source-owned"

  if [[ "$COMMAND" == "cua-run" ]]; then
    if [[ ! -x "$CUA_EXE" ]]; then
      echo "[desktop:$NAME] bundle was not produced by cua-build; run desktop-instance.sh rebuild-run $NAME" >&2
      exit 1
    fi
    codesign --verify --deep --strict "$(dirname "$(dirname "$(dirname "$CUA_EXE")")")"
    echo "[desktop:$NAME] launching existing signed CUA app from $INSTANCE_ROOT"
    cd "$INSTANCE_ROOT"
    exec_isolated_cua_bundle
  fi

  # The adapter prebuild compiles the shared desktop library and therefore
  # runs Tauri code generation too. Give it the same overlay as `tauri dev`;
  # otherwise Cargo can cache the canonical bundle identifier and the named
  # process is mistaken for a second canonical app by the single-instance
  # plugin.
  export TAURI_CONFIG
  TAURI_CONFIG="$(<"$CONFIG")"
  # Build metadata is compiled into the shared desktop library. Export the
  # candidate revision before the adapter prebuild; exporting it afterward can
  # reuse a library carrying an older revision while the instance manifest
  # claims the current source.
  export SYNTH_DESKTOP_SOURCE_REVISION="$SOURCE_REVISION"

  local adapters_ready=1 adapter
  local adapter_bin_args=()
  for adapter in "${SYNTH_MCP_ADAPTERS[@]}"; do
    [[ -x "$TARGET_ROOT/debug/$adapter" ]] || adapters_ready=0
    adapter_bin_args+=(--bin "$adapter")
  done
  if [[ "$adapters_ready" == "0" || "${SYNTH_DESKTOP_REBUILD_ADAPTERS:-0}" == "1" ]]; then
    echo "[desktop:$NAME] building embedded-agent MCP adapters"
    cargo build \
      --manifest-path "$ROOT/apps/synth_desktop/src-tauri/Cargo.toml" \
      --features eval-driver \
      "${adapter_bin_args[@]}"
  else
    echo "[desktop:$NAME] reusing embedded-agent MCP adapters (set SYNTH_DESKTOP_REBUILD_ADAPTERS=1 to refresh)"
  fi

  revalidate_provenance "post-build" "$pre_build_revision"
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
    # Instance builds carry the QA control plane; release artifacts never
    # enable this feature.
    # Packaging pins synth-mlx-rl 5d6db143 + lock sha. The sibling working
    # tree is often dirty WIP and will fail closed; prefer the v0.8 pin.
    if [[ -z "${SYNTH_MLX_RL_PROJECT_ROOT:-}" ]]; then
      if [[ -f "$REPO_SIBLING_ROOT/synth-mlx-rl-v08-compat/pyproject.toml" ]]; then
        SYNTH_MLX_RL_PROJECT_ROOT="$REPO_SIBLING_ROOT/synth-mlx-rl-v08-compat"
      elif [[ -f "$REPO_SIBLING_ROOT/synth-mlx-rl-v08-pinned/pyproject.toml" ]]; then
        SYNTH_MLX_RL_PROJECT_ROOT="$REPO_SIBLING_ROOT/synth-mlx-rl-v08-pinned"
      else
        SYNTH_MLX_RL_PROJECT_ROOT="$REPO_SIBLING_ROOT/synth-mlx-rl"
      fi
    fi
    export SYNTH_MLX_RL_PROJECT_ROOT
    "$ROOT/scripts/stage-mlx-runtime-distribution.sh"
    npx tauri build --debug --features eval-driver --bundles app --config "$PACKAGE_CONFIG" --config "$CONFIG"
    local app_bundle="$CARGO_TARGET_DIR/debug/bundle/macos/$APP_TITLE.app"
    local app_executable="$CUA_EXE"
    if [[ ! -x "$app_executable" ]]; then
      echo "[desktop:$NAME] expected CUA bundle executable missing: $app_executable" >&2
      exit 1
    fi
    revalidate_provenance "bundle-built" "$pre_build_revision"
    write_bundle_descriptor "$app_bundle"
    sign_cua_bundle "$app_bundle"
    codesign --verify --deep --strict "$app_bundle"
    record_packaged_provenance "$app_bundle"
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
    exec_isolated_cua_bundle
  fi
  release_operation_lock_before_exec
  exec npx tauri dev --features eval-driver --config "$PACKAGE_CONFIG" --config "$CONFIG"
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

wait_for_health_instance() {
  local descriptor="$DATA_ROOT/eval-driver.json" i report url token instance
  for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30; do
    if [[ -f "$descriptor" ]]; then
      url="$(jq -r '.url // empty' "$descriptor" 2>/dev/null || true)"
      token="$(jq -r '.token // empty' "$descriptor" 2>/dev/null || true)"
      if [[ -n "$url" ]]; then
        set +e
        report="$(curl -sf --max-time 2 ${token:+-H "Authorization: Bearer $token"} "$url/health" 2>/dev/null)"
        set -e
        instance="$(printf '%s' "${report:-}" | jq -r '.instance.name // empty' 2>/dev/null || true)"
        if [[ "$instance" == "$NAME" ]]; then
          printf '%s\n' "$report"
          return 0
        fi
      fi
    fi
    sleep 2
  done
  echo "[desktop:$NAME] ERROR /health.instance never matched $NAME" >&2
  return 1
}

print_runtime_identity() {
  echo "[desktop:$NAME] runtime identity"
  jq '{
    name: .name,
    bundleId: .bundleId,
    instanceRoot: .instanceRoot,
    dataRoot: .dataRoot,
    sourceRevision: .sourceRevision,
    runtime: .runtime
  }' "$MANIFEST"
}

# build → bundle → sign → record → verify → launch with descriptor →
# wait for /health.instance == NAME → print runtime identity. One command.
rebuild_run_instance() {
  write_contract
  if [[ "${SYNTH_DESKTOP_OPERATION_DRY_RUN:-0}" == "1" ]]; then
    echo "[desktop:$NAME] rebuild-run steps=build,bundle,sign,record,verify,launch,wait-health,print-runtime"
    echo "[desktop:$NAME] rebuild-run would wait for /health.instance == $NAME"
    dry_run_operation
    return
  fi
  COMMAND=cua-build
  dev_instance
  COMMAND=cua-run
  verify_packaged_provenance
  export_instance_env
  echo "[desktop:$NAME] launching recorded bundle from $INSTANCE_ROOT"
  cd "$INSTANCE_ROOT"
  exec_isolated_cua_bundle background
  wait_for_health_instance >/dev/null
  print_runtime_identity
}

case "$COMMAND" in
  cua-build|cua-run|cua|stop|clean|stage|rebuild-run)
    acquire_operation_lock "$COMMAND"
    ;;
esac

case "$COMMAND" in
  dev|cua|cua-build|cua-run) dev_instance ;;
  rebuild-run) rebuild_run_instance ;;
  assert-identity) assert_identity_command ;;
  status) status_instance ;;
  stage) stage_instance ;;
  stop) stop_instance ;;
  clean) clean_instance ;;
  print) print_contract ;;
  *) usage; exit 2 ;;
esac
