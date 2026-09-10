import { generateSave, Items } from './save-generator';
import { mkdirSync, writeFileSync } from 'node:fs';
import scenario from './scenario.json';
// A clean upstream image has no player directory yet. Create the engine's
// actual save root before the upstream generator chooses its fallback path.
mkdirSync('/app/server/engine/data/players/main', {recursive:true});
for (const [i,actor] of scenario.actors.entries()) {
  const name=actor.id;
  if (!/^[a-z][a-z0-9]{1,11}$/.test(name)) throw Error('Invalid bot name');
  await generateSave(name, { position: actor.position, skills:{Woodcutting:10}, inventory:[{id:Items.BRONZE_AXE,count:1},{id:Items.TINDERBOX,count:1}], varps:{281:1000}, appearance:{colors:[i, i<2?4:8,0,0,0]} });
  mkdirSync(`/app/bots/${name}`,{recursive:true});
  writeFileSync(`/app/bots/${name}/bot.env`, `BOT_USERNAME=${name}\nPASSWORD=test\nSERVER=localhost\nSHOW_CHAT=true\n`);
}
