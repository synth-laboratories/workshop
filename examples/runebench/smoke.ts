import {BotSDK} from '/app/sdk/index';
import {writeFileSync} from 'node:fs';
const names=process.env.BOT_NAMES!.split(/\s+/);
const bots:BotSDK[]=[];
try {
 for(const name of names){
  const sdk=new BotSDK({botUsername:name,password:'test',gatewayUrl:'ws://localhost:7780',connectionMode:'control',autoLaunchBrowser:false,autoReconnect:false});
  await sdk.connect();await sdk.waitForCondition(s=>s.inGame,60000);bots.push(sdk);
 }
 const token=`ma probe ${Date.now().toString().slice(-6)}`;
 const actions=[];
 for(let i=0;i<2;i++) actions.push(await bots[i].say(`${token} ${i}`));
 await bots[2].waitForCondition(s=>[0,1].every(i=>s.gameMessages?.some(m=>m.text.toLowerCase().includes(`${token} ${i}`))),30000);
 const result={kind:'local-scripted-ma-smoke',passed:true,timestamp:new Date().toISOString(),names,token,actions,witness:bots[2].getState()};
 writeFileSync('/logs/ma/smoke.json',JSON.stringify(result,null,2));console.log(JSON.stringify({passed:true,names,token}));
} finally {await Promise.all(bots.map(b=>b.disconnect()));}
