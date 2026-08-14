#!/usr/bin/env bash
# Default Workshop Desktop UI gate runner — streams Bombadil + Playwright live,
# writes logs, and prints a clean issues table at the end.
#
# Usage:
#   ./scripts/desktop-ui-gates.sh                 # bombadil + playwright (default, streams)
#   npm run desktop:ui-gates                      # same from repo root
#   npm run test:ui-gates --workspace @synth/synth-desktop
#   ./scripts/desktop-ui-gates.sh bombadil        # bombadil only
#   ./scripts/desktop-ui-gates.sh playwright      # playwright only
#   ./scripts/desktop-ui-gates.sh --no-build      # skip vite rebuild
#   ./scripts/desktop-ui-gates.sh --quiet         # logs only (no live stream)
#
# Env:
#   BOMBADIL_TIME_LIMIT   optional override (otherwise run.mjs defaults: 5s/10s)
#   BOMBADIL_JOBS         parallel bombadil specs (default 4)
#   SYNTH_PYTHON          preferred python for local-runtime (3.12+)
#   PLAYWRIGHT_GREP       optional -g filter for playwright
#   PLAYWRIGHT_WORKERS    playwright workers (default 8 local / 4 CI)
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/apps/synth_desktop"
OUT_ROOT="${DESKTOP_UI_GATES_OUT:-$ROOT/apps/synth_desktop/test-results/ui-gates}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="$OUT_ROOT/$STAMP"
BOMBADIL_JOBS="${BOMBADIL_JOBS:-4}"
ISSUES_TSV="$RUN_DIR/issues.tsv"

MODE="all"
SKIP_BUILD=0
QUIET=0
for arg in "$@"; do
	case "$arg" in
		bombadil|playwright|all) MODE="$arg" ;;
		--no-build) SKIP_BUILD=1 ;;
		--quiet|-q) QUIET=1 ;;
		-h|--help)
			sed -n '2,22p' "$0"
			exit 0
			;;
		*)
			echo "unknown arg: $arg" >&2
			exit 2
			;;
	esac
done

mkdir -p "$RUN_DIR"
: >"$ISSUES_TSV"
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
note() { printf '  · %s\n' "$*"; }

# suite | kind | id | detail
record_issue() {
	local suite="$1" kind="$2" id="$3" detail="${4:-}"
	detail="$(printf '%s' "$detail" | tr '\n' ' ' | sed -E 's/[[:space:]]+/ /g')"
	printf '%s\t%s\t%s\t%s\n' "$suite" "$kind" "$id" "$detail" >>"$ISSUES_TSV"
}

# Run a command, always capturing to $1; stream to terminal unless --quiet.
# Important: never flip `set -e` back on before returning — errexit is
# shell-global, so a non-zero return would abort the whole gates suite.
run_stream() {
	local log="$1"
	shift
	local code=0
	if [[ "$QUIET" -eq 1 ]]; then
		"$@" >"$log" 2>&1
		code=$?
	else
		"$@" 2>&1 | tee "$log"
		code=${PIPESTATUS[0]}
	fi
	return "$code"
}

BOMBADIL_SPECS=(
	layout.spec.ts
	visual-alignment.spec.ts
	visual-library-layout.spec.ts
	visual-pane-boundaries.spec.ts
	approval-card.spec.ts
	grouped-visual-evidence.spec.ts
	trace-catalog-layout.spec.ts
	shell-containment.spec.ts
	inference-state-honesty.spec.ts
	composer-surfaces.spec.ts
	composer-toolbar.spec.ts
	reasoning-disclosure.spec.ts
	launch-debt.spec.ts
	empty-completed-turn.spec.ts
	empty-outputs.spec.ts
	run-summary-sanity.spec.ts
	model-menu-polish.spec.ts
	mander-boundaries.spec.ts
)

BOMBADIL_EXIT=0
PLAYWRIGHT_EXIT=0
BOMBADIL_RED_SPECS=0
BOMBADIL_RED_PROPERTIES=0
BOMBADIL_RED_HARNESS=0
BOMBADIL_GREEN_SPECS=0
PLAYWRIGHT_PASSED="—"
PLAYWRIGHT_FAILED="—"
PLAYWRIGHT_FLAKY="—"
PLAYWRIGHT_SKIPPED="—"

