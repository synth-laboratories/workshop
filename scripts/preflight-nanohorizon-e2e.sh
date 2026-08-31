#!/usr/bin/env bash
# Credential-safe, non-Docker readiness gate for the NanoHorizon Craftax E2E.
set -euo pipefail

WORKSHOP_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
GITHUB_ROOT="$(cd "$WORKSHOP_ROOT/.." && pwd -P)"

CONTAINERS_ROOT="${SYNTH_E2E_CONTAINERS_ROOT:-$GITHUB_ROOT/containers-nanohorizon-e2e-final}"
NANOHORIZON_ROOT="${SYNTH_E2E_NANOHORIZON_ROOT:-$GITHUB_ROOT/nanohorizon-e2e-final}"
EVALS_ROOT="${SYNTH_E2E_EVALS_ROOT:-$GITHUB_ROOT/evals-craftax-live-context}"
GAMEBENCH_ROOT="${SYNTH_E2E_GAMEBENCH_ROOT:-$GITHUB_ROOT/gamebench-craftax-live-context}"

CONTAINERS_REVISION="6ae8225a6221ded40b963124b1ac0c59a1b4dda2"
NANOHORIZON_REVISION="e8be0dc5f6565b1744a96ee2e054442ce4185559"
EVALS_REVISION="43ec21b8a73f87a72fae982f5bb614245ea1f106"
GAMEBENCH_REVISION="fcf925554f8b171e91a44986bb65b4c5dfbd9f66"
SOURCE_MANIFEST_DIGEST="sha256:6b9586d74ea2c8b9848954bdc6ac164fa334864324754fdc8b3ebecef1aa2016"

fail() {
  echo "nanohorizon_e2e_not_ready:$1" >&2
  exit 2
}

require_exact_clean_repo() {
  local label="$1"
  local root="$2"
  local expected="$3"
  [[ -d "$root" ]] || fail "$label:root_missing"
  local actual
  actual="$(git -C "$root" rev-parse --verify HEAD 2>/dev/null)" || fail "$label:not_git"
  [[ "$actual" == "$expected" ]] || fail "$label:revision_mismatch:$actual"
  [[ -z "$(git -C "$root" status --porcelain)" ]] || fail "$label:dirty"
}

[[ -z "$(git -C "$WORKSHOP_ROOT" status --porcelain)" ]] || fail "workshop:dirty"

python3 - "$WORKSHOP_ROOT/workshop.recipe.toml" \
  "$WORKSHOP_ROOT/workshop.containers.toml" <<'PY'
import sys
import tomllib
from pathlib import Path

recipe = tomllib.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
container_manifest = tomllib.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))

expected_recipe = {
    "id": "nanohorizon.craftax.glm-5.3-flash.eval.v1",
    "container": "nanohorizon-craftax",
    "provider": "openrouter",
    "model": "z-ai/glm-5.3-flash",
    "policy_source": "src/challenge/policy.py",
    "train_seeds": [780000, 780001, 780002, 780003, 780004],
}
for key, expected in expected_recipe.items():
    if recipe.get(key) != expected:
        raise SystemExit(f"nanohorizon_e2e_not_ready:workshop_recipe:{key}")
if (recipe.get("policy") or {}).get("thinking_budget") != 640:
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_recipe:thinking_budget")
if (recipe.get("bounds") or {}).get("max_cost_usd") != 2.45:
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_recipe:max_cost_usd")
if (recipe.get("bounds") or {}).get("max_total_rollouts") != 5:
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_recipe:max_total_rollouts")
for key, expected in {
    "max_calls": 10,
    "max_steps": 2000,
    "timeout_seconds": 180.0,
}.items():
    if (recipe.get("policy") or {}).get(key) != expected:
        raise SystemExit(f"nanohorizon_e2e_not_ready:workshop_recipe:{key}")

containers = container_manifest.get("container") or []
if len(containers) != 1 or containers[0].get("id") != "nanohorizon-craftax":
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_container:identity")
if containers[0].get("contract") != "synth.container.live-eval.v1":
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_container:contract")
if containers[0].get("health") != "/health":
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_container:health")
if containers[0].get("policy_source") != "src/challenge/policy.py":
    raise SystemExit("nanohorizon_e2e_not_ready:workshop_container:policy_source")
PY

