import { resolve } from "node:path";
import { test, expect } from "./browser.fixture";

for (const mode of ["reordered", "truncated", "browser", "failure"] as const) {
  test(`live eval shell consumes ${mode} evidence through its real hook`, async ({ page }) => {
    const shellPath = resolve(import.meta.dirname, "../../../../packages/workshop-visuals/families/first_class_example_containers/live.eval_stream.v1/shell.tsx");
    await page.evaluate(async ({ shellPath, mode }) => {
      const entry = await fetch("/src/main.tsx").then(r => r.text());
      const specifiers = entry.split('"').filter(token => token.startsWith("/"));
      const dependency = (suffix: string) => {
        const match = specifiers.find(token => token.split("?")[0].endsWith(suffix));
        if (!match) throw new Error(`Missing Vite dependency ${suffix}`);
        return match;
      };
      const [reactModule, domModule, shell] = await Promise.all([
        import(/* @vite-ignore */ dependency("/react.js")),
        import(/* @vite-ignore */ dependency("react-dom_client.js")),
        import(/* @vite-ignore */ `/@fs${shellPath}`)
      ]);
      const react = reactModule.createElement ? reactModule : reactModule.default;
      const dom = domModule.createRoot ? domModule : domModule.default;
      const streams = [{streamId:"a",pollUrl:"/test-a"},{streamId:"b",pollUrl:"/test-b"}];
      const replay = { streams, async poll(stream: {streamId: string}) {
        const older = stream.streamId === "a";
        if (older) {
          await new Promise(resolve => setTimeout(resolve, 100));
          document.getElementById("host-evidence-test")!.dataset.delayed = "done";
        }
        if (mode === "failure" && !older) throw new Error("test poll failed");
        const counts = older ? [1,0] : [1,1];
        return { events:[{kind:"reward_signal",event_id:stream.streamId,rollout_id:stream.streamId,sequence:1,payload:{reward:.1}}],
          cursor:{next:1,hasMore:false,closed:true},
          ...(mode === "browser" ? {} : {
            projection:{schema_version:"synth.live-eval-projection.v1",event_count:older?1:2,
              kinds:["reward_signal"],reward:older ? .2 : .75,usage:null,has_live_frames:false,has_reward_txt:false},
            evidenceTruncated: mode === "truncated",
            receipt:{schemaVersion:"synth.visual-stream-receipt.v1",visualId:"hook-test",revision:1,
              ready:true,recovered:older?1:2,declaredStreamCount:2,respondingStreamCount:older?1:2,
              streamsMissingTransport:[],gaps:[],conflicts:[],
              streams:streams.map((row,index)=>({streamId:row.streamId,declaredSource:row.pollUrl,pollResponses:counts[index]}))}
          })};
      }};
      const host=document.createElement("div"); host.id="host-evidence-test"; document.body.appendChild(host);
      dom.createRoot(host).render(react.createElement(shell.Shell,{replay,visualId:"hook-test",revision:1,title:"Host evidence integration"}));
    }, {shellPath,mode});
    const host=page.locator("#host-evidence-test");
    await expect(host).toHaveAttribute("data-delayed", "done");
    if (mode === "failure") {
      await expect(host.getByRole("alert")).toHaveText("test poll failed");
      await expect(host.getByTestId("compose-metrics-count")).toHaveText("0");
      return;
    }
    await expect(host.getByTestId("compose-event-stream").locator("button[data-event-kind]")).toHaveCount(2);
    await expect(host.getByTestId("compose-event-stream").getByText("terminal", {exact:true})).toBeVisible();
    await expect(host.getByTestId("compose-metrics-count")).toHaveText("2");
    await expect(host.getByTestId("compose-metrics-scalar")).toHaveText(mode === "browser" ? "0.10" : "0.75");
    if (mode === "truncated") await expect(host.getByText(/Host evidence is truncated/)).toBeVisible();
    await host.screenshot({path:resolve("test-results", `host-live-evidence-${mode}.png`)});
  });
}
