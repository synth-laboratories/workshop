import { expect, test } from "./browser.fixture";
import { liveVisual, openVisual } from "./v02-helpers";

test("instance TSX templates render, reload edits, and reject forbidden source", async ({ page }) => {
  const row = liveVisual({id: "vis_user_template", templateId: "user.release-proof.v1", title: "Local template"});
  await page.addInitScript(row => {
    const state = { source: 'export default function Shell() { return <div data-testid="user-proof">First revision</div>; }', digest: "sha256:first" };
    (window as any).userTemplateProof = state;
    const meta = () => ({id: row.templateId, schemaVersion: "synth.visual-template.v1", version: "1.0.0", title: row.title,
      genre: "custom", inputs: [], sourceKind: "user", rendererKind: "template", templateDigest: state.digest});
    (window as any).synthVisuals = {
      listTemplates: async () => [meta()], getTemplate: async () => meta(),
      templateShellSource: async () => state.source,
      list: async () => [row], get: async () => row, show: async () => row,
      onEvent: () => () => undefined, onShow: () => () => undefined
    };
  }, row);
  await page.reload();
  const pane = await openVisual(page, row.id);
  await expect(pane.getByTestId("user-proof")).toHaveText("First revision");
  await page.evaluate(() => {
    (window as any).userTemplateProof.source = 'export default function Shell() { return <div data-testid="user-proof">Edited revision</div>; }';
    (window as any).userTemplateProof.digest = "sha256:second";
  });
  await expect(pane.getByTestId("user-proof")).toHaveText("Edited revision");
  await page.evaluate(() => {
    (window as any).userTemplateProof.source = 'export default function Shell() { fetch("https://example.test"); return <div>Must not render</div>; }';
    (window as any).userTemplateProof.digest = "sha256:third";
  });
  await expect(pane.getByTestId("visual-sourced-invalid")).toContainText("fetch");
  await expect(pane.getByTestId("user-proof")).toHaveCount(0);
});
