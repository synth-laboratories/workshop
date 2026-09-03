#!/usr/bin/env bash
# Assemble the QA instance workspace from tracked sources.
#
# The workspace is what Workshop reads to answer three questions: which
# containers may be driven (workshop.containers.toml), which recipes exist
# (workshop.recipes/), and where a live annotation protocol comes from
# (domains/<task>/annotations/). Every one of those was hand-assembled inside
# an instance data directory, tracked by nothing -- so a lost or rebuilt
# instance silently lost the container identities, and admission failures
# looked like product bugs rather than a missing file.
#
# Declarations live in this repo; image and protocol *content* stays in the
# evals checkout and is copied in. The copies are real directories rather than
# symlinks because Workshop resolves working_directory under the workspace and
# follows links when it checks that a path is genuinely inside it.
#
#   materialize-qa-workspace.sh <instance>
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EVALS="${EVALS_ROOT:-$HOME/GitHub/evals}"
INSTANCE="${1:-}"
if [[ -z "$INSTANCE" ]]; then
  echo "usage: materialize-qa-workspace.sh <instance>" >&2
  exit 2
fi
WORKSPACE="$HOME/.synth-desktop/instances/v09/$INSTANCE/workspace"

if [[ ! -d "$EVALS/containers/images" ]]; then
  echo "[qa-workspace] evals checkout not found at $EVALS" >&2
  echo "[qa-workspace] set EVALS_ROOT to the checkout that holds containers/images" >&2
  exit 1
fi

mkdir -p "$WORKSPACE/workshop.recipes" "$WORKSPACE/containers" "$WORKSPACE/domains"

# Container declarations: the operator's statement that a URL may be driven.
cp "$ROOT/qa/workshop.containers.toml" "$WORKSPACE/workshop.containers.toml"

# Recipes come from two tracked locations: the annotated evals ship with the
# app, the GEPA QA recipes are workspace-only.
cp "$ROOT/apps/synth_desktop/src-tauri/recipes/annotation_eval/"*.toml "$WORKSPACE/workshop.recipes/"
cp "$ROOT/workshop.recipes/"*.toml "$WORKSPACE/workshop.recipes/"

# Image directories named by each declaration's working_directory. Only
# image.toml is in the launch `include` set, but the whole directory is copied
# so the declaration resolves and so a reader can see what the service is.
for image in banking77 healthbench2 craftax-gamebench-rust; do
  rm -rf "${WORKSPACE:?}/containers/$image"
  cp -R "$EVALS/containers/images/$image" "$WORKSPACE/containers/$image"
done

# Live annotation protocols. `protocol_source` in the *_live_annotated recipes
# is workspace-relative, and admission refuses a recipe whose protocol source
# does not resolve -- which is not obviously a missing-file problem from the
# error text alone.
for task in banking77 craftax healthbench; do
  src="$EVALS/domains/$task/annotations"
  [[ -d "$src" ]] || continue
  mkdir -p "$WORKSPACE/domains/$task"
  rm -rf "${WORKSPACE:?}/domains/$task/annotations"
  cp -R "$src" "$WORKSPACE/domains/$task/annotations"
done

echo "[qa-workspace] materialized $WORKSPACE"
printf '[qa-workspace] recipes=%s containers=%s protocols=%s\n' \
  "$(ls "$WORKSPACE/workshop.recipes" | wc -l | tr -d ' ')" \
  "$(ls "$WORKSPACE/containers" | wc -l | tr -d ' ')" \
  "$(find "$WORKSPACE/domains" -name live_protocol.py | wc -l | tr -d ' ')"
