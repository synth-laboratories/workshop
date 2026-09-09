import {useMemo} from "react";
import {DynamicDiagramSurface} from "@synth/visuals-react";
import type {ArtifactRef} from "../types/landing";
import {bridges} from "../runtime/desktopBridge";

export function SystemsDynamicVisual({artifact}:{artifact:ArtifactRef}){
  const id=artifact.visualId??artifact.id;
  const services=useMemo(()=>({
    content:async()=>bridges.visuals?.content?.(id),
    rendition:async(theme:"dark"|"light")=>bridges.visuals?.rendition?.(id,"svg",theme,"pane"),
    render:async()=>bridges.visuals?.render?.(id),
  }),[id]);
  return <DynamicDiagramSurface id={id} title={artifact.title} revision={artifact.revision??String(artifact.metadata?.currentRevision??artifact.metadata?.revision??"")} services={services}/>;
}
