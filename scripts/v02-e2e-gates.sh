#!/usr/bin/env bash
# Deterministic v0.2 E2E slice: family-truth node tests, MCP schema,
# Playwright [v0.2] specs, and Bombadil approval-card + grouped visual evidence.
#
# Does not spend paid budget. Does not replace W1–W3 CUA on an installed
# candidate. Fixture replay is not A1/A2/A8.
#
# Usage:
#   ./scripts/v02-e2e-gates.sh
#   ./scripts/v02-e2e-gates.sh --receipt path.json --tester "name"
#   ./scripts/desktop.sh conform && npm run desktop:check
#   npm run desktop:v02-e2e          # alias of this script
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/apps/synth_desktop"
SKIP_BUILD=0
RECEIPT=""
TESTER=""
while [[ "$#" -gt 0 ]]; do
	case "$1" in
		--no-build) SKIP_BUILD=1 ;;
		--receipt)
			[[ "$#" -ge 2 ]] || { echo "--receipt requires a path" >&2; exit 2; }
			RECEIPT="$2"
			shift
			;;
		--tester)
			[[ "$#" -ge 2 ]] || { echo "--tester requires a name" >&2; exit 2; }
			TESTER="$2"
			shift
			;;
		-h|--help)
			sed -n '2,16p' "$0"
			exit 0
			;;
		*)
			echo "unknown arg: $1" >&2
			exit 2
			;;
	esac
	shift
done
if [[ -n "$RECEIPT" && -z "$TESTER" ]]; then
	echo "--receipt requires --tester <name>" >&2
	exit 2
fi

if [[ -z "${SYNTH_PYTHON:-}" ]] && command -v python3 >/dev/null 2>&1; then
	export SYNTH_PYTHON="$(command -v python3)"
fi

info() { printf '\n\033[1m==> %s\033[0m\n' "$*"; }
ok() { printf '  \033[32m✓\033[0m %s\n' "$*"; }
bad() { printf '  \033[31m✗\033[0m %s\n' "$*"; }

FAILED=0
RENDERER_BUILT=0
RESULTS_FILE="$(mktemp "${TMPDIR:-/tmp}/synth-v02-gates.XXXXXX")"
trap 'rm -f "$RESULTS_FILE"' EXIT
record_gate() {
	printf '%s\t%s\n' "$1" "$2" >> "$RESULTS_FILE"
}
fail() {
	bad "$1"
	FAILED=1
}

SHA="$(git -C "$ROOT" rev-parse HEAD)"
STATUS="$(git -C "$ROOT" status --porcelain=v1 --untracked-files=all)"
CLEAN_STATE=1
[[ -n "$STATUS" ]] && CLEAN_STATE=0

write_receipt() {
	[[ -n "$RECEIPT" ]] || return 0
	mkdir -p "$(dirname "$RECEIPT")"
	RECEIPT_PATH="$RECEIPT" TESTER_NAME="$TESTER" COMMIT_SHA="$SHA" \
		CLEAN_STATE="$CLEAN_STATE" WORKTREE_STATUS="$STATUS" \
		RENDERER_BUILT="$RENDERER_BUILT" FAILED_COUNT="$FAILED" \
		RESULTS_PATH="$RESULTS_FILE" node <<'NODE'
const fs = require("node:fs");
const path = require("node:path");
const gates = fs.readFileSync(process.env.RESULTS_PATH, "utf8").trim().split("\n")
  .filter(Boolean)
  .map((line) => {
    const [name, status] = line.split("\t");
    return { name, status };
  });
const receipt = {
  schema: "synth.v02-e2e-gates.receipt.v1",
  generatedAt: new Date().toISOString(),
  tester: process.env.TESTER_NAME,
  commit: process.env.COMMIT_SHA,
  cleanState: process.env.CLEAN_STATE === "1",
  worktreeStatus: process.env.WORKTREE_STATUS ? process.env.WORKTREE_STATUS.split("\n") : [],
  rendererBuilt: process.env.RENDERER_BUILT === "1",
  gates,
  result: process.env.FAILED_COUNT === "0" ? "pass" : "fail"
};
const target = process.env.RECEIPT_PATH;
const temporary = `${target}.tmp-${process.pid}`;
fs.writeFileSync(temporary, `${JSON.stringify(receipt, null, 2)}\n`);
fs.renameSync(temporary, target);
console.log(`  · receipt: ${path.resolve(target)}`);
NODE
}

