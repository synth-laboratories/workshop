/**
 * No template may present its bundled example as measurement.
 *
 * The 2026-09-03 visual QA sweep found five templates that imported a fixture
 * and rendered it whenever the bound source was absent or of a shape they did
 * not accept. A visual bound to a real retained run showed
 * `reward.breakdown.v1` totalling 4.20 while the run's mean was 0.25, and
 * `model.compare.v1` showed Laguna/Luna/Terra example rows. That is worse than
 * an empty pane: a reviewer cannot tell an example from a measurement.
 *
 * Each case below binds a real, non-fixture source that does not resolve into
 * something the template can render, and asserts the recognizable example
 * values are absent. The esbuild step is also the mandatory parse check — this
 * sweep already found syntax that passed `tsc` and the test suite but failed
 * Vite.
 */

import assert from "node:assert/strict";
import { mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import test from "node:test";
import { buildSync } from "esbuild";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

const appRoot = join(dirname(fileURLToPath(import.meta.url)), "..");
const visuals = join(appRoot, "../../visuals");
const compiledDir = join(appRoot, "node_modules/.cache/synth-desktop-tests");
mkdirSync(compiledDir, { recursive: true });

function load(relative, name) {
  const compiled = join(compiledDir, `${name}.mjs`);
  buildSync({
    entryPoints: [join(visuals, relative)],
    bundle: true,
    format: "esm",
    target: "es2022",
    platform: "neutral",
    jsx: "automatic",
    outfile: compiled,
    loader: { ".css": "empty" },
    external: ["react", "react/jsx-runtime", "react-dom", "react-dom/server"]
  });
  return import(pathToFileURL(compiled).href);
}

/** The values a reviewer would recognise as the bundled example. */
const CASES = [
  {
    name: "RewardBreakdown",
    shell: "families/analysis/reward.breakdown.v1/shell.tsx",
    input: "reward",
    fixtureMarkers: ["4.20", "invalid_action_penalty", "length_bonus"]
  },
  {
    name: "ModelCompare",
    shell: "families/analysis/model.compare.v1/shell.tsx",
    input: "comparison",
    fixtureMarkers: ["Laguna XS", "Luna", "Terra"]
  },
  {
    name: "PosttrainRolloutViewer",
    shell: "families/analysis/posttrain.rollout_viewer.v1/shell.tsx",
    input: "trajectory",
    fixtureMarkers: ["Laguna"]
  },
  {
    name: "CraftaxEvalMatrix",
    shell: "families/first_class_example_containers/craftax.eval_matrix.v1/shell.tsx",
    input: "matrix",
    fixtureMarkers: ["mock slice"]
  },
  {
    name: "CraftaxRolloutScrub",
    shell: "families/first_class_example_containers/craftax.rollout_scrub.v1/shell.tsx",
    input: "rollout",
    fixtureMarkers: ["Laguna"]
  }
];

/** A retained binding that resolved into a document of another schema. */
const INCOMPATIBLE = {
  schemaVersion: "synth.trace.v5",
  frames: [],
  note: "a real projection this template cannot render"
};

for (const testCase of CASES) {
  const { Shell } = await load(testCase.shell, testCase.name);

  test(`${testCase.shell} shows no example data under an unresolved real binding`, () => {
    const markup = renderToStaticMarkup(
      createElement(Shell, {
        bindings: [
          {
            input: testCase.input,
            kind: "trace_v5",
            source: "sha256:598ae3f11853a6b3d81fefb2f58dfb6916e33d6ae5ea7948f6a0159911d31800"
          }
        ]
      })
    );
    for (const marker of testCase.fixtureMarkers) {
      assert.ok(
        !markup.includes(marker),
        `${testCase.shell} rendered the bundled example value ${JSON.stringify(marker)}`
      );
    }
    assert.match(markup, /Unavailable/);
    assert.ok(markup.includes(testCase.input), "the unavailable state names the input it wanted");
    assert.match(markup, /sha256:598ae3f1/, "and the binding it was asked to resolve");
  });

  test(`${testCase.shell} shows no example data when a real source is the wrong shape`, () => {
    const markup = renderToStaticMarkup(
      createElement(Shell, {
        data: INCOMPATIBLE,
        bindings: [{ input: testCase.input, kind: "trace_v5", source: "sha256:deadbeef" }]
      })
    );
    for (const marker of testCase.fixtureMarkers) {
      assert.ok(
        !markup.includes(marker),
        `${testCase.shell} substituted the bundled example for an incompatible projection`
      );
    }
    assert.match(markup, /cannot render/);
  });

  test(`${testCase.shell} shows no example data when nothing is bound at all`, () => {
    const markup = renderToStaticMarkup(createElement(Shell, {}));
    for (const marker of testCase.fixtureMarkers) {
      assert.ok(!markup.includes(marker), `${testCase.shell} rendered an example with no binding`);
    }
    assert.match(markup, /no source is bound/);
  });

  test(`${testCase.shell} still renders its example under an explicit fixture binding`, () => {
    const markup = renderToStaticMarkup(
      createElement(Shell, {
        bindings: [{ input: testCase.input, kind: "fixture", source: "bundled" }]
      })
    );
    assert.ok(
      testCase.fixtureMarkers.some((marker) => markup.includes(marker)),
      `${testCase.shell} refused its own example under kind: fixture`
    );
  });
}

const { Shell: ModelCompareShell } = await load(
  "families/analysis/model.compare.v1/shell.tsx",
  "ModelCompareCatalog"
);

test("cross-benchmark runs render as independent cards without a winner", () => {
  const markup = renderToStaticMarkup(
    createElement(ModelCompareShell, {
      comparison: {
        comparison_kind: "run_catalog",
        rows: [
          { benchmark: "craftax", model: "policy-a", mean_reward: 0.25 },
          { benchmark: "banking77", model: "policy-b", mean_reward: 0.625 }
        ]
      }
    })
  );
  assert.match(markup, /Evaluation run catalog/);
  assert.match(markup, /not comparable or ranked across cards/);
  assert.match(markup, /Benchmark-local evaluation runs/);
  assert.doesNotMatch(markup, /<table/);
  assert.doesNotMatch(markup, /var\(--sv-accent\)/);
});

test("a catalog row without a benchmark fails closed", () => {
  const markup = renderToStaticMarkup(
    createElement(ModelCompareShell, {
      comparison: {
        comparison_kind: "run_catalog",
        rows: [{ model: "policy-a", mean_reward: 0.25 }]
      },
      bindings: [{ input: "comparison", kind: "inline", data: {} }]
    })
  );
  assert.match(markup, /cannot render/);
});

test("an unknown comparison kind fails closed", () => {
  const markup = renderToStaticMarkup(
    createElement(ModelCompareShell, {
      comparison: {
        comparison_kind: "rank_everything",
        rows: [{ model: "policy-a", mean_reward: 0.25 }]
      },
      bindings: [{ input: "comparison", kind: "inline", data: {} }]
    })
  );
  assert.match(markup, /cannot render/);
});

test("like-for-like ranking follows the declared achievement metric", () => {
  const markup = renderToStaticMarkup(
    createElement(ModelCompareShell, {
      comparison: {
        comparison_kind: "like_for_like",
        metric: "mean_achievements",
        rows: [
          { model: "reward-winner", mean_reward: 0.9, mean_achievements: 1 },
          { model: "achievement-winner", mean_reward: 0.2, mean_achievements: 4 }
        ]
      }
    })
  );
  assert.match(markup, /<strong style="color:var\(--sv-accent\)">achievement-winner<\/strong>/);
  assert.doesNotMatch(markup, /<strong style="color:var\(--sv-accent\)">reward-winner<\/strong>/);
});

test("like-for-like cost ranking treats lower cost as better", () => {
  const markup = renderToStaticMarkup(
    createElement(ModelCompareShell, {
      comparison: {
        comparison_kind: "like_for_like",
        metric: "cost_usd",
        rows: [
          { model: "expensive", cost_usd: 0.8 },
          { model: "efficient", cost_usd: 0.2 }
        ]
      }
    })
  );
  assert.match(markup, /<strong style="color:var\(--sv-accent\)">efficient<\/strong>/);
  assert.doesNotMatch(markup, /<strong style="color:var\(--sv-accent\)">expensive<\/strong>/);
});
