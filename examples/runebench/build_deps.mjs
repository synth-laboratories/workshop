import { createRequire } from 'node:module';
// Resolve from the app's locked Vite dependency, whether npm hoists it or not.
const appRequire = createRequire(new URL('../../apps/synth_desktop/package.json', import.meta.url));
const viteRequire = createRequire(appRequire.resolve('vite'));
export const { build } = viteRequire('esbuild');
