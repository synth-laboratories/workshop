#!/usr/bin/env bash
# Deterministic launch-gate subset (WP7). Live CRAFTAX-LUNA-010 and signed
# artifact checks are separate and fail closed when credentials/artifacts
# are missing — this script does not invent a pass for them.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EVALS_WORKSHOP="${EVALS_WORKSHOP:-$(cd "$ROOT/../evals/workshop" 2>/dev/null && pwd || true)}"
BACKEND="${BACKEND:-$(cd "$ROOT/../backend-desktop-account-snapshot" 2>/dev/null && pwd || true)}"

echo "== identity =="
git -C "$ROOT" rev-parse --short HEAD
git -C "$ROOT" status -sb

if [[ -n "${EVALS_WORKSHOP}" && -f "${EVALS_WORKSHOP}/package.json" ]]; then
  echo "== evals/workshop unit + typecheck =="
  (cd "$EVALS_WORKSHOP" && npm test && npm run typecheck)
else
  echo "evals/workshop not found at ${EVALS_WORKSHOP:-unset}; skip"
fi

if [[ -n "${BACKEND}" && -d "${BACKEND}/tests/units" ]]; then
  echo "== backend fake-Autumn + settlement units =="
  (cd "$BACKEND" && python -m pytest -m units \
    tests/units/test_fake_autumn_checkout.py \
    tests/units/test_desktop_account_snapshot.py \
    tests/units/test_public_inference_gateway_settlement.py)
else
  echo "backend snapshot not found at ${BACKEND:-unset}; skip"
fi

echo "== desktop rust units (tariffs + credential broker) =="
# cargo test takes a single TESTNAME filter, so run one module per invocation.
for module in tariffs credential_broker; do
  (cd "$ROOT/apps/synth_desktop/src-tauri" && cargo test --lib "$module" -- --nocapture)
done

echo "deterministic subset finished. Run gate:release + CRAFTAX-LUNA-010 + signed artifact separately."
