import {retainVisualRead,type VisualSessionClient} from "@synth/visuals-sdk";

/** Only explicit read methods are wrapped. Review/submission and media CAS
 * authorities retain their existing permissions and immutable-content paths. */
export function retainWorkshopPorts(client:VisualSessionClient|null,ports:Record<string,unknown>):Record<string,unknown>{
 const result={...ports};
 for(const [name,methods] of Object.entries({history:["projectionAt"],evidence:["load"],traceResearch:["request"],collections:["page","item"]})){
  const original=ports[name] as Record<string,any>|undefined;
  if(!original)continue;
  const wrapped={...original};
  for(const method of methods)if(typeof original[method]==="function"){
   wrapped[method]=(...args:unknown[])=>retainVisualRead(client,`${name}.${method}`,JSON.parse(JSON.stringify(args)),()=>original[method](...args));
  }
  if(name==="collections")for(const [subscribe,read,field] of [["subscribePage","page","page"],["subscribeItem","item","row"]]){
   if(typeof original[subscribe]!=="function")continue;
   wrapped[subscribe]=(...args:any[])=>{
    const listener=args.pop();let cancelled=false, generation=0;
    let pending=Promise.resolve();
    if(client?.getSnapshot().state.replay){
     void wrapped[read](...args).then((value:unknown)=>{if(!cancelled)listener({status:"ready",[field]:value,stale:false});}).catch((reason:unknown)=>{if(!cancelled)listener({status:"unavailable",[field]:null,stale:false,error:String(reason)});});
     return()=>{cancelled=true;};
    }
    const stop=original[subscribe](...args,(state:any)=>{
     const current=++generation;
     if(!state[field]||state.status!=="ready"){if(!cancelled)listener(state);return;}
     // Serialize retained commits and skip superseded notifications. An older
     // asynchronous answer must not replace a newer source cut or rendered row.
     pending=pending.then(async()=>{
      if(cancelled||current!==generation)return;
      const value=await retainVisualRead(client,`${name}.${read}`,JSON.parse(JSON.stringify(args)),async()=>state[field]);
      if(!cancelled&&current===generation)listener({...state,[field]:value});
     }).catch(reason=>{if(!cancelled&&current===generation)listener({status:"unavailable",[field]:null,stale:false,error:String(reason)});});
    });
    return()=>{cancelled=true;stop();};
   };
  }
  result[name]=wrapped;
 }
 return result;
}
