#!/usr/bin/env bash
# Workshop v0.2 Wave 0 — SynthStyle CONFORM CHECKS for apps/synth_desktop.
#
# Prints labeled baseline counts for every check in SynthStyle
# § WORKSHOP CONFORMANCE AUDIT → CONFORM CHECKS. Each count may only decrease
# over time. Patterns are relative to apps/synth_desktop (src-tauri/src,
# src/renderer/src); the item 31 boundary checks below also reach the
# repo-root visuals/ template package, which is the plugin side of the
# same boundaries.
#
# Usage (from repo root):
#   ./scripts/conform-desktop.sh
#   ./scripts/desktop.sh conform
#
# Paragons cited by Wave 0 scaffolding: objective_tests, real_fixtures
# (enforcement only — no product refactor in this script).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DESKTOP="$ROOT/apps/synth_desktop"

if [[ ! -d "$DESKTOP/src-tauri/src" || ! -d "$DESKTOP/src/renderer/src" ]]; then
  echo "[conform] missing apps/synth_desktop sources under $DESKTOP" >&2
  exit 1
fi

if ! command -v rg >/dev/null 2>&1; then
  echo "[conform] ripgrep (rg) is required" >&2
  exit 1
fi

# Sum rg -c output (path:count lines, or a bare count for a single file).
count_rg() {
  local pattern="$1"
  shift
  local out
  out="$(rg -c --no-messages "$pattern" "$@" 2>/dev/null || true)"
  if [[ -z "$out" ]]; then
    printf '%s' 0
    return
  fi
  printf '%s\n' "$out" | awk '
    {
      if (match($0, /:[0-9]+$/)) {
        s += substr($0, RSTART + 1) + 0
      } else if ($0 ~ /^[0-9]+$/) {
        s += $0 + 0
      }
    }
    END { print s + 0 }
  '
}

# Like count_rg, but blanks out in-file `#[cfg(test)]` modules before counting.
# Rust keeps its tests beside production code, so a `--glob` cannot separate
# them and a ratchet over the raw count moves whenever a test is added. The
# awk filter erases a top-level `#[cfg(test)] mod <name> { ... }` block (up to
# its column-0 closing brace) and passes every other line, including whole
# non-Rust files, through untouched.
count_rg_prod() {
  local pattern="$1"
  shift
  local files file total=0 n
  files="$(rg -l --no-messages "$pattern" "$@" 2>/dev/null || true)"
  if [[ -z "$files" ]]; then
    printf '%s' 0
    return
  fi
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    n="$(awk '
      /^#\[cfg\(test\)\]/ && !intest { pending = 1; print ""; next }
      pending {
        pending = 0
        if ($0 ~ /^(pub(\([^)]*\))? )?mod [A-Za-z_0-9]+ *\{/) { intest = 1; print ""; next }
      }
      intest { if ($0 ~ /^\}/) intest = 0; print ""; next }
      { print }
    ' "$file" | rg -c --no-messages "$pattern" || true)"
    total=$(( total + ${n:-0} ))
  done <<< "$files"
  printf '%s' "$total"
}

cd "$DESKTOP"

map_err_to_string="$(count_rg 'map_err\(\|e\| e\.to_string\(\)\)' src-tauri/src)"
to_string_contains="$(count_rg '\.to_string\(\)\.contains\(' src-tauri/src --glob '!**/*tests*.rs' --glob '!**/tests.rs' --glob '!**/tests/**')"
# Production error-path check; cfg(test) modules in *service.rs / *ingestion.rs still match —
# subtract known test-only assert sites that live beside production code.
to_string_contains_tests="$(rg -c --no-messages 'assert!.*\.to_string\(\)\.contains\(' src-tauri/src 2>/dev/null | awk -F: '{s+=$NF} END{print s+0}')"
to_string_contains=$(( to_string_contains > to_string_contains_tests ? to_string_contains - to_string_contains_tests : 0 ))

