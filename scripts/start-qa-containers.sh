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

export SYNTH_CONTAINER_IMAGE_CATALOG="$IMAGES"

# The producing revision, per image directory.
#
# Naming bare HEAD for a dirty checkout is a lie of exactly the kind these
# gates exist to catch: the bytes serving requests are not the bytes at that
# commit, and a result that names the wrong source is not evidence. So a
# directory with uncommitted changes attests `<rev>-dirty-<digest>`, where the
# digest covers the working-tree content of that image directory. Two runs
# against the same edits agree; an edit between them does not.
#
# The digest is over the directory the server actually imports, not over the
# declaration's `include` list. Workshop's own launch check is scoped to the
# declared includes, which is the right scope for approving a *declaration*;
# it is the wrong scope for attesting what produced a *result*.
producer_revision() {
  local dir="$1"
  local status
  status="$(git -C "$EVALS" status --porcelain -- "containers/images/$dir")"
  if [ -z "$status" ]; then
    printf '%s' "$REV"
    return 0
  fi
  local digest
  digest="$(
    {
      git -C "$EVALS" ls-files -s -- "containers/images/$dir"
      git -C "$EVALS" diff -- "containers/images/$dir"
      git -C "$EVALS" diff --cached -- "containers/images/$dir"
    } | shasum -a 256 | awk '{print $1}'
  )"
  printf '%s-dirty-%s' "$REV" "${digest:0:12}"
}

start() {
  local name="$1" dir="$2" module="$3" port="$4"
  shift 4
  if lsof -ti "tcp:$port" >/dev/null 2>&1; then
    # A server already up may have been started without the identity
    # variables, in which case it answers /health but attests nothing and
    # admission refuses it. RESTART=1 replaces it rather than leaving a
    # silently unusable container in place.
    if [ "${RESTART:-0}" != "1" ]; then
      echo "[$name] already listening on :$port"
      return 0
    fi
    echo "[$name] replacing the server on :$port"
    lsof -ti "tcp:$port" | xargs -r kill
    for _ in $(seq 1 20); do
      lsof -ti "tcp:$port" >/dev/null 2>&1 || break
      sleep 0.5
    done
  fi
  local rev
  rev="$(producer_revision "$dir")"
  (
    cd "$IMAGES/$dir"
    # `env` because a variable assignment only counts as one when it is
    # literal at parse time; "$@" expands after that, so the per-task
    # assignments would otherwise be run as a command.
    nohup env \
      SYNTH_CONTAINER_PRODUCER_SOURCE_REVISION="$rev" \
      SYNTH_CONTAINER_INSTANCE_ID="$name-qa-$port" \
      SYNTH_CONTAINER_IMAGE_DIGEST="sha256:$(printf '%s' "$rev-$name" | shasum -a 256 | awk '{print $1}')" \
      PYTHONPATH=. "$@" "$PY" -m "$module" --port "$port" >"$LOGS/$name.log" 2>&1 &
  )
  echo "[$name] starting on :$port as $rev"
}

start banking77 banking77 banking77_classify 8099
# The hosted SFT recipe binds checkpoint evaluation to a fixed slot --
# `LOCAL_BANKING77_SLOT` in hosted_sft.rs is `http://127.0.0.1:8110` -- and
# cancels the run when nothing serves it, naming a port the operator never
# chose. Serving that slot is cheaper than overriding
# SYNTH_CONTAINERS_BANKING77_URL, which would need an app restart to take.
start banking77-sft banking77 banking77_classify 8110
start craftax craftax-gamebench-rust craftax_gold 8097 \
  SYNTH_CRAFTAX_GOLD_BIN="$HOME/GitHub/gamebench/tasks/craftax-singleplayer/gold_rust/target/release/craftax_gold"
start healthbench healthbench2 healthbench_chat 8114 \
  HEALTHBENCH_GRADER_PROVIDER=openrouter \
  OPENROUTER_API_KEY="$(grep -m1 '^OPENROUTER_API_KEY=' "$HOME/.synth-desktop/.env" | cut -d= -f2-)"

for port in 8099 8110 8097 8114; do
  for _ in $(seq 1 40); do
    code="$(curl -s -m 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:$port/health" || true)"
    [ "$code" = "200" ] && break
    sleep 1
  done
  echo ":$port -> ${code:-000}"
done
