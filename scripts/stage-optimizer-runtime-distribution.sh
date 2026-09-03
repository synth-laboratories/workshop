#!/usr/bin/env bash
# Stage the pinned Synth Optimizers wheel carried by a Workshop app bundle.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/runtime-distributions/optimizers"
VERSION="0.2.20"
EXPECTED_SOURCE_REVISION="96d7bbabf7c23f80732c57ca08e69f66ffcdf873"
EXPECTED_LOCK_SHA256="b3dd2c3171fbf37aa78ff14ce6fb9edb43d546ae9fbadd9c5809aeb5c2edb160"
PROJECT="${SYNTH_OPTIMIZER_DISTRIBUTION_SOURCE:-}"

verify_existing_distribution() {
  python3 - "$TARGET" "$VERSION" "$EXPECTED_SOURCE_REVISION" "$EXPECTED_LOCK_SHA256" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
version, revision, lock_sha256 = sys.argv[2:]
try:
    manifest = json.loads((root / "manifest.json").read_text())
    if manifest.get("schemaVersion") != "synth.optimizer-runtime-distribution.v1":
        raise ValueError("unexpected manifest schema")
    if manifest.get("package") != "synth-optimizers" or manifest.get("version") != version:
        raise ValueError("unexpected package or version")
    if manifest.get("sourceRevision") != revision or manifest.get("lockSha256") != lock_sha256:
        raise ValueError("runtime pin does not match this release")
    artifact = manifest.get("artifact")
    if not isinstance(artifact, dict):
        raise ValueError("manifest has no primary artifact")
    name = artifact.get("fileName")
    expected_hash = artifact.get("sha256")
    expected_size = artifact.get("sizeBytes")
    if not isinstance(name, str) or pathlib.Path(name).name != name:
        raise ValueError("invalid artifact name")
    if not name.startswith(f"synth_optimizers-{version}-"):
        raise ValueError("unexpected primary artifact")
    if not isinstance(expected_hash, str) or not isinstance(expected_size, int):
        raise ValueError("invalid artifact metadata")
    path = root / "wheels" / name
    if not path.is_file() or path.stat().st_size != expected_size:
        raise ValueError("missing or resized primary artifact")
    if hashlib.sha256(path.read_bytes()).hexdigest() != expected_hash:
        raise ValueError("primary artifact digest mismatch")
except (OSError, ValueError, json.JSONDecodeError, TypeError, AttributeError) as error:
    print(f"[optimizers-runtime] existing distribution is not reusable: {error}", file=sys.stderr)
    raise SystemExit(1)
PY
}

# A verified resource is immutable input to a named CUA build. Reuse it before
# consulting an optional release checkout, so the packaged app never acquires a
# dependency on a live source folder at runtime.
if verify_existing_distribution; then
  echo "[optimizers-runtime] reusing verified embedded distribution at $TARGET"
  exit 0
fi

if [[ -z "$PROJECT" ]]; then
  echo "[optimizers-runtime] synth-optimizers source is unavailable" >&2
  echo "[optimizers-runtime] set SYNTH_OPTIMIZER_DISTRIBUTION_SOURCE to the pinned release checkout" >&2
  exit 1
fi
if [[ ! -f "$PROJECT/pyproject.toml" ]] || ! rg -q '^name = "synth-optimizers"$' "$PROJECT/pyproject.toml"; then
  echo "[optimizers-runtime] invalid synth-optimizers source: $PROJECT" >&2
  exit 1
fi
if [[ ! -f "$PROJECT/uv.lock" ]]; then
  echo "[optimizers-runtime] pinned source lock is unavailable at $PROJECT/uv.lock" >&2
  exit 1
fi
if [[ -n "$(git -C "$PROJECT" status --porcelain=v1 --untracked-files=all)" ]]; then
  echo "[optimizers-runtime] release source must be clean: $PROJECT" >&2
  exit 1
fi
SOURCE_REVISION="$(git -C "$PROJECT" rev-parse HEAD)"
LOCK_SHA256="$(shasum -a 256 "$PROJECT/uv.lock" | awk '{print $1}')"
if [[ "$SOURCE_REVISION" != "$EXPECTED_SOURCE_REVISION" ]]; then
  echo "[optimizers-runtime] expected synth-optimizers $EXPECTED_SOURCE_REVISION, got $SOURCE_REVISION" >&2
  exit 1
fi
if [[ "$LOCK_SHA256" != "$EXPECTED_LOCK_SHA256" ]]; then
  echo "[optimizers-runtime] expected uv.lock $EXPECTED_LOCK_SHA256, got $LOCK_SHA256" >&2
  exit 1
fi

UV="${SYNTH_OPTIMIZER_UV_PATH:-}"
if [[ -z "$UV" ]]; then
  for candidate in /opt/homebrew/bin/uv /usr/local/bin/uv "$HOME/.local/bin/uv" "$HOME/.cargo/bin/uv"; do
    if [[ -x "$candidate" ]]; then UV="$candidate"; break; fi
  done
fi
if [[ ! -x "$UV" ]]; then
  echo "[optimizers-runtime] uv is required to stage the embedded distribution" >&2
  exit 1
fi

STAGING="$(mktemp -d "${TMPDIR:-/tmp}/synth-optimizers-runtime.XXXXXX")"
trap 'rm -rf "$STAGING"' EXIT
mkdir -p "$STAGING/wheels"
WHEEL="$(find "$PROJECT/target/wheels" -maxdepth 1 -type f -name "synth_optimizers-${VERSION}-*.whl" -print -quit 2>/dev/null || true)"
if [[ -z "$WHEEL" ]]; then
  "$UV" build --wheel --out-dir "$STAGING/wheels" "$PROJECT"
  WHEEL="$(find "$STAGING/wheels" -maxdepth 1 -type f -name "synth_optimizers-${VERSION}-*.whl" -print -quit)"
fi
if [[ -z "$WHEEL" ]]; then
  echo "[optimizers-runtime] build omitted synth-optimizers==$VERSION" >&2
  exit 1
fi
if [[ "$(dirname "$WHEEL")" != "$STAGING/wheels" ]]; then
  cp "$WHEEL" "$STAGING/wheels/"
fi

python3 - "$STAGING" "$VERSION" "$SOURCE_REVISION" "$LOCK_SHA256" <<'PY'
import hashlib
import json
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
version, source_revision, lock_sha256 = sys.argv[2:]
wheels = sorted((root / "wheels").glob(f"synth_optimizers-{version}-*.whl"))
if len(wheels) != 1:
    raise SystemExit("embedded distribution must contain exactly one primary wheel")
wheel = wheels[0]
data = wheel.read_bytes()
(root / "manifest.json").write_text(json.dumps({
    "schemaVersion": "synth.optimizer-runtime-distribution.v1",
    "package": "synth-optimizers",
    "version": version,
    "sourceRevision": source_revision,
    "lockSha256": lock_sha256,
    "artifact": {
        "fileName": wheel.name,
        "sha256": hashlib.sha256(data).hexdigest(),
        "sizeBytes": len(data),
    },
}, indent=2) + "\n")
PY

mkdir -p "$(dirname "$TARGET")"
if [[ -e "$TARGET" ]]; then
  retained="${TARGET}.invalid-$(uuidgen | tr '[:upper:]' '[:lower:]' | tr -d '-')"
  mv "$TARGET" "$retained"
  echo "[optimizers-runtime] retained invalid distribution at $retained"
fi
mv "$STAGING" "$TARGET"
trap - EXIT
echo "[optimizers-runtime] staged verified embedded distribution at $TARGET"