# Pattern as published in SynthStyle CONFORM CHECKS (Wave 1 magic status strings).
status_magic="$(count_rg 'status == "|"running"|"interrupted"' src-tauri/src/session/codex src-tauri/src/codex.rs)"
target_json_kind="$(count_rg 'target_json\["kind"\]|\bkind\b == "codex"|== "intern"' src-tauri/src)"
static_once_lock="$(count_rg 'static .*OnceLock' src-tauri/src)"
client_new="$(count_rg 'Client::new\(\)' src-tauri/src)"
window_synth="$(count_rg 'window\.synth' src/renderer/src --glob '!**/desktopBridge.ts')"
is_tauri_ternary="$(count_rg 'isTauri \?' src/renderer/src)"
env_d_ts_lines="$(wc -l < src/renderer/src/env.d.ts | tr -d '[:space:]')"
use_state_app="$(count_rg 'useState' src/renderer/src/App.tsx)"
invoke_string="$(count_rg 'invoke\("' src/renderer/src --glob '!**/generated/**')"

# Wave 2 boundary: specta-generated `commands.*` is the invoke surface.
# Event-channel names remain in protocolConstants.ts.

# ---------------------------------------------------------------------------
# Item 31 boundary checks. Each one names a module that owns a rule and counts
# the places outside it that re-derive the rule locally. All six use
# count_rg_prod so the number tracks production drift, not test volume.
# ---------------------------------------------------------------------------

# Boundary: the SYNTH_DESKTOP_DATA_ROOT -> instance data-root rule is resolved
# once, in instance_paths.rs (the constant and the resolution) and locked by
# instance.rs (the ID-R-10 test). Every other reader is a local copy of that
# rule and drifts exactly the way instance_paths.rs says local copies do.
# Baseline note: this was 1 -- visuals/templates.rs::user_templates_root() read
# the variable itself and fell back to visuals_root(), producing
# <root>/visuals/visuals/templates and a silently dead registry in every
# non-dev install. That is being fixed separately by routing it through
# instance::state_root(); once the fix lands the count is 0 and must stay 0.
# The two remaining hits in optimizers/{recipes,mlx_runtime}.rs are cfg(test)
# save/restore harnesses and are excluded by count_rg_prod, not by a glob.
data_root_env_outside="$(count_rg_prod 'env::var(_os)?\("SYNTH_DESKTOP_DATA_ROOT"\)' src-tauri/src --glob '!**/instance_paths.rs' --glob '!**/instance.rs')"

# Boundary: one envelope fold. `sequenceNumber` is the durable identity
# (runProgress/protocol.ts), and dedupe/replay/gap handling belongs in that
# fold plus runtime/optimizerEventCursor.ts -- not re-implemented per consumer.
# The pattern is deliberately narrow: a sequence compared against a cursor in
# an `if`/`while`, or a `seen` set keyed on a sequence. Bare reads, emissions,
# sorts, SQL columns and prose all stay out, which is why this counts real
# folds instead of every mention of the word.
sequence_fold_pattern='(if|while)\s*\(?\s*[a-z_.]*sequence(_number)?\s*(<=|>=|!=|==|>|<)\s*[*&]?[a-z_.]*(cursor|last_sequence|lastSequence)|\bseen[A-Za-z_]*(\.current)?\.(has|add|contains|insert|get|set)\([^)]*[sS]equence'
sequence_fold_outside="$(count_rg_prod "$sequence_fold_pattern" src-tauri/src src/renderer/src --glob '!visuals/runtime/**' --glob '!visuals/chrome/**' --glob '!visuals/tests/**' --glob '!**/runProgress/**' --glob '!**/optimizerEventCursor.ts' --glob '!**/generated/**' --glob '!**/*tests*.rs' --glob '!**/tests.rs' --glob '!**/tests/**')"

# Boundary: a sequence gap is a claim about a producer's sequence space, and
# exactly one implementation may make it. The idiom check above (sequence_fold_
# outside) matches the two shapes that existed when it was written; it misses a
# fold written in a new idiom, which is how src-tauri/src/visuals/stream_receipt.rs
# arrived without moving the count. This one matches the concept instead: the
# construction of a gap record. `visuals/runtime/` is today's canonical home and
# is excluded; when the fold moves to Rust (item 1) the exclusion moves with it
# and the TypeScript side must go to zero.
sequence_gap_pattern='(StreamGap|SequenceGap)\s*\{|gaps\.push\(\{\s*scope'
sequence_gap_outside="$(count_rg_prod "$sequence_gap_pattern" src-tauri/src src/renderer/src "$ROOT/visuals" --glob '!**/visuals/runtime/**' --glob '!**/tests/**' --glob '!**/*tests*.rs' --glob '!**/tests.rs')"

