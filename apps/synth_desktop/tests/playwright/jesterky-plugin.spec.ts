import { expect, test } from "./browser.fixture";

test("optional Jesterky installs and disables without hiding retained visuals", async ({ page }) => {
  await page.addInitScript(() => {
    let installed = false, enabled = true;
    let settings = {annotationScope:"selected_rollouts"};
    const status = () => ({schemaVersion:"synth.plugin-status.v1",pluginId:"jesterky",enabled,
      phase: !enabled ? "disabled" : installed ? "ready" : "not_installed",installedVersion: installed ? "0.1.2" : null,
      selectedVersion:"0.1.2",releaseChannel:"official",catalogVersion:"0.1.2",service:{phase:installed?"ready":"not_installed",activeRuns:0},algorithms:[],templates:[]});
    (window as any).__jesterkyActions=[];
    (window as any).synthPlugins = {jesterkyAnalysisSettings:async(update?:typeof settings)=>{if(update)settings=update;return settings;},status:async()=>status(),list:async()=>[status()],setReleaseChannel:async()=>status(),manage:async(operation:string,id:string)=>{
      (window as any).__jesterkyActions.push({operation,id});
      if(operation === "install")installed=true;
      if(operation === "disable")enabled=false;
      if(operation === "remove")installed=false;
      return {result:"ok",status:status()};
    }};
  });
  await page.reload();
  await page.getByTestId("titlebar").waitFor();
  await page.getByRole("button",{name:"Plugins",exact:true}).click();
  await page.getByTestId("plugin-viewer-jesterky").getByRole("button",{name:"Open",exact:true}).click();
  const panel=page.getByTestId("jesterky-page");
  await expect(panel).toContainText("Optional analysis");
  await expect(panel).toContainText("independently of an annotated eval job");
  await expect(panel.getByLabel("Analysis scope")).toHaveValue("selected_rollouts");
  await panel.getByLabel("Analysis scope").selectOption("selected_evidence");
  await expect(panel.getByRole("status")).toContainText("No analysis has started");
  await panel.getByRole("button",{name:"Back",exact:false}).click();
  await page.getByRole("button",{name:"Plugins",exact:true}).click();
  await page.getByTestId("plugin-viewer-jesterky").getByRole("button",{name:"Open",exact:true}).click();
  await expect(panel.getByLabel("Analysis scope")).toHaveValue("selected_evidence");
  await panel.getByRole("button",{name:"Download Jesterky"}).click();
  await expect(page.getByTestId("jesterky-phase")).toContainText("Ready");
  await panel.getByRole("button",{name:"Disable",exact:true}).click();
  await expect(page.getByTestId("jesterky-phase")).toContainText("Disabled");
  await expect(panel.getByRole("button",{name:"Open trace visuals"})).toBeEnabled();
  await panel.screenshot({path:"../../artifacts/trace-research-2026-09-07/jesterky-settings.png"});
  expect(await page.evaluate(()=>(window as any).__jesterkyActions)).toEqual([{operation:"install",id:"jesterky"},{operation:"disable",id:"jesterky"}]);
});
