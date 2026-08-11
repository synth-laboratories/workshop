#!/usr/bin/env bash
# Canonical Synth Desktop development lifecycle.
#
# This script prevents the build-tree and /Applications copies from running at
# the same time. Acceptance and CUA should always target the installed app.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP_NAME="Synth Desktop.app"
INSTALLED_APP="${SYNTH_DESKTOP_APP_PATH:-/Applications/$APP_NAME}"
BUNDLE_APP="$ROOT/apps/synth_desktop/src-tauri/target/release/bundle/macos/$APP_NAME"
INSTALLED_EXE="$INSTALLED_APP/Contents/MacOS/synth-desktop"
BUNDLE_EXE="$BUNDLE_APP/Contents/MacOS/synth-desktop"
DEBUG_EXE="$ROOT/apps/synth_desktop/src-tauri/target/debug/synth-desktop"
BACKUP_ROOT="${SYNTH_DESKTOP_BACKUP_ROOT:-$HOME/.synth-desktop/backups/app-builds}"
INSTALL_STAGE=""
LAGUNA_PID_FILE="$HOME/.synth-desktop/laguna/sidecar.pid"
INSTALL_LOCK_DIR="${SYNTH_DESKTOP_INSTALL_LOCK_DIR:-$HOME/.synth-desktop/locks/canonical-install}"
INSTALL_LOCK_HELD=0

enable_rust_cache() {
	if command -v sccache >/dev/null 2>&1; then
		export RUSTC_WRAPPER="${RUSTC_WRAPPER:-$(command -v sccache)}"
		export SCCACHE_DIR="${SCCACHE_DIR:-$HOME/.cache/synth-workshop/sccache}"
		mkdir -p "$SCCACHE_DIR"
	fi
}

run_renderer_typecheck() {
	cd "$ROOT"
	npx turbo run typecheck --filter=@synth/synth-desktop
}

cleanup_stage() {
  if [[ -n "$INSTALL_STAGE" && -d "$INSTALL_STAGE" ]]; then
    /bin/rm -rf "$INSTALL_STAGE"
  fi
}

release_install_lock() {
  if [[ "$INSTALL_LOCK_HELD" -eq 1 && -d "$INSTALL_LOCK_DIR" ]]; then
    local owner=""
    owner="$(cat "$INSTALL_LOCK_DIR/pid" 2>/dev/null || true)"
    if [[ "$owner" == "$$" ]]; then
      /bin/rm -rf "$INSTALL_LOCK_DIR"
    fi
  fi
  INSTALL_LOCK_HELD=0
}

cleanup_all() {
  cleanup_stage
  release_install_lock
}

trap cleanup_all EXIT

acquire_install_lock() {
  local owner=""
  mkdir -p "$(dirname "$INSTALL_LOCK_DIR")"
  if ! mkdir "$INSTALL_LOCK_DIR" 2>/dev/null; then
    owner="$(cat "$INSTALL_LOCK_DIR/pid" 2>/dev/null || true)"
    if [[ "$owner" =~ ^[1-9][0-9]*$ ]] && kill -0 "$owner" 2>/dev/null; then
      echo "[desktop] ERROR canonical install is already owned by pid $owner" >&2
      return 1
    fi
    echo "[desktop] removing stale canonical install lock${owner:+ from pid $owner}"
    /bin/rm -rf "$INSTALL_LOCK_DIR"
    mkdir "$INSTALL_LOCK_DIR"
  fi
  printf '%s\n' "$$" >"$INSTALL_LOCK_DIR/pid"
  INSTALL_LOCK_HELD=1
}

