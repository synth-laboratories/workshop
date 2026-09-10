import type { ArtifactRef } from "../types/landing";
import { WorkshopRendition } from "../visuals/WorkshopRendition";

export function ChartVisual({artifact}:{artifact:ArtifactRef}) {
  return <WorkshopRendition artifact={artifact} kind="chart"/>;
}
