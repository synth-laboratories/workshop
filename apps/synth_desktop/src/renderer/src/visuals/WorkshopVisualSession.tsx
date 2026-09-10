import { useEffect, useMemo, type ReactNode } from "react";
import { createVisualClient } from "@synth/visuals-sdk";
import { visualExtensions } from "@synth/visuals";
import { VisualSessionProvider, VisualSessionToolbar, useVisualSessionClient, useVisualSessionSnapshot, installVisualCaptureBarrier } from "@synth/visuals-react";
import type { ArtifactRef } from "../types/landing";
import { bridges } from "../runtime/desktopBridge";
import { publicError } from "../runtime/publicError";

function ControlScene() {
  const client = useVisualSessionClient(); const snapshot = useVisualSessionSnapshot();
  useEffect(() => {
    if (!snapshot?.ready || !client) return;
    const state=snapshot.state;
    client.publishScene(state.scene ?? {visualId:state.visualId,revision:state.revision,stateVersion:state.stateVersion,
      clocks:{},selection:{members:[]},truth:{},diagnostics:[],landmarks:[]});
  },[client,snapshot]);
  return null;
}

function SessionSurface({children}:{children:ReactNode}) {
  const snapshot=useVisualSessionSnapshot();
  const state=snapshot?.state;
  return <div data-visual-session-id={state?.visualId} data-visual-session-revision={state?.revision}
    data-visual-session-version={state?.stateVersion} data-visual-session-ready={snapshot?.ready&&!snapshot.error?"true":"false"}
    style={{display:"contents"}}>{children}</div>;
}

// All panes share one paint barrier; unmounting a secondary pane must not
// remove the adapter from the remaining mounted visual.
let captureUsers=0;
let removeCaptureAdapter:(()=>void)|undefined;
function useCaptureAdapter(){
  useEffect(()=>{
    if(captureUsers++===0)removeCaptureAdapter=installVisualCaptureBarrier({
      nativeSnapshotPaint:true,
      blockedSelector:'[data-visual-capture-blocked],[data-testid="visual-invalid"],.visual-loading,[data-testid="visual-optimizer-hydrating"],[data-unresolved-input],[data-testid="optimizer-run-view-v2-unavailable"],[data-testid="optimizer-history-loading"]',
    });
    return()=>{if(--captureUsers===0){removeCaptureAdapter?.();removeCaptureAdapter=undefined;}};
  },[]);
}

export function WorkshopVisualSession({ artifact, children }: { artifact: ArtifactRef; children: ReactNode }) {
  useCaptureAdapter();
  const visualId = artifact.visualId ?? artifact.id;
  const revision = typeof artifact.revision === "number" ? artifact.revision : Number(artifact.metadata?.currentRevision ?? artifact.metadata?.revision ?? 0);
  const client=useMemo(() => {
    const bridge=bridges.visuals;
    if (!bridge?.engine || !visualId || revision < 1) return null;
    const definition=artifact.templateId?visualExtensions.definition(artifact.templateId):undefined;
    return createVisualClient({visualId,revision,viewKey:"default"},{id:definition?.id ?? artifact.templateId ?? artifact.rendererKind ?? "visual",version:definition?.version ?? "1.0.0"},{
      pixelCapture:true,
      evidenceCuts:true,
      request:async(request) => {
        try{return await bridge.engine!(visualId,request);}
        catch(reason){throw new Error(publicError(reason));}
      },
      subscribe: bridge.onEngineChanged ? (changed) => bridge.onEngineChanged!((identity) => {
        if(identity.visualId===visualId && identity.revision===revision && identity.viewKey==="default") changed();
      }) : undefined,
    });
  },[visualId,revision,artifact.templateId,artifact.rendererKind]);
  if (!client) return <>{children}</>;
  return <VisualSessionProvider client={client}><VisualSessionToolbar /><ControlScene /><SessionSurface>{children}</SessionSurface></VisualSessionProvider>;
}