frontend_build_is_stale() {
	local index="$APP/dist/index.html"
	[[ -f "$index" ]] || return 0
	# Any tracked input newer than the last build → rebuild.
	local newer
	newer="$(find \
		"$APP/src" \
		"$APP/index.html" \
		"$APP/vite.config.ts" \
		"$APP/package.json" \
		"$APP/tsconfig.json" \
		"$ROOT/packages" \
		"$ROOT/visuals" \
		\( -type f \( -name '*.ts' -o -name '*.tsx' -o -name '*.css' -o -name '*.json' -o -name '*.html' -o -name '*.mjs' \) \) \
		-newer "$index" \
		2>/dev/null | head -1 || true)"
	[[ -n "$newer" ]]
}

run_build() {
	if [[ "$SKIP_BUILD" -eq 1 ]]; then
		note "skipping frontend build (--no-build)"
		return
	fi
	info "Frontend build"
	if ! frontend_build_is_stale; then
		ok "renderer build fresh — skipping vite (use a touch under apps/synth_desktop/src to force)"
		return
	fi
	if (cd "$APP" && npm run frontend:build) >"$RUN_DIR/frontend-build.log" 2>&1; then
		ok "renderer build ready  (log: $RUN_DIR/frontend-build.log)"
	else
		bad "frontend build failed  (log: $RUN_DIR/frontend-build.log)"
		tail -n 40 "$RUN_DIR/frontend-build.log" || true
		record_issue "build" "harness" "frontend:build" "vite build failed; see frontend-build.log"
		exit 1
	fi
}

# Score one bombadil log into counters + issues. Args: spec log exit_code
score_bombadil_spec() {
	local spec="$1" log="$2" code="$3" summary="$4"
	local viols count harness=0
	# Full-horizon runs reprint the same always() violation every tick —
	# count distinct property names, not every reprint.
	viols="$(grep -E 'was violated:' "$log" | sed -E 's/[[:space:]]*was violated:.*//' | sed -E 's/^[[:space:]]+//' | sort -u || true)"
	count=0
	if [[ -n "$viols" ]]; then
		count="$(printf '%s\n' "$viols" | grep -c . || true)"
	fi
	local harness_msg=""
	if grep -q 'Bombadil exceeded its test limit plus startup grace' "$log"; then
		harness_msg="watchdog (hung past time limit + grace)"
	elif grep -q 'Isolated Synth runtime did not become healthy' "$log"; then
		harness_msg="runtime failed to become healthy"
	elif grep -q 'No renderer build found' "$log"; then
		harness_msg="missing frontend build"
	elif [[ "$code" -ne 0 && "$count" -eq 0 ]]; then
		harness_msg="non-zero exit without property violation (see log)"
	fi

	if [[ "$code" -eq 0 && "$count" -eq 0 && -z "$harness_msg" ]]; then
		ok "$spec  (exit 0, 0 violations)"
		BOMBADIL_GREEN_SPECS=$((BOMBADIL_GREEN_SPECS + 1))
	else
		bad "$spec  (exit $code, $count property violation(s)${harness_msg:+, harness: $harness_msg})"
		BOMBADIL_RED_SPECS=$((BOMBADIL_RED_SPECS + 1))
		BOMBADIL_RED_PROPERTIES=$((BOMBADIL_RED_PROPERTIES + count))
		[[ -n "$harness_msg" ]] && BOMBADIL_RED_HARNESS=$((BOMBADIL_RED_HARNESS + 1))
		while IFS= read -r line; do
			[[ -z "$line" ]] && continue
			note "RED property  $line"
			record_issue "bombadil" "property" "$line" "$spec"
		done <<<"$viols"
		if [[ -n "$harness_msg" ]]; then
			note "RED harness   $harness_msg"
			record_issue "bombadil" "harness" "$spec" "$harness_msg"
		fi
	fi
	{
		echo "--- $spec ---"
		echo "exit=$code violations=$count harness=${harness_msg:-none}"
		printf '%s\n' "$viols"
		echo
	} >>"$summary"
}

