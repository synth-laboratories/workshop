#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${SYNTH_MLX_RL_PROJECT_ROOT:-$(dirname "$ROOT")/synth-mlx-rl}"
TARGET="$ROOT/runtime-distributions/mlx-rl"
VERSION="0.6.0"
# Derived from the Rust catalog, never restated. mlx_runtime.rs is what
# verifies the manifest at install time, so a constant copied here can drift
# from it -- and did: this script staged 6b4595f9 while the app demanded
# 5d6db143, so every packaged build produced a runtime the app then refused
# with "manifest does not match the pinned catalog".
MLX_CATALOG="$ROOT/apps/synth_desktop/src-tauri/src/optimizers/mlx_runtime.rs"
EXPECTED_SOURCE_REVISION="$(rg -o 'MLX_RUNTIME_SOURCE_REVISION: &str = "([0-9a-f]{40})"' --replace '$1' -m1 "$MLX_CATALOG")"
EXPECTED_LOCK_SHA256="$(rg -o 'MLX_RUNTIME_LOCK_SHA256: &str =\s*\n?\s*"([0-9a-f]{64})"' --replace '$1' -m1 --multiline "$MLX_CATALOG")"
[[ -n "$EXPECTED_SOURCE_REVISION" && -n "$EXPECTED_LOCK_SHA256" ]] || {
  echo "[mlx-runtime] cannot read the pinned catalog from $MLX_CATALOG" >&2
  exit 1
}
UV="${SYNTH_OPTIMIZER_UV_PATH:-}"

# Only consulted when the staged wheelhouse cannot be reused. A verified
# distribution is immutable input to a build, so requiring a clean release
# checkout to *reuse* it made packaging depend on a live source folder that
# has nothing left to contribute -- and failed the build outright when that
# folder was dirty or parked at another revision, which is how this ran
# aground: the wheelhouse on disk already matched the pin exactly.
require_release_source() {
if [[ ! -f "$PROJECT/pyproject.toml" ]] || ! rg -q '^name = "synth-mlx-rl"$' "$PROJECT/pyproject.toml"; then
  echo "[mlx-runtime] synth-mlx-rl source is unavailable at $PROJECT" >&2
  echo "[mlx-runtime] set SYNTH_MLX_RL_PROJECT_ROOT to the release checkout" >&2
  exit 1
fi
if [[ ! -f "$PROJECT/uv.lock" ]]; then
  echo "[mlx-runtime] pinned source lock is unavailable at $PROJECT/uv.lock" >&2
  exit 1
fi
SOURCE_REVISION="$(git -C "$PROJECT" rev-parse HEAD)"
LOCK_SHA256="$(shasum -a 256 "$PROJECT/uv.lock" | awk '{print $1}')"
if [[ -n "$(git -C "$PROJECT" status --porcelain=v1 --untracked-files=all)" ]]; then
  echo "[mlx-runtime] release source must be clean: $PROJECT" >&2
  exit 1
fi
if [[ "$SOURCE_REVISION" != "$EXPECTED_SOURCE_REVISION" ]]; then
  echo "[mlx-runtime] expected synth-mlx-rl $EXPECTED_SOURCE_REVISION, got $SOURCE_REVISION" >&2
  exit 1
fi
if [[ "$LOCK_SHA256" != "$EXPECTED_LOCK_SHA256" ]]; then
  echo "[mlx-runtime] expected uv.lock $EXPECTED_LOCK_SHA256, got $LOCK_SHA256" >&2
  exit 1
fi
}

