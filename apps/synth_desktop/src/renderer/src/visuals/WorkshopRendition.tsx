import { useMemo } from "react";
import { RenditionSurface, type RenditionServices } from "@synth/visuals-react";
import type { ArtifactRef } from "../types/landing";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

export function WorkshopRendition({artifact,kind}:{artifact:ArtifactRef;kind:"mermaid"|"systems"|"chart"}) {
  const visualId=artifact.visualId??artifact.id;
  const identity=`${visualId}:${artifact.revision??artifact.metadata?.currentRevision??artifact.metadata?.revision??""}`;
  const services=useMemo<RenditionServices>(()=>({
    async load(){
      let source:string|null=null;
      try {const asset=await bridges.visuals?.content?.(visualId);if(asset)source=new TextDecoder().decode(Uint8Array.from(atob(asset.base64),c=>c.charCodeAt(0)));}catch{/* Rendition remains useful without source. */}
      let theme:"light"|"dark"|null=kind==="chart"?null:"light";
      if(kind==="systems"&&source){try{if(JSON.parse(source).theme==="technical-dark")theme="dark";}catch{/* Native renderer diagnoses malformed source. */}}
      try {const image=await bridges.visuals?.rendition?.(visualId,"svg",theme,"pane");return{source,image:image??null};}
      catch(error){return{source,image:null,error:publicError(error)};}
    },
    async retry(){return bridges.visuals?.render?.(visualId);},
  }),[visualId,identity,kind]);
  return <RenditionSurface identity={identity} title={artifact.title} className={`${kind}-visual`} testId={kind==="systems"?"visual-systems-map":`visual-${kind}`} services={services}
    status={typeof artifact.metadata?.renderStatus==="string"?artifact.metadata.renderStatus:"queued"}
    renderError={typeof artifact.metadata?.renderError==="string"?artifact.metadata.renderError:undefined}
    sourceLabel={kind==="chart"?"Spec":"Source"} viewLabel={kind==="chart"?"Chart":kind==="systems"?"Map":"Diagram"}/>;
}
