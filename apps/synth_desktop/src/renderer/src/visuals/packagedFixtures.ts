/**
 * Resolve `kind: "fixture"` bindings against the packaged visuals assets.
 *
 * Before this, nothing in the renderer could load one. A stored fixture
 * binding carried only a path, `propsFromBindings` had no data to hand the
 * shell, and the pane failed with `Input "annotations" (fixture) has not been
 * resolved by the Rust runtime` — which is how `annotation.overlay.v1` showed
 * a runtime error instead of its annotations on 2026-09-03.
 *
 * Both packaged locations are covered: the shared `visuals/fixtures/` set and
 * each template's own `examples/`. The globs are eager so a bound fixture
 * resolves in the same tick as an inline one.
 */

const SHARED = import.meta.glob("../../../../../../visuals/fixtures/*.json", {
  eager: true,
  import: "default"
}) as Record<string, unknown>;

const TEMPLATE_EXAMPLES = import.meta.glob("../../../../../../visuals/families/*/*/examples/*.json", {
  eager: true,
  import: "default"
}) as Record<string, unknown>;

/** Suffix index: a binding names a path relative to `visuals/`, not to here. */
function index(): Map<string, unknown> {
  const byPath = new Map<string, unknown>();
  for (const [key, value] of [...Object.entries(SHARED), ...Object.entries(TEMPLATE_EXAMPLES)]) {
    const normalized = key.replace(/^.*\/visuals\//, "");
    byPath.set(normalized, value);
    // Also index by bare file name, so a binding written as
    // `annotation_markers.json` finds `fixtures/annotation_markers.json`.
    const file = normalized.split("/").at(-1);
    if (file && !byPath.has(file)) byPath.set(file, value);
  }
  return byPath;
}

const PACKAGED = index();

export function packagedFixtureNames(): string[] {
  return [...PACKAGED.keys()].sort();
}

/**
 * Load one packaged fixture, or throw naming what was asked for.
 *
 * A missing fixture is an error rather than an empty payload: a template that
 * silently receives nothing renders its own bundled example, which is exactly
 * the substitution the visual QA sweep exists to prevent.
 */
export function loadPackagedFixture(source: string | undefined): unknown {
  const wanted = (source ?? "").trim().replace(/^\.?\//, "");
  if (!wanted) throw new Error("A fixture binding carries no source path");
  const found = PACKAGED.get(wanted) ?? PACKAGED.get(wanted.split("/").at(-1) ?? "");
  if (found === undefined) {
    throw new Error(`No packaged fixture named "${wanted}"`);
  }
  return found;
}
