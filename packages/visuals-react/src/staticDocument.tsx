import {useEffect,useMemo,useRef,useState,type CSSProperties,type RefObject} from "react";
import {registerVisualPixelFreezeAdapter} from "./captureBarrier.ts";

// Only this nonce-authorized bootstrap executes. Authored scripts, event
// handlers, network access, nested frames and same-origin access stay denied.
export const FRAME_CAPTURE_RUNTIME=String.raw`(()=>{
 let cut;
 const release=()=>{if(!cut)return;cut.observer?.disconnect();clearTimeout(cut.timeout);for(const restore of cut.images)restore();for(const animation of cut.animations)animation.play();for(const svg of cut.svgs)svg.unpauseAnimations();cut=null;};
 addEventListener('message',async event=>{
  const data=event.data;if(event.source!==parent||data?.type!=='synth.visual.static-capture.v1'||typeof data.token!=='string')return;
  const answer=error=>parent.postMessage({type:'synth.visual.static-capture-result.v1',token:data.token,error},'*');
  try{
   if(data.operation==='release'){release();return;}
   if(data.operation==='prepare'){
    release();cut={token:data.token,animations:[],svgs:[],images:[],mutations:0};const current=cut;
    current.timeout=setTimeout(release,30000);
    for(const animation of document.getAnimations()){if(animation.playState==='running'){current.animations.push(animation);animation.pause();}}
    for(const svg of document.querySelectorAll('svg')){if(!svg.animationsPaused()){current.svgs.push(svg);svg.pauseAnimations();}}
    if(document.querySelector('canvas,video,audio,iframe,object,embed'))throw new Error('Document contains media requiring a renderer-specific freeze adapter');
    for(const element of document.querySelectorAll('*')){
     const rect=element.getBoundingClientRect();if(!rect.width||!rect.height)continue;
     for(const pseudo of [null,'::before','::after']){
      const style=getComputedStyle(element,pseudo);if(pseudo&&['none','normal'].includes(style.content))continue;
      if([style.backgroundImage,style.borderImageSource,style.maskImage,style.listStyleImage,style.content].some(value=>/url\(/i.test(value)))throw new Error('CSS image requires a renderer-specific pixel freeze adapter');
     }
    }
    await document.fonts.ready;
    await Promise.all([...document.images].map(async image=>{
     if(!image.complete)await image.decode();if(!image.naturalWidth)throw new Error('Static document image unavailable');
     if(cut!==current)throw new Error('Static document capture cancelled');
     if(image.closest('picture')||image.naturalWidth*image.naturalHeight>16777216)throw new Error('Image exceeds static capture capability');
     const canvas=document.createElement('canvas');canvas.width=image.naturalWidth;canvas.height=image.naturalHeight;
     const context=canvas.getContext('2d');if(!context)throw new Error('Image freeze unavailable');context.drawImage(image,0,0);
     const pixels=canvas.toDataURL('image/png'),src=image.getAttribute('src'),srcset=image.getAttribute('srcset');
     current.images.push(()=>{if(image.getAttribute('src')!==pixels||image.hasAttribute('srcset'))return;if(src===null)image.removeAttribute('src');else image.setAttribute('src',src);if(srcset!==null)image.setAttribute('srcset',srcset);});
     image.removeAttribute('srcset');image.src=pixels;await image.decode();
    }));
    if(data.nativeSnapshotPaint===true)document.documentElement.getBoundingClientRect();
    else await new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)));
    if(cut!==current)throw new Error('Static document capture cancelled');
    current.observer=new MutationObserver(records=>current.mutations+=records.length);
    current.observer.observe(document.documentElement,{subtree:true,attributes:true,childList:true,characterData:true});
    answer();return;
   }
   if(data.operation==='verify'){
    if(!cut||cut.token!==data.cutToken)throw new Error('Static document capture expired');
    cut.mutations+=cut.observer.takeRecords().length;
    if(cut.mutations)throw new Error('Static document changed during capture');
    answer();
   }
  }catch(error){answer(String(error?.message??error));}
 });
})();`;

export function staticVisualDocument(source:string):string{
 const nonce=crypto.randomUUID().replaceAll('-','');
 const policy=`default-src 'none'; script-src 'nonce-${nonce}'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:; base-uri 'none'; form-action 'none'; frame-src 'none'; object-src 'none'`;
 const runtime=`data:text/javascript;charset=utf-8,${encodeURIComponent(FRAME_CAPTURE_RUNTIME).replaceAll("'","%27").replaceAll('"','%22')}`;
 return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${policy}"><script nonce="${nonce}" src="${runtime}"></script></head><body>${source}</body></html>`;
}

/** The frame must install FRAME_CAPTURE_RUNTIME itself under its existing CSP.
 * No same-origin access or additional authored-script authority is granted.
 * Timers may keep running: a DOM change causes verification to fail closed. */
export function useFramePixelCapture(frame:RefObject<HTMLIFrameElement|null>,loaded:boolean,identity:unknown){
 useEffect(()=>{
  const surface=frame.current;if(!surface||!loaded)return;
  let cutToken:string|undefined;
  const pending=new Map<string,{resolve:()=>void;reject:(reason:Error)=>void;timer:ReturnType<typeof setTimeout>}>();
  const receive=(event:MessageEvent)=>{
   if(event.source!==surface.contentWindow||event.data?.type!=="synth.visual.static-capture-result.v1")return;
   const request=pending.get(event.data.token);if(!request)return;
   pending.delete(event.data.token);clearTimeout(request.timer);
   if(typeof event.data.error==="string")request.reject(new Error(event.data.error));else request.resolve();
  };
  window.addEventListener('message',receive);
  const send=(operation:string,signal?:AbortSignal,nativeSnapshotPaint=false)=>new Promise<void>((resolve,reject)=>{
   if(signal?.aborted){reject(new Error('Static capture cancelled'));return;}
   const token=crypto.randomUUID();if(operation==='prepare')cutToken=token;
   const timer=setTimeout(()=>{pending.delete(token);reject(new Error('Static capture handshake timed out'));},3000);
   pending.set(token,{resolve,reject,timer});
   surface.contentWindow?.postMessage({type:'synth.visual.static-capture.v1',operation,token,cutToken,nativeSnapshotPaint},'*');
  });
  const release=()=>{
   surface.contentWindow?.postMessage({type:'synth.visual.static-capture.v1',operation:'release',token:crypto.randomUUID()},'*');
   cutToken=undefined;
   for(const request of pending.values()){clearTimeout(request.timer);request.reject(new Error('Static capture released'));}pending.clear();
  };
  const unregister=registerVisualPixelFreezeAdapter(surface,{prepare:(signal,context)=>send('prepare',signal,context?.nativeSnapshotPaint),verify:()=>send('verify'),release});
  return ()=>{unregister();release();window.removeEventListener('message',receive);};
 },[loaded,identity,frame]);
}

export function StaticVisualDocument({source,title,style}:{source:string;title:string;style?:CSSProperties}){
 const frame=useRef<HTMLIFrameElement>(null),[loaded,setLoaded]=useState(false);
 const document=useMemo(()=>staticVisualDocument(source),[source]);
 useEffect(()=>{setLoaded(false);},[document]);
 useFramePixelCapture(frame,loaded,document);
 return <iframe ref={frame} title={title} srcDoc={document} sandbox="allow-scripts" referrerPolicy="no-referrer" onLoad={()=>setLoaded(true)} style={style}/>;
}
