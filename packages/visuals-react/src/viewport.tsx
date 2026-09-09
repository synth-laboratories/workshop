import type { ReactNode } from "react";
import { useViewport, type ViewportTransform } from "./useViewport.ts";
export type { ViewportTransform } from "./useViewport.ts";

export function VisualViewport({ children, minScale = 0.25, maxScale = 4, initial = { scale: 1, x: 0, y: 0 }, onChange, controlId="viewport" }: {
  children: ReactNode; minScale?: number; maxScale?: number; initial?: ViewportTransform;
  onChange?: (transform: ViewportTransform) => void; controlId?:string;
}) {
  const {transform,zoom,fit,stageProps}=useViewport({id:controlId,initial,minScale,maxScale,onChange});
  return <div className="visuals-viewport" data-viewport-scale={transform.scale}>
    <div role="toolbar" aria-label="Viewport controls">
      <button type="button" onClick={() => zoom(0.15)} aria-label="Zoom in">+</button>
      <button type="button" onClick={() => zoom(-0.15)} aria-label="Zoom out">−</button>
      <button type="button" onClick={fit}>Fit</button>
    </div>
    <div className="visuals-viewport__stage" {...stageProps} role="region" aria-label="Visual viewport">
      <div style={{ transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`, transformOrigin: "0 0" }}>{children}</div>
    </div>
  </div>;
}
