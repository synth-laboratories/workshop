import assert from "node:assert/strict";
import { createServer } from "node:http";
import test from "node:test";
import { build } from "esbuild";
import { chromium } from "playwright";
import { fileURLToPath } from "node:url";

test("browser SSE reconnects with Last-Event-ID and reaches terminal without polling", async () => {
  let connections = 0;
  let resumedFrom;
  let pollRequests = 0;
  const server = createServer((req, res) => {
    if (req.url !== "/stream") { pollRequests += 1; res.writeHead(404); res.end(); return; }
    connections += 1;
    resumedFrom = req.headers["last-event-id"] ?? resumedFrom;
    res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache",
      "access-control-allow-origin": "*" });
    res.write("retry: 10\n\n");
    if (connections === 1) {
      res.write(`data: ${JSON.stringify({ kind: "stream.subscribed", sequence: null, payload: { ready: true } })}\n\n`);
      res.write(`id: 1\ndata: ${JSON.stringify({ kind: "jesterky.transition", sequence: 1, run_id: "run", payload: { event_kind: "workflow_started" } })}\n\n`);
      setTimeout(() => res.destroy(), 10);
      return;
    }
    res.write(`id: 2\ndata: ${JSON.stringify({ kind: "jesterky.transition", sequence: 2, run_id: "run", payload: { event_kind: "workflow_completed" } })}\n\n`);
    res.write(`id: 3\ndata: ${JSON.stringify({ kind: "stream.closed", sequence: 3, run_id: "run", payload: { status: "completed" } })}\n\n`);
    res.end();
  });
  await new Promise((ready) => server.listen(0, "127.0.0.1", ready));
  const streamUrl = `http://127.0.0.1:${server.address().port}/stream`;
  const root = fileURLToPath(new URL("../../", import.meta.url));
  const bundle = await build({ stdin: { contents: `
    import React from 'react';
    import {createRoot} from 'react-dom/client';
    import {createReplayClient} from './packages/workshop-visuals/runtime/replayClient.ts';
    import {useLiveEvalStreams} from './packages/workshop-visuals/chrome/useLiveEvalStreams.ts';
    const client=createReplayClient([{streamId:'jesterky',sseUrl:${JSON.stringify(streamUrl)}}]);
    function App(){const view=useLiveEvalStreams(client);const jsonSafe=Object.values(view).every(value=>value!==undefined);return <output data-testid="state">{view.state}:{view.events.length}:{String(view.ready)}:{view.error??''}:{String(jsonSafe)}</output>}
    createRoot(document.getElementById('root')).render(<App/>);
  `, resolveDir: root, loader: "tsx" }, bundle: true, write: false, format: "iife", platform: "browser", jsx: "automatic", outfile: "sse.js" });
  const browser = await chromium.launch();
  try {
    const page = await browser.newPage();
    await page.setContent('<div id="root"></div>');
    await page.addScriptTag({ content: bundle.outputFiles.find((file) => file.path.endsWith(".js")).text });
    await page.getByTestId("state").filter({ hasText: "terminal:2:true::true" }).waitFor({ timeout: 10_000 });
    assert.equal(connections, 2);
    assert.equal(resumedFrom, "1");
    assert.equal(pollRequests, 0);
  } finally {
    await browser.close();
    await new Promise((done) => server.close(done));
  }
});
