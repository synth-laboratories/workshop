#!/usr/bin/env bash
set -euo pipefail
example_root="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$example_root/../.." && pwd)"
cd "$example_root"
source "$repo_root/scripts/local-toolchain-env.sh"
export DOCKER_CONFIG="$example_root/docker-config"
case "${1:-help}" in
  setup)
    command -v uv >/dev/null || { echo 'Run scripts/install.sh --bootstrap first.' >&2; exit 1; }
    node build_deps.mjs >/dev/null || { echo 'Run scripts/install.sh first.' >&2; exit 1; }
    [[ -x .venv/bin/python ]] || uv venv --python 3.12 .venv
    uv pip sync --python .venv/bin/python --require-hashes requirements.lock
    if ! command -v ffmpeg >/dev/null; then
      command -v brew >/dev/null || { echo 'Install ffmpeg before replaying game frames.' >&2; exit 1; }
      brew install ffmpeg
    fi
    ;;
  run)
    [[ -x .venv/bin/python ]] || { echo 'Run bash examples/runebench/example.sh setup first.' >&2; exit 1; }
    docker info >/dev/null || { echo 'Start Docker Desktop or OrbStack and retry.' >&2; exit 1; }
    docker compose version >/dev/null
    shift
    .venv/bin/python run.py --duration 25 --calls 3 "$@"
    .venv/bin/python query.py --build
    node build_web.mjs
    echo 'Run finished. Start review with: bash examples/runebench/example.sh review'
    ;;
  sample)
    [[ -x .venv/bin/python ]] || { echo 'Run setup first.' >&2; exit 1; }
    .venv/bin/python sample.py
    .venv/bin/python query.py --build
    node build_web.mjs
    echo 'Sample ready. Run: bash examples/runebench/example.sh review'
    ;;
  rebuild)
    .venv/bin/python query.py --build
    node build_web.mjs
    ;;
  review)
    [[ -f web/index.html ]] || { echo 'No review built yet. Run sample or run first.' >&2; exit 1; }
    command -v ffmpeg >/dev/null || { echo 'Install ffmpeg for video frames (brew install ffmpeg).' >&2; exit 1; }
    echo 'Open http://127.0.0.1:8128/viewer — Ctrl-C stops only this review server.'
    .venv/bin/python serve.py
    ;;
  *)
    echo 'Usage: bash examples/runebench/example.sh setup | sample | run [--scripted] | rebuild | review'
    echo 'Default run: four scripted actors, 25 seconds, zero model calls.'
    echo 'Optional paid mode: run --provider openrouter --env-file /private/path/.env'
    ;;
esac
