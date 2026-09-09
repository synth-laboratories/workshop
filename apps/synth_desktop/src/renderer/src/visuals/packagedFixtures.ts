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

import {createFixtureIndex} from "@synth/workshop-visuals/runtime/fixtureIndex.ts";
import {generatedVisualFixtures} from "@synth/workshop-visuals/runtime/generatedFixtures.ts";

const SHARED = import.meta.glob("../../../../../../packages/workshop-visuals/fixtures/*.json", {
  eager: true,
  import: "default"
}) as Record<string, unknown>;

const TEMPLATE_EXAMPLES = import.meta.glob("../../../../../../packages/workshop-visuals/families/**/examples/*.json", {
  eager: true,
  import: "default"
}) as Record<string, unknown>;


const PACKAGED = createFixtureIndex([...Object.entries(SHARED),...Object.entries(TEMPLATE_EXAMPLES)]);

export function packagedFixtureNames(): string[] {
  return [...PACKAGED.names(),...Object.keys(generatedVisualFixtures)].sort();
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
  if(Object.hasOwn(generatedVisualFixtures,wanted))return generatedVisualFixtures[wanted]!();
  return PACKAGED.load(wanted);
}
