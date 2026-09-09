import {readdirSync,readFileSync,existsSync,writeFileSync,mkdirSync} from 'node:fs';
import {join,dirname,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';
import {spawnSync} from 'node:child_process';
const repo=resolve(dirname(fileURLToPath(import.meta.url)),'../..'),root=process.argv[2];
if(!root?.startsWith('/tmp/workshop-visuals-native-'))throw new Error('Explicit isolated acceptance root required');
function walk(path){return readdirSync(path,{withFileTypes:true}).flatMap(entry=>entry.isDirectory()?walk(join(path,entry.name)):entry.name==='template.json'?[join(path,entry.name)]:[]);}
const rows=[];
for(const path of walk(join(repo,'packages/workshop-visuals/families'))){
 const family=JSON.parse(readFileSync(path,'utf8')).id;
 if(!existsSync(join(dirname(path),'examples/fixture_binding.json'))){rows.push({family,status:'needs-fixture'});continue;}
 const result=spawnSync(process.execPath,[join(repo,'visuals/tests/native_visual_acceptance.mjs'),root,family,'--capture','--exercise',...(process.argv.includes('--refresh-fixture')?['--refresh-fixture']:[])],{encoding:'utf8',timeout:65_000});
 const evidence=result.status===0?JSON.parse(readFileSync(join(root,'acceptance',family+'.json'),'utf8')):null;
 const row={family,status:result.status===0?(evidence.interaction?.replayRestored?'captured-and-exercised':'capture-only'):'failed',output:(result.stdout+result.stderr).slice(-4000)};
 rows.push(row);console.log(JSON.stringify(row));
}
mkdirSync(join(root,'acceptance'),{recursive:true});
writeFileSync(join(root,'acceptance/sweep.json'),JSON.stringify(rows,null,2));
console.log(JSON.stringify({total:rows.length,passed:rows.filter(row=>row.status==='captured-and-exercised').length,remaining:rows.filter(row=>row.status!=='captured-and-exercised').map(row=>row.family)}));
