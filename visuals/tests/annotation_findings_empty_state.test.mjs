import assert from "node:assert/strict";
import test from "node:test";
import { build } from "esbuild";
import { chromium } from "playwright";
import { fileURLToPath } from "node:url";

// Fixtures. Each projection is hand-written to the annotation-evidence shape and
// named for the absence it stands for; none is a captured campaign.
const base = {
  campaign: { domain: "craftax", title: "Fixture campaign" },
  findings: [],
  milestones: [],
  spans: [],
  taxonomy: [],
  coverage: {},
  validation: {}
};

test("an empty Findings tab says which absence it is", async () => {
  const root = fileURLToPath(new URL("../../", import.meta.url));
  const bundle = await build({
    stdin: {
      contents:
        "import React from 'react';import {createRoot} from 'react-dom/client';" +
        "import {Shell} from './visuals/families/analysis/analysis.annotation_workbench.v1/shell.tsx';" +
        "const root=createRoot(document.getElementById('root'));" +
        "window.render=(evidence)=>root.render(<Shell evidence={evidence}/>);",
      resolveDir: root,
      loader: "tsx"
    },
    bundle: true,
    write: false,
    outfile: "findings-empty-test.js",
    format: "iife",
    platform: "browser",
    jsx: "automatic"
  });

  const browser = await chromium.launch({ headless: true });
  try {
    const page = await browser.newPage();
    const errors = [];
    page.on("pageerror", (error) => errors.push(error.message));
    await page.route("http://findings.test/", (route) =>
      route.fulfill({ body: '<div id="root"></div>', contentType: "text/html" }));
    await page.goto("http://findings.test/");
    await page.addStyleTag({ content: bundle.outputFiles.find((f) => f.path.endsWith(".css")).text });
    await page.addScriptTag({ content: bundle.outputFiles.find((f) => f.path.endsWith(".js")).text });

    const empty = page.getByTestId("analysis-findings-empty");
    const show = async (evidence) => {
      await page.evaluate((value) => window.render(value), evidence);
      await page.getByTestId("analysis-view-findings").click();
    };

    // Nothing has been analysed. Not the same as "nothing was found".
    await show(base);
    await empty.waitFor();
    assert.match(await empty.innerText(), /No annotation job has run/);

    // Jobs ran and reported nothing. That is their result.
    await show({ ...base, coverage: { jobs: 4, sealed: 4 } });
    await empty.waitFor();
    assert.match(await empty.innerText(), /4 annotation jobs reported no finding/);
    assert.match(await empty.innerText(), /none raised anything/);

    // An abstention is not a clean zero, and must not read as one.
    await show({ ...base, coverage: { jobs: 4, sealed: 2, abstained: 2 } });
    await empty.waitFor();
    assert.match(await empty.innerText(), /2 abstained/);
    assert.match(await empty.innerText(), /abstention is not a clean result/);

    // The retained imported case CUA hit: sealed campaign, zero jobs, evidence
    // head present. No annotation ran here at all.
    await show({
      ...base,
      campaign: { ...base.campaign, status: "sealed" },
      coverage: { jobs: 0 },
      evidenceHead: { digest: "sha256:fixture" }
    });
    await empty.waitFor();
    assert.match(await empty.innerText(), /imported evidence; no annotation job ran here/);

    // Findings exist but a filter hides them: the pane must not claim there are none.
    await show({
      ...base,
      coverage: { jobs: 1, sealed: 1 },
      findings: [
        { id: "f1", label: "drift", target: { id: "s1" }, summary: "one" },
        { id: "f2", label: "drift", target: { id: "s2" }, summary: "two" }
      ]
    });
    await page.getByTestId("analysis-finding-f1").waitFor();
    assert.equal(await empty.count(), 0, "findings are present, so nothing is empty");

    assert.deepEqual(errors, []);
  } finally {
    await browser.close();
  }
});
