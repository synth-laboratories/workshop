import { chromium, expect } from "@playwright/test";

const browser=await chromium.launch({headless:true});
try {
  const page=await browser.newPage({viewport:{width:1440,height:1000}});
  const errors=[];page.on("pageerror",error=>errors.push(String(error)));
  await page.goto(process.env.VISUALS_DEMO_URL??"http://127.0.0.1:5194");
  const first=page.getByRole("region",{name:"Client 1",exact:true});
  const second=page.getByRole("region",{name:"Client 2",exact:true});
  await expect(first.getByRole("button",{name:"Snapshot",exact:true})).toBeEnabled();
  await first.getByLabel("Population",{exact:true}).selectOption("flagged");
  await expect(second.getByLabel("Population",{exact:true})).toHaveValue("flagged");
  await expect(second.getByText("250 of 1,000 rows; displaying 8",{exact:true})).toBeVisible();
  await first.getByRole("button",{name:"Snapshot",exact:true}).click();
  await first.getByRole("button",{name:"Record",exact:true}).click();
  await second.getByLabel("Logical step",{exact:true}).fill("37");
  await expect(first.getByLabel("Logical step",{exact:true})).toHaveValue("37");
  await first.getByRole("button",{name:"Stop recording",exact:true}).click();
  await first.getByRole("button",{name:"Saved views",exact:true}).click();
  const snapshot=await first.getByLabel("Restore snapshot",{exact:true}).locator("option").last().getAttribute("value");
  await first.getByLabel("Restore snapshot",{exact:true}).selectOption(snapshot);
  await expect(second.getByLabel("Logical step",{exact:true})).toHaveValue("0");
  const recording=await first.getByLabel("Replay recording",{exact:true}).locator("option").last().getAttribute("value");
  await first.getByLabel("Replay recording",{exact:true}).selectOption(recording);
  await first.getByLabel("Recording event",{exact:true}).fill("1");
  await expect(second.getByLabel("Logical step",{exact:true})).toHaveValue("37");
  await first.getByRole("button",{name:"Zoom in",exact:true}).click();
  await expect(second.locator("[data-viewport-scale]")).toHaveAttribute("data-viewport-scale","1.15");
  const firstFrame=first.frameLocator('iframe[title="Sandbox control"]');
  const secondFrame=second.frameLocator('iframe[title="Sandbox control"]');
  await firstFrame.getByRole("button",{name:"Increment sandbox counter"}).click();
  await expect(secondFrame.locator("output")).toHaveText("1");
  // A main-window message is not accepted as if it came from the sandbox.
  await page.evaluate(()=>window.postMessage({type:"synth.visual.session.request.v1",requestId:"spoof",operation:"register",controls:[{id:"frame.spoof",label:"Spoof",type:"number"}],defaults:{"frame.spoof":99}},"*"));
  await expect(secondFrame.locator("output")).toHaveText("1");
  expect(errors).toEqual([]);
  console.log("Portable host: two-client synchronization, full-population counts, snapshot restore, logical-time replay, viewport and sandbox control bridge passed.");
} finally {await browser.close();}