info "v0.2 deterministic E2E gates"
note_root() { printf '  · %s\n' "$*"; }
note_root "root: $ROOT"
note_root "these tests never claim paid A1–A8"

info "1/4 Visuals family truth"
if (cd "$ROOT" && node --experimental-strip-types --test visuals/tests/v02_family_truth.test.mjs visuals/tests/live_stream_contract.test.mjs visuals/tests/live_eval_reducer.test.mjs); then
	ok "visuals node tests"
	record_gate "visuals_family_truth" "pass"
else
	fail "visuals node tests"
	record_gate "visuals_family_truth" "fail"
fi

info "2/4 Desktop surface + MCP schema"
if (cd "$ROOT" && NODE_PATH="$ROOT/node_modules" node --test \
	apps/synth_desktop/tests/v02_mcp_schema.test.mjs \
	apps/synth_desktop/tests/v02_surface_invariants.test.mjs \
	apps/synth_desktop/tests/activity_presentation.test.mjs); then
	ok "desktop v0.2 node tests"
	record_gate "desktop_surface_mcp" "pass"
else
	fail "desktop v0.2 node tests"
	record_gate "desktop_surface_mcp" "fail"
fi

info "3/4 Playwright [v0.2]"
if (cd "$APP" && npx playwright test --config playwright.config.ts --reporter=line -g '\[v0\.2\]'); then
	ok "playwright v0.2"
	record_gate "playwright_v02" "pass"
else
	fail "playwright v0.2"
	record_gate "playwright_v02" "fail"
fi

info "4/4 Bombadil approval-card + grouped visual evidence"
GATE4_FAILED=0
if [[ "$SKIP_BUILD" -eq 0 ]]; then
	index="$APP/dist/index.html"
	need_build=1
	if [[ -f "$index" ]]; then
		newer="$(find "$APP/src" "$APP/index.html" "$APP/vite.config.ts" \
			"$APP/package.json" "$ROOT/package.json" "$ROOT/package-lock.json" "$ROOT/visuals" \
			\( -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.css' -o -name '*.html' \
				-o -name 'package.json' -o -name 'package-lock.json' \) \) \
			-newer "$index" 2>/dev/null | head -1 || true)"
		[[ -z "$newer" ]] && need_build=0
	fi
	if [[ "$need_build" -eq 1 ]]; then
		if (cd "$APP" && npm run frontend:build); then
			RENDERER_BUILT=1
		else
			fail "renderer build"
			GATE4_FAILED=1
		fi
	else
		note_root "renderer build fresh"
	fi
else
	note_root "renderer build explicitly skipped"
fi
for spec in approval-card.spec.ts grouped-visual-evidence.spec.ts; do
	if (cd "$APP" && BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/"$spec" node tests/bombadil/run.mjs); then
		ok "bombadil ${spec%.spec.ts}"
	else
		fail "bombadil ${spec%.spec.ts}"
		GATE4_FAILED=1
	fi
done
[[ "$GATE4_FAILED" -eq 0 ]] && record_gate "bombadil_evidence" "pass" || record_gate "bombadil_evidence" "fail"

echo
if [[ "$FAILED" -eq 0 ]]; then
	ok "v0.2 deterministic E2E slice green"
	note_root "CUA on an installed candidate is still required: docs/launch/v0.2-e2e-suite.md"
	write_receipt
	exit 0
fi
bad "v0.2 deterministic E2E slice red"
write_receipt
exit 1
