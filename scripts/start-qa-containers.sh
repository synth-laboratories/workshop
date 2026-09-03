#!/usr/bin/env bash
# Start the QA task containers with the identity Workshop admission requires.
#
# Workshop refuses to admit paid work against a container that cannot say what
# produced it -- "container declaration has no immutable source revision". A
# bare `python -m <task>` satisfies /health but not that gate, so every server
# here declares its producing revision, a stable instance id, and its image id.
set -euo pipefail

EVALS="${EVALS_ROOT:-$HOME/GitHub/evals}"
PY="$EVALS/.venv/bin/python"
IMAGES="$EVALS/containers/images"
LOGS="${CONTAINER_LOG_DIR:-${TMPDIR:-/tmp}/synth-qa-containers}"
mkdir -p "$LOGS"
REV="$(git -C "$EVALS" rev-parse HEAD)"

export SYNTH_CONTAINER_PRODUCER_SOURCE_REVISION="$REV"
export SYNTH_CONTAINER_IMAGE_CATALOG="$IMAGES"

start() {
  local name="$1" dir="$2" module="$3" port="$4"
  shift 4
  if lsof -ti "tcp:$port" >/dev/null 2>&1; then
    echo "[$name] already listening on :$port"
    return 0
  fi
  (
    cd "$IMAGES/$dir"
    # `env` because a variable assignment only counts as one when it is
    # literal at parse time; "$@" expands after that, so the per-task
    # assignments would otherwise be run as a command.
    nohup env \
      SYNTH_CONTAINER_INSTANCE_ID="$name-qa-$port" \
      SYNTH_CONTAINER_IMAGE_DIGEST="sha256:$(printf '%s' "$REV-$name" | shasum -a 256 | awk '{print $1}')" \
      PYTHONPATH=. "$@" "$PY" -m "$module" --port "$port" >"$LOGS/$name.log" 2>&1 &
  )
  echo "[$name] starting on :$port"
}

start banking77 banking77 banking77_classify 8099
start craftax craftax-gamebench-rust craftax_gold 8097 \
  SYNTH_CRAFTAX_GOLD_BIN="$HOME/GitHub/gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold"
start healthbench healthbench2 healthbench_chat 8114 \
  HEALTHBENCH_GRADER_PROVIDER=openrouter \
  OPENROUTER_API_KEY="$(grep -m1 '^OPENROUTER_API_KEY=' "$HOME/.synth-desktop/.env" | cut -d= -f2-)"

for port in 8099 8097 8114; do
  for _ in $(seq 1 40); do
    code="$(curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/health" || true)"
    [ "$code" = "200" ] && break
    sleep 1
  done
  echo ":$port -> ${code:-000}"
done
