/** Paint-side half of the host capture barrier. The native host freezes session
 * commits first. This guard pauses compositor/media animation, waits for assets,
 * then rejects ANY DOM/evidence change while pixels are being photographed.
 * Canvas/opaque frames need an explicit renderer freeze adapter, never a guess. */
type Stamp={visualId:string;revision:number;stateVersion:number;mutations:number;ready:boolean;verified?:boolean;error?:string};
/** A renderer must stop all pixel-producing work before resolving prepare.
 * verify runs immediately before releasing the native screenshot barrier.
 * release must be idempotent, including when preparation was cancelled. */
export type VisualPixelFreezeAdapter = {
  prepare(signal:AbortSignal,context?:{nativeSnapshotPaint:boolean}):Promise<void>;
  verify():void|Promise<void>;
  release():void;
};
const adapters=new WeakMap<Element,VisualPixelFreezeAdapter>();
export function registerVisualPixelFreezeAdapter(surface:Element,adapter:VisualPixelFreezeAdapter):()=>void {
  if(adapters.has(surface))throw new Error("Capture adapter already registered for surface");
  adapters.set(surface,adapter);
  return ()=>{if(adapters.get(surface)===adapter)adapters.delete(surface);};
}
type Barrier={stamp:Stamp;observer?:MutationObserver;animations:Animation[];media:HTMLMediaElement[];roots:HTMLElement[];adapters:VisualPixelFreezeAdapter[];restoreImages:Array<()=>void>;svgs:SVGSVGElement[];abort:AbortController;closed:boolean;timeout?:ReturnType<typeof setTimeout>};
let active:Barrier|undefined;
let blockedSelector='[data-visual-capture-blocked]';
// A native offscreen snapshot performs its own paint. It must not wait for
// requestAnimationFrame, which WebKit suspends for occluded windows.
let nativeSnapshotPaint=false;
const rootsFor=(id:string)=>Array.from(document.querySelectorAll<HTMLElement>("[data-visual-session-id]"))
  .filter(node=>node.dataset.visualSessionId===id && Array.from(node.children).some(child=>child.getBoundingClientRect().width>0));
