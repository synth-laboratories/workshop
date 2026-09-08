#!/usr/bin/env node
// The UI preference schema owns its defaults. Export them for a headless
// runtime without hand-maintaining a second copy of the desktop configuration.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = fs.readFileSync(path.join(root, 'apps/synth_desktop/src/renderer/src/preferences/schema.ts'), 'utf8');
const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 } }).outputText;
const { DEFAULT_PREFERENCES } = await import(`data:text/javascript;base64,${Buffer.from(js).toString('base64')}`);
const output = JSON.stringify(DEFAULT_PREFERENCES, null, 2) + '\n';
const target = path.join(root, 'apps/synth_desktop/src-tauri/src/contract/desktop_preferences.json');
if (process.argv.includes('--check')) {
  if (fs.readFileSync(target, 'utf8') !== output) throw new Error('Desktop defaults drift; run scripts/generate-desktop-preferences.mjs');
} else fs.writeFileSync(target, output);
console.log('Desktop preference defaults verified');