require_exact_clean_repo "containers" "$CONTAINERS_ROOT" "$CONTAINERS_REVISION"
require_exact_clean_repo "nanohorizon" "$NANOHORIZON_ROOT" "$NANOHORIZON_REVISION"
require_exact_clean_repo "evals" "$EVALS_ROOT" "$EVALS_REVISION"
require_exact_clean_repo "gamebench" "$GAMEBENCH_ROOT" "$GAMEBENCH_REVISION"

python3 - "$NANOHORIZON_ROOT/workshop.containers.toml" \
  "$CONTAINERS_REVISION" "$EVALS_REVISION" "$GAMEBENCH_REVISION" \
  "$SOURCE_MANIFEST_DIGEST" <<'PY'
import sys
import tomllib
from pathlib import Path

manifest_path = Path(sys.argv[1])
expected = {
    "SYNTH_CONTAINERS_SOURCE_REVISION": sys.argv[2],
    "SYNTH_EVALS_SOURCE_REVISION": sys.argv[3],
    "SYNTH_GAMEBENCH_SOURCE_REVISION": sys.argv[4],
}
payload = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
rows = payload.get("container") or []
if len(rows) != 1:
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:container_count")
container = rows[0]
if container.get("id") != "nanohorizon-craftax":
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:container_id")
if container.get("contract") != "synth.container.live-eval.v1":
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:contract")
if container.get("health") != "/health":
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:health")
launch = rows[0].get("launch") or {}
if launch.get("environment") != {**expected, "WORKSHOP_PROXY_ONLY": "1", "REPLACE": "1"}:
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:launch_environment")
if launch.get("command") != ["scripts/up_craftax_container.sh"]:
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:launch_command")
if launch.get("health_target") != "craftax_nanohorizon":
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:health_target")
source = launch.get("source") or {}
if source.get("tracked_revision") != "a6e9999daf811adf2c67351c544bf647411d3e81":
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:tracked_revision")
if source.get("include") != [
    "scripts/up_craftax_container.sh",
    "scripts/lib_local.sh",
    "scripts/validate_craftax_sources.py",
    "src/challenge/policy.py",
]:
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:source_include")
if source.get("dirty_digest") != sys.argv[5]:
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:source_digest")
expected_port = launch.get("expected_port")
if not isinstance(expected_port, int) or not 18080 <= expected_port <= 18127:
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:expected_port_not_in_reserved_range")
if container.get("url") != f"http://127.0.0.1:{expected_port}":
    raise SystemExit("nanohorizon_e2e_not_ready:manifest:url_port_mismatch")
PY

CONTAINERS_ROOT="$CONTAINERS_ROOT" \
EVALS_ROOT="$EVALS_ROOT" \
GAMEBENCH_CRAFTAX_ROOT="$GAMEBENCH_ROOT" \
python3 "$NANOHORIZON_ROOT/scripts/validate_craftax_sources.py" \
  --catalog "$EVALS_ROOT/containers/images/craftax-gamebench-rust" \
  --containers-root "$CONTAINERS_ROOT" \
  --evals-root "$EVALS_ROOT" \
  --gamebench-root "$GAMEBENCH_ROOT" \
  --containers-revision "$CONTAINERS_REVISION" \
  --evals-revision "$EVALS_REVISION" \
  --gamebench-revision "$GAMEBENCH_REVISION"

oauth_seed="${SYNTH_DESKTOP_DEV_OAUTH_FILE:-${HOME:?}/.codex/auth.json}"
[[ -s "$oauth_seed" ]] || fail "chatgpt_auth:file_unavailable"
command -v docker >/dev/null 2>&1 || fail "docker:command_unavailable"

workshop_revision="$(git -C "$WORKSHOP_ROOT" rev-parse --verify HEAD)"
echo "NanoHorizon E2E preflight: ready"
echo "Workshop: $workshop_revision"
echo "Containers: $CONTAINERS_REVISION"
echo "NanoHorizon: $NANOHORIZON_REVISION"
echo "Evals: $EVALS_REVISION"
echo "GameBench: $GAMEBENCH_REVISION"
echo "Source manifest: $SOURCE_MANIFEST_DIGEST"
echo "Run contract: seeds 780000..780004; rollouts 5; calls/rollout 10; steps/rollout 2000; hard cap USD 2.45"
echo "Docker/provider execution remains authorization-required."
