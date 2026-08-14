# templates-internal/ — private visual templates

Staged, never authored here. Source of truth is `~/.synth/visuals/templates/`.

    ./scripts/stage-internal-visuals.sh          # symlink ~/.synth templates in
    ./scripts/stage-internal-visuals.sh --clean  # remove them again

Everything except this README and `.gitkeep` is gitignored and excluded from
the `@synth/visuals` package `files` list, so a public release builds with an
empty root and contains no internal template.

The registry derives `distribution: "internal"` from this directory. A template
here cannot shadow a public id — the public one always wins.