run_bombadil() {
	local limit_note="${BOMBADIL_TIME_LIMIT:-run.mjs defaults (5s/10s)}"
	info "Bombadil suite (time limit ${limit_note}, jobs=${BOMBADIL_JOBS})"
	note "python=${SYNTH_PYTHON:-python3}"
	note "logs → $RUN_DIR/bombadil/"
	note "parallel jobs=${BOMBADIL_JOBS} (set BOMBADIL_JOBS=1 for serial live streams)"
	mkdir -p "$RUN_DIR/bombadil"
	local summary="$RUN_DIR/bombadil/summary.txt"
	: >"$summary"
	BOMBADIL_RED_SPECS=0
	BOMBADIL_RED_PROPERTIES=0
	BOMBADIL_RED_HARNESS=0
	BOMBADIL_GREEN_SPECS=0

	local specs=()
	local spec
	for spec in "${BOMBADIL_SPECS[@]}"; do
		if [[ -f "$APP/tests/bombadil/$spec" ]]; then
			specs+=("$spec")
		else
			note "skip missing spec $spec"
		fi
	done
	local total=${#specs[@]}
	if [[ "$total" -eq 0 ]]; then
		bad "no bombadil specs found"
		BOMBADIL_EXIT=1
		return
	fi

	# Parallel path: capture per-spec logs (interleaved chromium spam is useless).
	# Serial path (JOBS=1): stream live like before.
	if [[ "$BOMBADIL_JOBS" -le 1 ]]; then
		local idx=0
		for spec in "${specs[@]}"; do
			idx=$((idx + 1))
			local log="$RUN_DIR/bombadil/$spec.log"
			printf '\n--- [%s/%s] %s ---\n' "$idx" "$total" "$spec" | tee -a "$summary"
			set +e
			local env_args=(
				"BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/$spec"
				"BOMBADIL_OUTPUT_PATH=$RUN_DIR/bombadil/$spec.out"
			)
			[[ -n "${BOMBADIL_TIME_LIMIT:-}" ]] && env_args+=("BOMBADIL_TIME_LIMIT=$BOMBADIL_TIME_LIMIT")
			run_stream "$log" env "${env_args[@]}" bash -c "cd \"$APP\" && node tests/bombadil/run.mjs"
			local code=$?
			set -e
			score_bombadil_spec "$spec" "$log" "$code" "$summary"
		done
	else
		note "starting $total specs with xargs -P${BOMBADIL_JOBS}…"
		# macOS /bin/bash is 3.2 (no wait -n). xargs -P is the portable throttle.
		export APP RUN_DIR
		# shellcheck disable=SC2016
		printf '%s\0' "${specs[@]}" | xargs -0 -P "$BOMBADIL_JOBS" -I{} bash -c '
			spec="$1"
			log="$RUN_DIR/bombadil/${spec}.log"
			exit_file="$RUN_DIR/bombadil/${spec}.exit"
			echo "  · start $spec"
			set +e
			env_cmd=(env
				"BOMBADIL_SPEC=apps/synth_desktop/tests/bombadil/${spec}"
				"BOMBADIL_OUTPUT_PATH=$RUN_DIR/bombadil/${spec}.out"
			)
			if [[ -n "${BOMBADIL_TIME_LIMIT:-}" ]]; then
				env_cmd+=("BOMBADIL_TIME_LIMIT=$BOMBADIL_TIME_LIMIT")
			fi
			(cd "$APP" && "${env_cmd[@]}" node tests/bombadil/run.mjs) >"$log" 2>&1
			echo $? >"$exit_file"
			echo "  · done  $spec (exit $(cat "$exit_file"))"
		' _ {}
		for spec in "${specs[@]}"; do
			local code=1
			local exit_file="$RUN_DIR/bombadil/$spec.exit"
			[[ -f "$exit_file" ]] && code="$(cat "$exit_file")"
			score_bombadil_spec "$spec" "$RUN_DIR/bombadil/$spec.log" "$code" "$summary"
		done
	fi

	{
		echo
		echo "BOMBADIL_RED_SPECS=$BOMBADIL_RED_SPECS"
		echo "BOMBADIL_RED_PROPERTIES=$BOMBADIL_RED_PROPERTIES"
		echo "BOMBADIL_RED_HARNESS=$BOMBADIL_RED_HARNESS"
		echo "BOMBADIL_GREEN_SPECS=$BOMBADIL_GREEN_SPECS"
		echo "BOMBADIL_TOTAL_SPECS=$total"
	} | tee -a "$summary"

	info "Bombadil red tally"
	note "red specs:        $BOMBADIL_RED_SPECS / $total"
	note "red properties:   $BOMBADIL_RED_PROPERTIES"
	note "red harness:      $BOMBADIL_RED_HARNESS"
	note "green specs:      $BOMBADIL_GREEN_SPECS"
	BOMBADIL_EXIT=0
	[[ "$BOMBADIL_RED_PROPERTIES" -eq 0 && "$BOMBADIL_RED_HARNESS" -eq 0 && "$BOMBADIL_RED_SPECS" -eq 0 ]] || BOMBADIL_EXIT=1
}

run_playwright() {
	info "Playwright suite"
	note "logs → $RUN_DIR/playwright.log"
	note "workers=${PLAYWRIGHT_WORKERS:-8 (local default)} — fullyParallel, isolated Vite ports"
	if [[ "$QUIET" -eq 1 ]]; then
		note "quiet mode — live output silenced; see playwright.log"
	else
		note "streaming live (pass --quiet to silence)"
	fi
	set +e
	if [[ -n "${PLAYWRIGHT_GREP:-}" ]]; then
		note "filter: $PLAYWRIGHT_GREP"
		run_stream "$RUN_DIR/playwright.log" bash -c \
			"cd \"$APP\" && FORCE_COLOR=\"\${FORCE_COLOR:-0}\" npx playwright test --config playwright.config.ts --reporter=line -g \"$PLAYWRIGHT_GREP\""
	else
		run_stream "$RUN_DIR/playwright.log" bash -c \
			"cd \"$APP\" && FORCE_COLOR=\"\${FORCE_COLOR:-0}\" npx playwright test --config playwright.config.ts --reporter=line"
	fi
	PLAYWRIGHT_EXIT=$?
	set -e

	PLAYWRIGHT_PASSED="$(grep -Eo '[0-9]+ passed' "$RUN_DIR/playwright.log" | tail -1 | awk '{print $1}' || true)"
	PLAYWRIGHT_FAILED="$(grep -Eo '[0-9]+ failed' "$RUN_DIR/playwright.log" | tail -1 | awk '{print $1}' || true)"
	PLAYWRIGHT_FLAKY="$(grep -Eo '[0-9]+ flaky' "$RUN_DIR/playwright.log" | tail -1 | awk '{print $1}' || true)"
	PLAYWRIGHT_SKIPPED="$(grep -Eo '[0-9]+ skipped' "$RUN_DIR/playwright.log" | tail -1 | awk '{print $1}' || true)"
	PLAYWRIGHT_PASSED="${PLAYWRIGHT_PASSED:-0}"
	PLAYWRIGHT_FAILED="${PLAYWRIGHT_FAILED:-0}"
	PLAYWRIGHT_FLAKY="${PLAYWRIGHT_FLAKY:-0}"
	PLAYWRIGHT_SKIPPED="${PLAYWRIGHT_SKIPPED:-0}"

	{
		echo "PLAYWRIGHT_EXIT=$PLAYWRIGHT_EXIT"
		echo "PLAYWRIGHT_PASSED=$PLAYWRIGHT_PASSED"
		echo "PLAYWRIGHT_FAILED=$PLAYWRIGHT_FAILED"
		echo "PLAYWRIGHT_FLAKY=$PLAYWRIGHT_FLAKY"
		echo "PLAYWRIGHT_SKIPPED=$PLAYWRIGHT_SKIPPED"
	} >"$RUN_DIR/playwright-summary.txt"

	info "Playwright tally"
	if [[ "$PLAYWRIGHT_EXIT" -eq 0 ]]; then
		ok "exit 0 — passed=$PLAYWRIGHT_PASSED failed=$PLAYWRIGHT_FAILED flaky=$PLAYWRIGHT_FLAKY skipped=$PLAYWRIGHT_SKIPPED"
	else
		bad "exit $PLAYWRIGHT_EXIT — passed=$PLAYWRIGHT_PASSED failed=$PLAYWRIGHT_FAILED flaky=$PLAYWRIGHT_FLAKY skipped=$PLAYWRIGHT_SKIPPED"
	fi

	local titles=""
	titles="$(grep -E '^\s+[0-9]+\)\s' "$RUN_DIR/playwright.log" | sed -E 's/^[[:space:]]*[0-9]+\)[[:space:]]+//' || true)"
	if [[ -z "$titles" ]]; then
		titles="$(grep -E '^\s+✘|^\s+×' "$RUN_DIR/playwright.log" | head -40 || true)"
	fi
	if [[ "$PLAYWRIGHT_EXIT" -ne 0 ]] && grep -q 'No tests found' "$RUN_DIR/playwright.log"; then
		record_issue "playwright" "harness" "playwright-runner" "No tests found${PLAYWRIGHT_GREP:+ (filter: $PLAYWRIGHT_GREP)}"
		titles=""
	elif [[ "$PLAYWRIGHT_EXIT" -ne 0 && -z "$titles" && ! -s "$RUN_DIR/playwright.log" ]]; then
		record_issue "playwright" "harness" "playwright-runner" "no log produced; runner likely aborted before tests"
	elif [[ "$PLAYWRIGHT_EXIT" -ne 0 && -z "$titles" ]]; then
		record_issue "playwright" "harness" "playwright-runner" "exit $PLAYWRIGHT_EXIT with unparsed failures; see playwright.log"
	fi
	while IFS= read -r line; do
		[[ -z "$line" ]] && continue
		note "RED test  $line"
		record_issue "playwright" "test" "$line" "failed"
	done <<<"$titles"
}

print_issues_table() {
	info "Issues table"
	if [[ ! -s "$ISSUES_TSV" ]]; then
		ok "no issues recorded"
		return
	fi
	printf '  %-10s  %-10s  %-52s  %s\n' "SUITE" "KIND" "ID" "DETAIL"
	printf '  %-10s  %-10s  %-52s  %s\n' "----------" "----------" "----------------------------------------------------" "------"
	while IFS=$'\t' read -r suite kind id detail; do
		local id_show="$id"
		if [[ ${#id_show} -gt 52 ]]; then
			id_show="${id_show:0:49}..."
		fi
		printf '  %-10s  %-10s  %-52s  %s\n' "$suite" "$kind" "$id_show" "$detail"
	done <"$ISSUES_TSV"
	note "full TSV: $ISSUES_TSV"
}

FINAL_EXIT=0
info "Desktop UI gates — $STAMP"
note "root: $ROOT"
note "out:  $RUN_DIR"
note "mode: $MODE"
note "stream: $([[ "$QUIET" -eq 1 ]] && echo quiet || echo live)"

run_build

case "$MODE" in
	bombadil)
		run_bombadil
		FINAL_EXIT=$BOMBADIL_EXIT
		;;
	playwright)
		run_playwright
		FINAL_EXIT=$PLAYWRIGHT_EXIT
		;;
	all)
		run_bombadil
		run_playwright
		if [[ "$BOMBADIL_EXIT" -ne 0 || "$PLAYWRIGHT_EXIT" -ne 0 ]]; then
			FINAL_EXIT=1
		fi
		;;
esac

print_issues_table

info "Done"
note "artifacts: $RUN_DIR"
if [[ "$FINAL_EXIT" -eq 0 ]]; then
	ok "all selected gates green"
else
	bad "one or more gates red (exit $FINAL_EXIT)"
fi
exit "$FINAL_EXIT"
