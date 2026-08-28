#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
harness_root="${DEEPSWE_HARNESS_DIR:-${repo_root}/../evals/temp/deepswe-harbor-codex}"
corpus_root="${DEEPSWE_SRC:-${repo_root}/../deep-swe}"

if [[ ! -f "${harness_root}/pyproject.toml" ]]; then
  echo "DeepSWE harness not found: ${harness_root}" >&2
  echo "Set DEEPSWE_HARNESS_DIR to the deepswe-harbor-codex checkout." >&2
  exit 2
fi
if [[ ! -f "${corpus_root}/tasks/manifest.json" ]]; then
  echo "DeepSWE corpus not found: ${corpus_root}" >&2
  echo "Set DEEPSWE_SRC to the pinned deep-swe checkout." >&2
  exit 2
fi

probe_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
probe_state="$(mktemp -d /tmp/deepswe-workshop-e2e.XXXXXX)"
probe_pid=""

cleanup() {
  if [[ -n "${probe_pid}" ]] && kill -0 "${probe_pid}" 2>/dev/null; then
    kill "${probe_pid}" 2>/dev/null || true
    wait "${probe_pid}" 2>/dev/null || true
  fi
  if [[ -d "${probe_state}" ]]; then
    find "${probe_state}" -depth -delete
  fi
}
trap cleanup EXIT INT TERM

(
  cd "${harness_root}"
  python3 -m pytest -q
  PYTHONPATH=src python3 -m deepswe_harbor_container.app serve \
    --host 127.0.0.1 \
    --port "${probe_port}" \
    --corpus-root "${corpus_root}" \
    --state-root "${probe_state}" \
    >"${probe_state}/service.log" 2>&1
) &
probe_pid="$!"

for _attempt in $(seq 1 100); do
  if curl --fail --silent "http://127.0.0.1:${probe_port}/health" >"${probe_state}/health.json"; then
    break
  fi
  if ! kill -0 "${probe_pid}" 2>/dev/null; then
    echo "DeepSWE facade exited before becoming ready" >&2
    sed -n '1,200p' "${probe_state}/service.log" >&2
    exit 1
  fi
  sleep 0.1
done

curl --fail --silent "http://127.0.0.1:${probe_port}/info" >"${probe_state}/info.json"
curl --fail --silent "http://127.0.0.1:${probe_port}/task_info" >"${probe_state}/task-info.json"
curl --fail --silent \
  -X POST "http://127.0.0.1:${probe_port}/task_instances/materialize" \
  -H 'content-type: application/json' \
  -d '{"task_id":"deep-swe-1-1","seeds":[780019],"limits":{"max_calls":80,"max_steps":1,"max_cost_usd":0.6}}' \
  >"${probe_state}/materialized.json"

python3 -c '
import json, pathlib, sys
root = pathlib.Path(sys.argv[1])
health = json.loads((root / "health.json").read_text())
info = json.loads((root / "info.json").read_text())
task = json.loads((root / "task-info.json").read_text())
materialized = json.loads((root / "materialized.json").read_text())
assert health["status"] == "ok"
assert health["paid_execution"] == "blocked"
assert info["capabilities"]["protocol"] == "synth.container.live-eval.v1"
assert info["provider_contract"]["operations"] == ["responses.create"]
assert info["provider_contract"]["credential_source"] == "workshop_ephemeral_secrets_proxy"
assert info["provider_contract"]["keychain_used"] is False
assert task["task_id"] == "deep-swe-1-1"
instances = materialized["instances"]
assert len(instances) == 1
instance = instances[0]
assert instance["seed"] == 780019
assert instance["task_instance_id"] == "deep-swe-1-1:seed:780019"
assert instance["model_identity"] == "openai/gpt-5.6-luna"
assert instance["limits"] == {"max_calls": 80, "max_steps": 1, "max_cost_usd": 0.6}
assert instance["task_digest"].startswith("sha256:")
assert instance["verifier_identity"]["digest"].startswith("sha256:")
' "${probe_state}"

kill "${probe_pid}" 2>/dev/null || true
wait "${probe_pid}" 2>/dev/null || true
probe_pid=""

(
  cd "${repo_root}/apps/synth_desktop/src-tauri"
  cargo test -p synth-desktop --lib optimizers::inline_eval::tests
  cargo test -p synth-desktop --lib deepswe_proxy_scope_and_lifetime_match_the_approved_campaign
  cargo test -p synth-desktop --lib limits::tests
  cargo test -p synth-desktop --lib secrets::capability::tests
  cargo test -p synth-desktop --lib a_cancelled_worker_cannot_be_rewritten_as_failed
  cargo test -p synth-desktop --lib drift_detected_at_dispatch_demands_a_new_approval_rather_than_a_patch
  cargo test -p synth-desktop --lib session::approval::tests
  cargo test -p synth-desktop --lib secrets::proxy::tests
  cargo test -p synth-desktop --lib export_specta_protocol_bindings -- --nocapture
)

(
  cd "${repo_root}"
  npm run typecheck --workspace @synth/synth-desktop
  node --test apps/synth_desktop/tests/v02_surface_invariants.test.mjs
  (
    cd apps/synth_desktop
    npx playwright test --config playwright.config.ts \
      tests/playwright/runtime-regressions.spec.ts \
      --grep 'approval modes configure new native sessions'
  )
)

echo "DeepSWE mocked Workshop/Harbor end-to-end gate passed."
