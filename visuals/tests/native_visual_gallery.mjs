// Review aid, not an automated visual-quality verdict. Images are the native
// WKWebView captures; Chromium only lays out the labelled contact sheets.
import {readFileSync,readdirSync,mkdirSync} from 'node:fs';
import {join} from 'node:path';
import {chromium} from 'playwright';
const root=process.argv[2];
if(!root?.startsWith('/tmp/workshop-visuals-native-'))throw new Error('Explicit isolated acceptance root required');
const directory=join(root,'acceptance');
const records=readdirSync(directory).filter(name=>name.endsWith('.json')&&name!=='sweep.json')
 .map(name=>JSON.parse(readFileSync(join(directory,name)))).filter(record=>record.capture?.path).sort((a,b)=>a.family.localeCompare(b.family));
const output=join(root,'gallery');mkdirSync(output,{recursive:true});
const browser=await chromium.launch();
try{
 const page=await browser.newPage({viewport:{width:1920,height:1440}});
 for(let i=0;i<records.length;i+=4){
  const group=records.slice(i,i+4);
  await page.setContent('<style>body{margin:0;background:#ddd;font:20px system-ui;display:grid;grid-template-columns:1fr 1fr;grid-template-rows:720px 720px}section{padding:8px;overflow:hidden}h2{margin:0 0 8px;font-size:20px}img{width:100%;height:660px;object-fit:contain;object-position:top;background:white}</style>');
  await page.evaluate(items=>{for(const item of items){const section=document.createElement('section'),label=document.createElement('h2'),image=new Image();label.textContent=item.label;image.src=item.image;section.append(label,image);document.body.append(section);}},group.map(record=>({label:record.family+' · revision '+record.revision,image:'data:image/png;base64,'+readFileSync(record.capture.path).toString('base64')})));
  await page.evaluate(()=>Promise.all([...document.images].map(image=>image.decode())));
  const path=join(output,String(i/4+1).padStart(2,'0')+'.png');await page.screenshot({path});
  console.log(JSON.stringify({path,families:group.map(record=>record.family)}));
 }
}finally{await browser.close();}