const frame=()=>new Promise<void>((resolve,reject)=>{
  const timeout=setTimeout(()=>{cancelAnimationFrame(id);reject(new Error("Capture paint frame timed out; ensure the native window is visible"));},2000);
  const id=requestAnimationFrame(()=>{clearTimeout(timeout);resolve();});
});
function release(){
  if(!active)return;
  active.closed=true;active.observer?.disconnect();
  active.abort.abort();
  clearTimeout(active.timeout);
  for(const adapter of active.adapters){try{adapter.release();}catch{/* Release every renderer even when one fails. */}}
  for(const restore of active.restoreImages)restore();
  for(const svg of active.svgs)svg.unpauseAnimations();
  for(const animation of active.animations){try{animation.play();}catch{/* A renderer may have removed its animation. */}}
  for(const media of active.media)void media.play().catch(()=>{});
  active=undefined;
}
function begin(visualId:string,revision:number,stateVersion:number){
  release();
  const roots=rootsFor(visualId);
  const barrier:Barrier={stamp:{visualId,revision,stateVersion,mutations:0,ready:false},roots,animations:[],media:[],adapters:[],restoreImages:[],svgs:[],abort:new AbortController(),closed:false};
  active=barrier;
  barrier.timeout=setTimeout(()=>{if(active===barrier)release();},30_000);
  void (async()=>{
    try{
      if(!roots.length)throw new Error("Capture session surface is not mounted");
      for(const root of roots){
        if(root.querySelector(blockedSelector))
          throw new Error("Capture visual is unresolved, loading, or invalid");
        if(root.dataset.visualSessionReady!=="true" || Number(root.dataset.visualSessionRevision)!==revision || Number(root.dataset.visualSessionVersion)!==stateVersion)
          throw new Error("Capture renderer has not observed the committed session version");
        // CSS URL images can animate independently of the DOM and WAAPI.
        // Until a renderer freezes them explicitly, do not certify those pixels.
        for(const element of [root,...root.querySelectorAll('*')]){
          const rect=element.getBoundingClientRect();if(!rect.width||!rect.height)continue;
          for(const pseudo of [null,'::before','::after']){
            const style=getComputedStyle(element,pseudo);
            if(pseudo&&['none','normal'].includes(style.content))continue;
            if([style.backgroundImage,style.borderImageSource,style.maskImage,style.listStyleImage,style.content].some(value=>/url\(/i.test(value)))
              throw new Error("CSS image requires a renderer-specific pixel freeze adapter");
          }
        }
        for(const surface of root.querySelectorAll("canvas,iframe")){
          const adapter=adapters.get(surface);
          if(!adapter)throw new Error("This renderer requires a canvas/frame freeze adapter for coherent capture");
          if(!barrier.adapters.includes(adapter))barrier.adapters.push(adapter);
        }
        for(const animation of root.getAnimations({subtree:true})){
          if(animation.playState==="running"){barrier.animations.push(animation);animation.pause();}
        }
        for(const svg of root.querySelectorAll("svg"))if(!svg.animationsPaused()){barrier.svgs.push(svg);svg.pauseAnimations();}
        for(const media of root.querySelectorAll<HTMLMediaElement>("video,audio")){
          if(!media.paused){barrier.media.push(media);media.pause();}
        }
      }
      await Promise.all(barrier.adapters.map(adapter=>adapter.prepare(barrier.abort.signal,{nativeSnapshotPaint})));
      if(barrier.closed)return;
      await document.fonts.ready;
      await Promise.all(roots.flatMap(root=>Array.from(root.querySelectorAll("img"))).map(async image=>{
        if(!image.complete)await image.decode();
        if(!image.naturalWidth)throw new Error("Capture contains an unavailable image");
        if(barrier.closed)return;
        // Decode alone does not stop animated GIF/WebP. Freeze the actual
        // decoded frame, preserving dimensions; tainted pixels fail closed.
        if(image.closest('picture') || image.naturalWidth*image.naturalHeight>16_777_216)throw new Error("Image requires a bounded pixel freeze adapter");
        const canvas=document.createElement('canvas');canvas.width=image.naturalWidth;canvas.height=image.naturalHeight;
        const context=canvas.getContext('2d');if(!context)throw new Error("Image freeze canvas unavailable");
        context.drawImage(image,0,0);
        const pixels=canvas.toDataURL('image/png'),src=image.getAttribute('src'),srcset=image.getAttribute('srcset');
        barrier.restoreImages.push(()=>{if(image.getAttribute('src')!==pixels||image.hasAttribute('srcset'))return;if(src===null)image.removeAttribute('src');else image.setAttribute('src',src);if(srcset!==null)image.setAttribute('srcset',srcset);});
        image.removeAttribute('srcset');image.src=pixels;await image.decode();
      }));
      if(nativeSnapshotPaint){
        for(const root of roots)for(const child of root.children)child.getBoundingClientRect();
      }else{await frame();await frame();}
      if(barrier.closed)return;
      // Recheck after asynchronous image/font loading. A native event may have
      // caught this pane up while assets were resolving.
      for(const root of roots)if(Number(root.dataset.visualSessionVersion)!==stateVersion)throw new Error("Capture version changed");
      barrier.observer=new MutationObserver(records=>{barrier.stamp.mutations+=records.length;});
      for(const root of roots)barrier.observer.observe(root,{subtree:true,attributes:true,childList:true,characterData:true});
      barrier.stamp.ready=true;
    }catch(error){barrier.stamp.error=error instanceof Error?error.message:String(error);}
  })();
}
function read(){
  if(!active)return null;
  active.stamp.mutations+=active.observer?.takeRecords().length ?? 0;
  if(active.roots.some(root=>!root.isConnected || Number(root.dataset.visualSessionVersion)!==active!.stamp.stateVersion))
    active.stamp.error="Capture surface detached or changed version";
  return {...active.stamp};
}
function verify(){
  const barrier=active;if(!barrier||!barrier.stamp.ready)return;
  barrier.stamp.verified=false;
  void Promise.all(barrier.adapters.map(async adapter=>adapter.verify())).then(()=>{
    if(!barrier.closed)barrier.stamp.verified=true;
  }).catch(error=>{if(!barrier.closed)barrier.stamp.error=error instanceof Error?error.message:String(error);});
}
/** nativeSnapshotPaint is only for hosts that subsequently paint the frozen DOM
 * using an offscreen renderer API and verify this barrier before release. It
 * does not certify a desktop screenshot or a previously rendered frame. */
export function installVisualCaptureBarrier(options:{blockedSelector?:string;nativeSnapshotPaint?:boolean}={}):()=>void {
  blockedSelector=options.blockedSelector ?? '[data-visual-capture-blocked]';
  nativeSnapshotPaint=options.nativeSnapshotPaint===true;
  const host=window as unknown as {__synthVisualCapture?:{begin:typeof begin;read:typeof read;verify:typeof verify;release:typeof release}};
  host.__synthVisualCapture={begin,read,verify,release};
  return ()=>{release();delete host.__synthVisualCapture;};
}
