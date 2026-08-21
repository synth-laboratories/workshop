#!/usr/bin/env bash
# Cookbooks are reference-only. Workshop does not stage, bundle, or spawn them.
# Copy templates into a workspace with `$author-synth-container`.
set -euo pipefail
echo "stage-packaged-cookbooks.sh: no-op (cookbooks are not packaged into Workshop)" >&2
exit 0