# Boundary: schema DDL lives in storage/migrations.rs, which is the only file
# allowed to say what a table looks like. A DDL const anywhere else is a second
# schema that no migration ever upgrades.
create_table_outside="$(count_rg_prod 'CREATE TABLE' src-tauri/src --glob '!**/storage/migrations.rs' --glob '!**/*tests*.rs' --glob '!**/tests.rs' --glob '!**/tests/**')"

# Boundary: templates are discovered through the visuals registry, not by the
# bundler. Every import.meta.glob is a build-time directory scan that decides
# at compile time what exists, so it cannot see a user-installed template and
# it silently changes what ships when a directory is renamed.
import_meta_glob="$(count_rg_prod 'import\.meta\.glob\(' src/renderer/src "$ROOT/visuals" --glob '!**/node_modules/**')"

# Boundary: the visual template registry root is derived only in
# visuals/templates.rs (visuals_root / user_templates_root). Anything else that
# joins its own path to the registry is the bug above waiting to happen again.
# The optimizer sidecar's own `home.join("templates")` is a different tree and
# is deliberately not matched.
template_root_join="$(count_rg_prod '\.join\("visuals"\)|"visuals/templates"' src-tauri/src --glob '!**/visuals/templates.rs')"

# Boundary: template ids are host vocabulary declared in contract/runtimes.rs.
# A hardcoded id elsewhere pins host code to a plugin that may not be
# installed. The pattern matches a fully quoted id only, so module specifiers
# like "@synth/visual-templates/.../optimizer.run.v1/..." and a template
# declaring its own id in its own template.json are not counted.
optimizer_template_id="$(count_rg_prod '"optimizer\.[a-z_0-9]+(\.[a-z_0-9]+)*\.v1"' src-tauri/src src/renderer/src --glob '!**/contract/runtimes.rs' --glob '!**/generated/**' --glob '!**/*tests*.rs' --glob '!**/tests.rs' --glob '!**/tests/**')"

cat <<EOF
[conform] apps/synth_desktop SynthStyle CONFORM CHECKS (counts may only decrease)
[conform] map_err_to_string          ${map_err_to_string}    # W6 → 0   rg map_err(|e| e.to_string())
[conform] to_string_contains         ${to_string_contains}    # W6 → 0   rg .to_string().contains(
[conform] status_magic_codex         ${status_magic}    # W1 → 0   rg status == "|"running"|"interrupted" codex.rs
[conform] target_json_kind           ${target_json_kind}    # W1 → 0   rg target_json["kind"]|kind == "codex"|== "intern"
[conform] static_once_lock           ${static_once_lock}    # W5 → 0   rg static .*OnceLock
[conform] client_new                 ${client_new}    # W5 → 0   rg Client::new()
[conform] window_synth               ${window_synth}    # W3 ↓     rg window.synth (excl. desktopBridge.ts)
[conform] is_tauri_ternary           ${is_tauri_ternary}    # W3/W9 → 0  rg 'isTauri ?'
[conform] env_d_ts_lines             ${env_d_ts_lines}    # W2 → <100  wc -l env.d.ts
[conform] use_state_app              ${use_state_app}    # W3 → <10  rg useState App.tsx
[conform] invoke_string              ${invoke_string}    # W2 → 0   rg invoke(" (excl. generated/)
[conform] data_root_env_outside      ${data_root_env_outside}    # 31 → 0   rg env::var(_os)("SYNTH_DESKTOP_DATA_ROOT") (excl. instance_paths.rs, instance.rs)
[conform] sequence_fold_outside      ${sequence_fold_outside}    # 31 → 0   rg sequence-vs-cursor / seen-set dedupe (excl. visuals/, runProgress/, optimizerEventCursor.ts)
[conform] sequence_gap_outside       ${sequence_gap_outside}    # 31 → 0   rg gap-record construction (excl. visuals/runtime = today's canonical fold)
[conform] create_table_outside       ${create_table_outside}    # 31 → 0   rg CREATE TABLE (excl. storage/migrations.rs)
[conform] import_meta_glob           ${import_meta_glob}    # 31 → 0   rg import.meta.glob( renderer + visuals/
[conform] template_root_join         ${template_root_join}    # 31 → 0   rg .join("visuals") | "visuals/templates" (excl. visuals/templates.rs)
[conform] optimizer_template_id      ${optimizer_template_id}    # 31 → 0   rg "optimizer.*.v1" literal (excl. contract/runtimes.rs)
EOF