source_revision() {
  local revision
  revision="$(git -C "$ROOT" rev-parse --short=12 HEAD 2>/dev/null || printf 'unknown')"
  if [[ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no 2>/dev/null)" ]]; then
    revision="$revision-dirty"
  fi
  printf '%s' "$revision"
}

require_source_revision() {
  local phase="$1" expected="$2" current
  current="$(source_revision)"
  if [[ "$current" != "$expected" ]]; then
    echo "[desktop] ERROR provenance drift ($phase): expected $expected got $current" >&2
    return 1
  fi
}

executable_digest() {
  shasum -a 256 "$1" | awk '{print "sha256:" $1}'
}

usage() {
  cat <<'EOF'
Usage: ./scripts/desktop.sh <command>

  dev [name] Run an isolated named Tauri/Vite development instance (default: codex)
  check     Run the fast local type and Rust compile checks
  build     Typecheck and build the signed-app input bundle (no tests)
  verify    Run the full desktop type, Rust, and renderer release gates
  install   Build, install, sign, and launch /Applications (no tests)
  install-release Run full release gates, then install /Applications
  restart   Restart the already-installed canonical app
  stop      Stop every Synth Desktop process, including stale backup/build copies
  status    Show canonical Synth Desktop process and install status
EOF
}

desktop_processes() {
  ps -axo pid=,args= | awk -v installed="$INSTALLED_EXE" -v debug="$DEBUG_EXE" '
    {
      pid=$1
      sub(/^[[:space:]]*[0-9]+[[:space:]]+/, "", $0)
      if ($0 == installed || $0 == debug || $0 ~ /\/Synth Desktop\.app\/Contents\/MacOS\/synth-desktop$/) print pid "\t" $0
    }
  '
}

stop_desktop() {
  local rows pids
  rows="$(desktop_processes)"
  if [[ -z "$rows" ]]; then
    echo "[desktop] no Synth Desktop process is running"
    return
  fi
  echo "$rows" | sed 's/^/[desktop] stopping /'
  pids="$(printf '%s\n' "$rows" | awk '{print $1}')"
  # shellcheck disable=SC2086
  kill $pids 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    [[ -z "$(desktop_processes)" ]] && return
    sleep 0.25
  done
  rows="$(desktop_processes)"
  if [[ -n "$rows" ]]; then
    echo "$rows" | sed 's/^/[desktop] force stopping /'
    pids="$(printf '%s\n' "$rows" | awk '{print $1}')"
    # Exact executable paths were resolved above; escalation stays scoped.
    # shellcheck disable=SC2086
    kill -KILL $pids 2>/dev/null || true
  fi
}

stop_managed_laguna() {
  local pid command
  [[ -f "$LAGUNA_PID_FILE" ]] || return 0
  pid="$(tr -d '[:space:]' < "$LAGUNA_PID_FILE")"
  if [[ ! "$pid" =~ ^[1-9][0-9]*$ ]]; then
    echo "[desktop] ignoring invalid Laguna pid file: $LAGUNA_PID_FILE" >&2
    return
  fi
  command="$(ps -p "$pid" -o command= 2>/dev/null || true)"
  if [[ "$command" != *"-m laguna_daemon"* ]]; then
    echo "[desktop] stale Laguna pid $pid is not our managed daemon"
    /bin/rm -f "$LAGUNA_PID_FILE"
    return
  fi
  echo "[desktop] stopping managed Laguna sidecar $pid"
  kill -TERM "$pid" 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do
    kill -0 "$pid" 2>/dev/null || break
    sleep 0.25
  done
  if kill -0 "$pid" 2>/dev/null; then
    echo "[desktop] force stopping managed Laguna sidecar $pid"
    kill -KILL "$pid" 2>/dev/null || true
  fi
  /bin/rm -f "$LAGUNA_PID_FILE"
}

status_desktop() {
  local rows installed_count debug_count other_count total_count
  rows="$(desktop_processes)"
  if [[ -z "$rows" ]]; then
    echo "[desktop] stopped"
  else
    printf '%s\n' "$rows" | sed 's/^/[desktop] running /'
  fi
  installed_count="$(printf '%s\n' "$rows" | awk -F '\t' -v exe="$INSTALLED_EXE" '$2 == exe {count++} END {print count+0}')"
  debug_count="$(printf '%s\n' "$rows" | awk -F '\t' -v exe="$DEBUG_EXE" '$2 == exe {count++} END {print count+0}')"
  other_count="$(printf '%s\n' "$rows" | awk -F '\t' -v installed="$INSTALLED_EXE" -v debug="$DEBUG_EXE" '$2 != "" && $2 != installed && $2 != debug {count++} END {print count+0}')"
  total_count=$((installed_count + debug_count + other_count))
  if [[ -d "$INSTALLED_APP" ]]; then
    echo "[desktop] installed $INSTALLED_APP"
  else
    echo "[desktop] not installed at $INSTALLED_APP"
  fi
  if [[ "$total_count" -gt 1 || "$other_count" -gt 0 ]]; then
    echo "[desktop] ERROR: duplicate or noncanonical Synth Desktop process detected" >&2
    return 1
  fi
  if [[ "$debug_count" -eq 1 ]]; then
    echo "[desktop] mode development"
  elif [[ "$installed_count" -eq 1 ]]; then
    echo "[desktop] mode installed acceptance"
  fi
}

launch_installed() {
  local stable=0 rows installed_count other_count
  [[ -x "$INSTALLED_EXE" ]] || {
    echo "[desktop] missing executable: $INSTALLED_EXE" >&2
    return 1
  }
  /usr/bin/open -na "$INSTALLED_APP"
  for _ in {1..40}; do
    rows="$(desktop_processes)"
    installed_count="$(printf '%s\n' "$rows" | awk -F '\t' -v exe="$INSTALLED_EXE" '$2 == exe {count++} END {print count+0}')"
    other_count="$(printf '%s\n' "$rows" | awk -F '\t' -v exe="$INSTALLED_EXE" '$2 != "" && $2 != exe {count++} END {print count+0}')"
    if [[ "$installed_count" -eq 1 && "$other_count" -eq 0 ]]; then
      stable=$((stable + 1))
    else
      stable=0
    fi
    if [[ "$stable" -ge 6 ]]; then
      status_desktop
      return
    fi
    sleep 0.5
  done
  echo "[desktop] canonical app did not remain stable: $INSTALLED_APP" >&2
  return 1
}

verify_desktop() {
  cd "$ROOT"
	enable_rust_cache
	run_renderer_typecheck
  cargo test --manifest-path apps/synth_desktop/src-tauri/Cargo.toml
  ./scripts/test-desktop-instance.sh
  ./scripts/test-desktop-install-lock.sh
  npm run test:playwright --workspace @synth/synth-desktop
}

verify_desktop_fast() {
  cd "$ROOT"
	enable_rust_cache
	local type_pid type_status=0 rust_status=0
	run_renderer_typecheck &
	type_pid=$!
	cargo check --manifest-path apps/synth_desktop/src-tauri/Cargo.toml || rust_status=$?
	wait "$type_pid" || type_status=$?
	[[ "$type_status" -eq 0 && "$rust_status" -eq 0 ]]
}

build_desktop() {
	cd "$ROOT"
	enable_rust_cache
	local type_pid type_status=0 build_status=0
	# TypeScript checking is read-only and independent of the Tauri/Rust build,
	# so overlap it with the real packaging compilation instead of serializing it.
	run_renderer_typecheck &
	type_pid=$!
	(cd "$ROOT/apps/synth_desktop" && npx tauri build --bundles app) || build_status=$?
	wait "$type_pid" || type_status=$?
	[[ "$type_status" -eq 0 && "$build_status" -eq 0 ]]
}

install_desktop() {
	local verification="${1:-fast}" timestamp stage backup="" pre_build_revision stage_digest installed_digest
	acquire_install_lock
	pre_build_revision="$(source_revision)"
	if [[ "$verification" == "release" ]]; then
		verify_desktop
	fi
	# The synth_* MCP noun adapters are NOT bundled into the .app; the installed
	# app resolves them beside its executable and then falls back to the build
	# tree's target/debug copies (see codex.rs). Building release copies here
	# added ~20s of link time per install without changing what the installed
	# app loads, so it was removed; bundling them as Tauri sidecars is the real
	# fix and is tracked in the launch program.
	build_desktop
  require_source_revision "post-build" "$pre_build_revision"
  [[ -d "$BUNDLE_APP" && -x "$BUNDLE_EXE" ]] || {
    echo "[desktop] build did not produce $BUNDLE_APP" >&2
    return 1
  }

  stop_desktop
  stop_managed_laguna
  timestamp="$(date '+%Y%m%d-%H%M%S')"
  stage="/Applications/.Synth Desktop.stage-$timestamp.app"
  INSTALL_STAGE="$stage"
  mkdir -p "$BACKUP_ROOT"
  if [[ -e "$stage" ]]; then
    echo "[desktop] refusing to overwrite staging path: $stage" >&2
    return 1
  fi
  /usr/bin/ditto "$BUNDLE_APP" "$stage"
  for adapter in synth-containers-mcp synth-visuals-mcp synth-optimizers-mcp; do
    /usr/bin/ditto \
      "$ROOT/apps/synth_desktop/src-tauri/target/release/$adapter" \
      "$stage/Contents/MacOS/$adapter"
  done
  # The generated release bundle is not a supported launch target. Removing it
  # after staging makes stale Dock/Finder entries fail closed instead of opening
  # a second app with a different environment and state directory.
  /bin/rm -rf "$BUNDLE_APP"
  /usr/bin/codesign --force --deep --sign - "$stage"
  /usr/bin/codesign --verify --deep --strict "$stage"
  stage_digest="$(executable_digest "$stage/Contents/MacOS/synth-desktop")"
  require_source_revision "pre-install" "$pre_build_revision"

  if [[ -d "$INSTALLED_APP" ]]; then
    backup="$BACKUP_ROOT/Synth Desktop-$timestamp.app"
    mv "$INSTALLED_APP" "$backup"
    echo "[desktop] previous app backed up to $backup"
  fi

  if ! mv "$stage" "$INSTALLED_APP"; then
    [[ -n "$backup" && ! -e "$INSTALLED_APP" ]] && mv "$backup" "$INSTALLED_APP"
    echo "[desktop] install failed; previous app restored" >&2
    return 1
  fi
  INSTALL_STAGE=""
  /usr/bin/codesign --verify --deep --strict "$INSTALLED_APP"
  installed_digest="$(executable_digest "$INSTALLED_EXE")"
  if [[ "$installed_digest" != "$stage_digest" ]]; then
    echo "[desktop] ERROR installed executable digest drift: staged $stage_digest installed $installed_digest" >&2
    return 1
  fi
  require_source_revision "pre-launch" "$pre_build_revision"
  echo "[desktop] installed $INSTALLED_APP"
  echo "[desktop] provenance $pre_build_revision $installed_digest"
  launch_installed
}

command="${1:-}"
case "$command" in
  dev)
    exec "$ROOT/scripts/desktop-instance.sh" dev "${2:-codex}"
    ;;
  verify)
    verify_desktop
    ;;
  verify-fast)
    verify_desktop_fast
    ;;
	check)
		verify_desktop_fast
		;;
	build)
		build_desktop
		;;
  install)
		install_desktop fast
		;;
	install-release)
		install_desktop release
    ;;
  restart)
    acquire_install_lock
    stop_desktop
    launch_installed
    ;;
  _lock-probe)
    acquire_install_lock
    sleep "${2:-1}"
    ;;
  stop)
    stop_desktop
    ;;
  status)
    status_desktop
    ;;
  *)
    usage
    exit 2
    ;;
esac
