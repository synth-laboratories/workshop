# templates-internal/ — private visual templates

Staged, never authored here. Source of truth is
`~/.synth-desktop/visuals/templates/`, with a legacy fallback described below.

    ./scripts/stage-internal-visuals.sh          # copy private templates in (roots below)
    ./scripts/stage-internal-visuals.sh --clean  # remove them again
    ./scripts/stage-internal-visuals.sh --check  # exit 1 if anything is staged

Templates are **copied, not symlinked** — this README said "symlink" and the
script has never done that. A shell imports its chrome by relative path
(`../../chrome/VisualChrome.tsx`), and bundlers resolve a symlinked file from its
real path, so through a symlink those imports would resolve inside
`~/.synth-desktop`, where no chrome exists, and the build would fail. Copying
keeps the template inside the workspace where its relative imports are valid. The
cost is that editing in `~/.synth-desktop` needs a re-stage.

## Where the script reads from

The source root moved from `~/.synth/visuals/templates` to
`~/.synth-desktop/visuals/templates` when everything else consolidated under
`~/.synth-desktop`. For one revision the script read only the new path while
every internal template still sat in the old one, so it exited 0 having staged
nothing. It now reads both, and names the one it used on every run:

1. `$SYNTH_INTERNAL_VISUALS_ROOT`, if set. An explicit override that is not a
   directory is an error, not a fallback — you asked for that path.
2. `~/.synth-desktop/visuals/templates`, if it holds at least one directory
   with a `template.json`. *Holds*, not *exists*: the app creates this root for
   the runtime user tier whether or not anything was migrated into it, and an
   empty new root shadowing a populated old one is the regression again.
3. `~/.synth/visuals/templates` — the legacy location. Staging from it works,
   but the run prints a migration notice, because a template left there can
   never load at runtime.

Staging zero templates from a root that exists is a warning on stderr, never a
quiet success. Neither root present is a plain "nothing to stage" that names
both paths it looked at.

Migrate out of the legacy path with:

    mkdir -p ~/.synth-desktop/visuals/templates
    mv ~/.synth/visuals/templates/* ~/.synth-desktop/visuals/templates/

The new path is also where the runtime tier looks, so a moved template is a user
template as soon as it can compile there. Until it is moved, the legacy fallback
only keeps the *build-time* copy working.

Everything except this README and `.gitkeep` is gitignored and excluded from
the `@synth/visuals` package `files` list, so a public release builds with an
empty root and contains no internal template.

The registry derives `distribution: "internal"` from this directory. A template
here cannot shadow a public id — the public one always wins.

## Why this still exists

Item 30 was going to delete the script: user templates now load from
`<state root>/visuals/templates` at runtime, with no rebuild, so the copy-in
dance should be unnecessary. **It is not, yet.** The runtime tier compiles a
template through `compileSourcedModule`, whose allowlist
(`visuals/runtime/sourcedValidate.ts`) is eleven exact module specifiers. It has
no relative-path resolution at all, so a relative *value* import is refused with
`Unknown import "…"`. Type-only imports are fine — sucrase elides them before
`requireAllowed` ever sees them.

Both internal templates that exist today would fail:

| Import used by the internal shells | Runtime tier |
| --- | --- |
| `useLiveEvalStream` | `@synth/visuals/chrome/useLiveEvalStream` — works |
| `VisualChrome` | `@synth/visuals/chrome` — works |
| `MetricStrip` | specifier is allowlisted, but `ALLOWED_MODULES["@synth/visuals/chrome"]` exposes only `{ VisualChrome }` — resolves to `undefined` |
| `Identifier` (`chrome/Identifier.tsx`) | no allowlisted specifier |
| `formatMissingNumber` (`runtime/liveStream.ts`) | no allowlisted specifier |
| `projectLiveEval` (`runtime/liveEvalReducer.ts`) | no allowlisted specifier |

The runtime tier also reads exactly two files, `template.json` and `shell.tsx`.
A staged template is a whole directory: both internal ones ship an `examples/`
tree, one ships its own README, and shipped families reach further still
(`craftax.eval_matrix.v1` imports `./components/matrixUtils.ts` and a fixture
JSON). Single-file is a real constraint, not an incidental one.

So retiring this needs three things first, none of them large:

1. Publish the missing chrome and runtime helpers as allowlisted specifiers —
   `MetricStrip` and `Identifier` under `@synth/visuals/chrome`, and the
   formatting and reducer helpers under a new `@synth/visuals/runtime/live`
   entry. Adding a specifier means adding it to **both**
   `SOURCED_ALLOWED_IMPORTS` and `ALLOWED_MODULES`; the `MetricStrip` row above
   is what happens when only the first one is updated.
2. Port the two internal shells onto those specifiers, and confirm they render.
3. Decide what a template's non-shell files mean at runtime — today `examples/`
   is scanned past and a sibling `.ts` module cannot be imported at all.

Until then the script stays, because deleting it would strand the only working
way to run these two templates. A public release still runs `--check`, not
`--clean`: failing loudly beats silently deleting whatever a developer staged.
