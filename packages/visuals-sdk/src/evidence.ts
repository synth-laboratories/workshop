import type {VisualSessionClient} from "./client.ts";
import {canonicalDigest} from "./engine.ts";

/** A read result is retained before consumers render it. Replay reads only the
 * retained answer and never invokes the original port, including while offline.
 * This is a bounded derived cache, not a replacement domain store. */
export async function retainVisualRead<T>(client:VisualSessionClient|null,owner:string,args:unknown,read:()=>Promise<T>):Promise<T>{
 if(!client?.supportsEvidenceCuts)return read();
 if(!/^[a-zA-Z0-9_.-]{1,40}$/.test(owner))throw new Error("Invalid evidence read owner");
 const requestKey=await canonicalDigest(args);
 const id=`source.read.${owner}.${requestKey.slice(7)}`;
 const restored=async():Promise<T>=>{
  const digest=client.getSnapshot().state.values[id];
  if(typeof digest!=="string")throw new Error("This read was not retained in the restored evidence cut; resume live explicitly to read it");
  const answer=await client.analyticalRequest({operation:"evidence.read",digest});
  if(await canonicalDigest(answer.value)!==digest)throw new Error("Retained read digest mismatch");
  return answer.value as T;
 };
 if(client.getSnapshot().state.replay)return restored();
 client.register({id,label:id,type:"string",nullable:true},null);
 const value=await read();
 if(client.getSnapshot().state.replay)return restored();
 const digest=await client.commitEvidence(id,value);
 return digest===undefined?restored():value;
}