# A matching wheelhouse is enough for CUA/debug packaging. Rebuilding with
# `uv build` is Killed:9 on this machine (Homebrew uv / memory pressure) and
# is not required when every artifact hash already matches the pin.
reuse_existing_wheelhouse() {
  [[ "${SYNTH_MLX_RL_REBUILD:-0}" == "1" ]] && return 1
  [[ -f "$TARGET/manifest.json" ]] || return 1
  /usr/bin/python3 - "$TARGET" "$VERSION" "$EXPECTED_SOURCE_REVISION" "$EXPECTED_LOCK_SHA256" <<'PY'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
version, expected_rev, expected_lock = sys.argv[2], sys.argv[3], sys.argv[4]
manifest = json.loads((root / "manifest.json").read_text())
if manifest.get("schemaVersion") != "synth.mlx-runtime-wheelhouse.v1":
    raise SystemExit("schema")
if manifest.get("package") != "synth-mlx-rl":
    raise SystemExit("package")
if manifest.get("version") != version:
    raise SystemExit("version")
if manifest.get("sourceRevision") != expected_rev:
    raise SystemExit("revision")
if manifest.get("lockSha256") != expected_lock:
    raise SystemExit("lock")
artifacts = manifest.get("artifacts") or []
if not artifacts:
    raise SystemExit("empty")
wheels = root / "wheels"
have_package = False
for artifact in artifacts:
    name = artifact.get("fileName") or ""
    path = wheels / name
    if not path.is_file():
        raise SystemExit(f"missing {name}")
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != artifact.get("sha256"):
        raise SystemExit(f"hash {name}")
    if name.startswith(f"synth_mlx_rl-{version}-") and name.endswith(".whl"):
        have_package = True
if not have_package:
    raise SystemExit("package wheel")
PY
}

if reuse_existing_wheelhouse; then
  echo "[mlx-runtime] reusing verified wheelhouse at $TARGET"
  exit 0
fi

require_release_source

if [[ -z "$UV" ]]; then
  for candidate in /opt/homebrew/bin/uv /usr/local/bin/uv "$HOME/.local/bin/uv" "$HOME/.cargo/bin/uv"; do
    if [[ -x "$candidate" ]]; then UV="$candidate"; break; fi
  done
fi
if [[ ! -x "$UV" ]]; then
  echo "[mlx-runtime] uv is required to stage the offline wheelhouse" >&2
  exit 1
fi

STAGING="$(mktemp -d "${TMPDIR:-/tmp}/synth-mlx-runtime.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
mkdir -p "$STAGING/build" "$STAGING/wheels"

"$UV" build --wheel --out-dir "$STAGING/build" "$PROJECT"
WHEEL="$(find "$STAGING/build" -maxdepth 1 -type f -name "synth_mlx_rl-${VERSION}-*.whl" -print -quit)"
if [[ -z "$WHEEL" ]]; then
  echo "[mlx-runtime] build omitted synth-mlx-rl==$VERSION" >&2
  exit 1
fi

"$UV" export --project "$PROJECT" --extra mlx --no-dev --no-emit-project \
  --format requirements-txt --no-hashes --frozen --output-file "$STAGING/requirements.txt"
"$UV" run --no-project --with pip python -m pip download \
  --only-binary=:all: --dest "$STAGING/wheels" --requirement "$STAGING/requirements.txt"
cp "$WHEEL" "$STAGING/wheels/"

/usr/bin/python3 - "$STAGING" "$VERSION" "$SOURCE_REVISION" "$LOCK_SHA256" <<'PY'
import hashlib, json, pathlib, sys
root = pathlib.Path(sys.argv[1])
version = sys.argv[2]
source_revision = sys.argv[3]
lock_sha256 = sys.argv[4]
artifacts = []
for path in sorted((root / "wheels").glob("*.whl")):
    data = path.read_bytes()
    artifacts.append({
        "fileName": path.name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "sizeBytes": len(data),
    })
if not artifacts:
    raise SystemExit("wheelhouse is empty")
(root / "manifest.json").write_text(json.dumps({
    "schemaVersion": "synth.mlx-runtime-wheelhouse.v1",
    "package": "synth-mlx-rl",
    "version": version,
    "sourceRevision": source_revision,
    "lockSha256": lock_sha256,
    "artifacts": artifacts,
}, indent=2) + "\n")
PY

rm -rf "$TARGET"
mkdir -p "$(dirname "$TARGET")"
mv "$STAGING" "$TARGET"
trap - EXIT
echo "[mlx-runtime] staged verified offline wheelhouse at $TARGET"
