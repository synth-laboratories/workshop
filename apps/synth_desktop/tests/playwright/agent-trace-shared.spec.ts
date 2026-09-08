import { test, expect } from './browser.fixture';
import { installVisuals, liveVisual } from './v02-helpers';
import { readFileSync } from 'node:fs';
import { gzipSync } from 'node:zlib';
import { createHash } from 'node:crypto';

test('recorded RuneBench uses the shared inspector with complete evidence', async ({ page }) => {
  const root = process.env.RUNEBENCH_DEMO_ROOT;
  test.skip(!root, 'Set RUNEBENCH_DEMO_ROOT to inspect existing local recordings');
  const bytes = readFileSync(`${root}/trace-bindings.json`);
  const source = readFileSync(`${root}/viewer.built.tsx`, 'utf8');
  const data = { traceArchive: { encoding: 'gzip+base64', sha256: createHash('sha256').update(bytes).digest('hex'), data: gzipSync(bytes).toString('base64') } };
  const visual = liveVisual({ id: 'shared-runebench', title: 'Shared RuneBench', templateId: 'sourced.visual.v1', rendererKind: 'tsx', bindings: { schemaVersion: 'synth.visual-bindings.v1', inputs: [{ input: 'data', kind: 'inline', data }] } });
  await installVisuals(page, [visual], { [visual.id]: source });
  await page.getByTestId('open-visuals').click();
  const inspector = page.getByTestId('agent-trace-inspector');
  await expect(inspector).toBeVisible({ timeout: 20000 });
  await inspector.getByLabel('Trace agent', {exact:true}).selectOption({label: 'maa · lead'});
  await expect(inspector.getByText('maa-0', {exact:false}).first()).toBeVisible();
  await inspector.getByText('Observation supplied to agent', {exact:true}).first().click();
  await expect(inspector).toContainText('nearbyLocs');
  await inspector.getByRole('button', {name:'Rewards & annotations',exact:true}).click();
  await expect(inspector).toContainText('cumulative');
  await inspector.getByRole('button', {name:'Messages',exact:true}).click();
  await expect(inspector).toContainText('No messages recorded');
  await page.screenshot({path: `${root}/shared-inspector-check.png`});
});
