#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROJECT="${SYNTH_MLX_RL_PROJECT_ROOT:-$(dirname "$ROOT")/synth-mlx-rl}"
TARGET="$ROOT/runtime-distributions/mlx-rl"
VERSION="0.6.0"
EXPECTED_SOURCE_REVISION="5d6db14330babcff170d2afbb8535de2138385a9"
EXPECTED_LOCK_SHA256="7f14b704ba9a6c30e6ced5cc88fc2ba6a58a936a9531cfaf168cbb664f83c420"
UV="${SYNTH_OPTIMIZER_UV_PATH:-}"

verify_existing_wheelhouse() {
  python3 - "$TARGET" "$VERSION" "$EXPECTED_SOURCE_REVISION" "$EXPECTED_LOCK_SHA256" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
version, revision, lock_sha256 = sys.argv[2:]
try:
    manifest = json.loads((root / "manifest.json").read_text())
    if manifest.get("schemaVersion") != "synth.mlx-runtime-wheelhouse.v1":
        raise ValueError("unexpected manifest schema")
    if manifest.get("package") != "synth-mlx-rl" or manifest.get("version") != version:
        raise ValueError("unexpected package or version")
    if manifest.get("sourceRevision") != revision or manifest.get("lockSha256") != lock_sha256:
        raise ValueError("runtime pin does not match this release")
    artifacts = manifest.get("artifacts")
    if not isinstance(artifacts, list) or not artifacts:
        raise ValueError("manifest has no artifacts")
    for artifact in artifacts:
        name = artifact.get("fileName")
        expected_hash = artifact.get("sha256")
        expected_size = artifact.get("sizeBytes")
        if not isinstance(name, str) or pathlib.Path(name).name != name:
            raise ValueError("invalid artifact name")
        if not isinstance(expected_hash, str) or not isinstance(expected_size, int):
            raise ValueError("invalid artifact metadata")
        path = root / "wheels" / name
        if not path.is_file() or path.stat().st_size != expected_size:
            raise ValueError(f"missing or resized artifact: {name}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != expected_hash:
            raise ValueError(f"artifact hash mismatch: {name}")
except (OSError, ValueError, json.JSONDecodeError, TypeError, AttributeError) as error:
    print(f"[mlx-runtime] existing wheelhouse is not reusable: {error}", file=sys.stderr)
    raise SystemExit(1)
PY
}

# The release pin is carried by the wheelhouse manifest and every wheel hash.
# Reuse a verified copy before asking for a mutable source checkout, so named
# CUA instances remain reproducible even while a developer has unrelated MLX
# work in progress elsewhere.
if verify_existing_wheelhouse; then
  echo "[mlx-runtime] reusing verified offline wheelhouse at $TARGET"
  exit 0
fi

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

python3 - "$STAGING" "$VERSION" "$SOURCE_REVISION" "$LOCK_SHA256" <<'PY'
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
