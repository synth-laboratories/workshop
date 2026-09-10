import { useEffect, useRef, useState } from "react";
import { useVisualState } from "./session.ts";
import { useViewport } from "./useViewport.ts";

export type RenditionAsset = { mediaType: string; base64: string };
export type RenditionSource = { source: string | null; image: RenditionAsset | null; error?: string };
export type RenditionServices = { load(): Promise<RenditionSource>; retry(): Promise<unknown> };

/** Host-injected source/rendition display. Pointer motion is local; completed
 * gestures, source selection and zoom are durable semantic presentation. */
export function RenditionSurface({ identity, title, className, testId, services, status = "ready", sourceLabel = "Source", viewLabel = "Diagram", renderError }: {
  identity: string; title?: string; className: string; testId: string;
  services: RenditionServices; status?: string; sourceLabel?: string; viewLabel?: string; renderError?: string;
}) {
  const [loaded,setLoaded]=useState<RenditionSource>({source:null,image:null});
  const [reload,setReload]=useState(0); const [notice,setNotice]=useState<string>();
  const [showSource,setShowSource]=useVisualState("rendition.source",false,{label:"Show canonical source"});
  const {transform:viewport,zoom,fit,stageProps}=useViewport({id:"rendition.viewport"});
  const identityRef=useRef(identity);identityRef.current=identity;
  const imageUrl=loaded.image?`data:${loaded.image.mediaType};base64,${loaded.image.base64}`:null;
  useEffect(()=>{
    let canceled=false;setLoaded({source:null,image:null});
    void services.load().then(result=>{if(!canceled)setLoaded(result);}).catch(error=>{if(!canceled)setLoaded({source:null,image:null,error:String(error)});});
    return()=>{canceled=true;};
  },[identity,services,reload]);
  const displaySource=showSource||(!imageUrl&&Boolean(loaded.source)&&(status==="failed"||Boolean(loaded.error)));
  const sourceClass=sourceLabel==="Spec"?"spec":"source";
  return <div className={className} data-testid={testId} data-render-status={status}>
    <div className={`${className}-toolbar`}>
      <span className={`${className}-status`}>{viewLabel}{loaded.error?" · Source":!imageUrl?" · Rendering":""}</span>
      <div className={`${className}-actions`}>
        <button type="button" aria-label="Zoom in" onClick={()=>zoom(.15)}>+</button>
        <button type="button" aria-label="Zoom out" onClick={()=>zoom(-.15)}>−</button>
        <button type="button" aria-label="Fit diagram" onClick={fit}>Fit</button>
        <button type="button" onClick={()=>setShowSource(value=>!value)}>{displaySource?viewLabel:sourceLabel}</button>
        <button type="button" disabled={!loaded.source} onClick={()=>{void navigator.clipboard.writeText(loaded.source??"").catch(error=>setNotice(String(error)));}}>Copy {sourceLabel.toLowerCase()}</button>
        <button type="button" disabled={!imageUrl} onClick={()=>{if(imageUrl){const link=document.createElement("a");link.href=imageUrl;link.download=`${title||"visual"}.svg`;link.click();setNotice(`Exported ${link.download}`);}}}>Export SVG</button>
        <button type="button" onClick={()=>{const token=identityRef.current;void services.retry().then(()=>{if(identityRef.current===token)setReload(value=>value+1);}).catch(error=>setNotice(String(error)));}}>Retry</button>
      </div>
    </div>
    {notice&&<p className={`${className}-notice`} role="status">{notice}</p>}
    {displaySource?<pre className={`${className}-${sourceClass}`} data-testid={`${testId}-${sourceClass}`}>{loaded.source??renderError??loaded.error??"Source unavailable."}</pre>:imageUrl?
      <div className={`${className}-stage`} {...stageProps} role="region" aria-label={`${title??viewLabel} viewport`}>
        <img src={imageUrl} alt={title??viewLabel} draggable={false} style={{transform:`translate(${viewport.x}px, ${viewport.y}px) scale(${viewport.scale})`}}/>
      </div>:<p className="visual-loading">{renderError??loaded.error??`Rendering ${viewLabel.toLowerCase()}…`}</p>}
  </div>;
}
