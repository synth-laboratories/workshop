#!/usr/bin/env bash
# Deterministic v0.2 E2E slice: family-truth node tests, MCP schema,
# Playwright [v0.2] specs, and Bombadil approval-card + grouped visual evidence.
#
# Does not spend paid budget. Does not replace W1–W3 CUA on an installed
# candidate. Fixture replay is not A1/A2/A8.
#
# Usage:
#   ./scripts/v02-e2e-gates.sh
#   npm run desktop:v02-e2e
#   ./scripts/v02-e2e-gates.sh --no-build
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/apps/synth_desktop"
SKIP_BUILD=0
for arg in "$@"; do
	case "$arg" in
		--no-build) SKIP_BUILD=1 ;;
		-h|--help)
			sed -n '2,14p' "$0"
			exit 0
			;;
		*)
			echo "unknown arg: $arg" >&2
			exit 2
			;;
	esac
done

export PATH="/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:${HOME}/.local/share/mise/installs/node/lts/bin:${HOME}/.local/share/mise/installs/python/3.12/bin:${HOME}/.cargo/bin:${PATH:-}"
if [[ -z "${SYNTH_PYTHON:-}" ]]; then
	for candidate in \
		"$HOME/.synth-desktop/laguna/.venv/bin/python" \
		"$HOME/.local/share/mise/installs/python/3.12/bin/python3" \
		"/opt/homebrew/bin/python3.12"
	do
		if [[ -x "$candidate" ]]; then
			export SYNTH_PYTHON="$candidate"
			break
		fi
	done
fi

info() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
bad() { printf '  \033[31m✗\033[0m %s\n' "$*"; }

FAILED=0
fail() {
	bad "$1"
	FAILED=1
}

info "v0.2 deterministic E2E gates"
note_root() { printf '  · %s\n' "$*"; }
note_root "root: $ROOT"
note_root "these tests never claim paid A1–A8"

info "1/4 Visuals family truth"
if (cd "$ROOT" && node --experimental-strip-types --test visuals/tests/v02_family_truth.test.mjs visuals/tests/live_stream_contract.test.mjs visuals/tests/live_eval_reducer.test.mjs); then
	ok "visuals node tests"
else
	fail "visuals node tests"
fi

info "2/4 Desktop surface + MCP schema"
if (cd "$ROOT" && NODE_PATH="$ROOT/node_modules" node --test \
	apps/synth_desktop/tests/v02_mcp_schema.test.mjs \
	apps/synth_desktop/tests/v02_surface_invariants.test.mjs \
	apps/synth_desktop/tests/activity_presentation.test.mjs); then
	ok "desktop v0.2 node tests"
else
	fail "desktop v0.2 node tests"
fi

info "3/4 Playwright [v0.2]"
if (cd "$APP" && npx playwright test --config playwright.config.ts --reporter=line -g '\[v0\.2\]'); then
	ok "playwright v0.2"
else
	fail "playwright v0.2"
fi

info "4/4 Bombadil approval-card + grouped visual evidence"
if [[ "$SKIP_BUILD" -eq 0 ]]; then
	index="$APP/dist/index.html"
	need_build=1
	if [[ -f "$index" ]]; then
		newer="$(find "$APP/src" "$APP/index.html" "$ROOT/visuals" \
			\( -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.css' -o -name '*.html' \) \) \
			-newer "$index" 2>/dev/null | head -1 || true)"
		[[ -z "$newer" ]] && need_build=0
	fi
	if [[ "$need_build" -eq 1 ]]; then
		(cd "$APP" && npm run frontend:build)
	else
		note_root "renderer build fresh"
	fi
fi
for spec in approval-card.spec.ts grouped-visual-evidence.spec.ts; do
	if (cd "$APP" && BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/"$spec" node tests/bombadil/run.mjs); then
		ok "bombadil ${spec%.spec.ts}"
	else
		fail "bombadil ${spec%.spec.ts}"
	fi
done

echo
if [[ "$FAILED" -eq 0 ]]; then
	ok "v0.2 deterministic E2E slice green"
	note_root "CUA on an installed candidate is still required: docs/launch/v0.2-e2e-suite.md"
	exit 0
fi
bad "v0.2 deterministic E2E slice red"
exit 1
