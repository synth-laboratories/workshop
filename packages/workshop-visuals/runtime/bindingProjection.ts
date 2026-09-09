import { bindTemplateSlots, type BindContext } from "./bind.ts";
import { bindingInputName, type VisualBindings, type VisualTemplateMeta } from "./types.ts";

/** Domain-owned resolution; the app grants read services, not bridge globals.
 * Every request is revision-scoped by its caller and cancellation discards
 * results even when the underlying native read cannot itself be interrupted. */
export type VisualBindingReadPorts=Omit<BindContext,"loadTraceV5"> & {
  traceWindow?:(digest:string)=>Promise<Record<string,unknown>>;
  traceProjection?:(digest:string)=>Promise<{traceDigest:string;projectionKind:string;projectionSchema:string;payload:unknown}>;
};
export async function resolveBoundVisual(
  template:VisualTemplateMeta, bindings:VisualBindings, ports:VisualBindingReadPorts, signal:AbortSignal,
):Promise<Record<string,unknown>>{
  const check=()=>{if(signal.aborted)throw new DOMException("Visual binding resolution cancelled","AbortError");};
  check();
  const slots=bindings.inputs ?? bindings.slots ?? [];
  const accepted=new Set(["synth.trace.v5","synth.trace-projection.rollout-inspector.v1"]);
  for(const binding of slots){
    if(binding.kind==="trace_v5" && binding.schema && !accepted.has(binding.schema))
      throw new Error(`Unsupported trace projection schema: ${binding.schema} (input ${bindingInputName(binding)})`);
  }
  const projections=new Map<string,Promise<unknown>>();
  const loadTraceV5=(digest:string)=>{
    check();
    let pending=projections.get(digest);
    if(!pending){
      pending=(async()=>{
        if(template.id==="trace.rollout_inspector.v1" && ports.traceWindow){
          const window=await ports.traceWindow(digest);check();
          if(window.trace_digest!==digest || window.schema_version!=="synth.trace-projection.rollout-inspector-window.v1")
            throw new Error("Trace window identity mismatch");
          return window;
        }
        if(!ports.traceProjection)throw new Error("Trace projection resolver is unavailable");
        const projection=await ports.traceProjection(digest);check();
        if(projection.traceDigest!==digest)throw new Error(`Trace resolver returned digest ${projection.traceDigest} for ${digest}`);
        if(projection.projectionKind!=="rollout-inspector")throw new Error(`Unsupported trace projection kind: ${projection.projectionKind}`);
        if(projection.projectionSchema!=="synth.trace-projection.rollout-inspector.v1")throw new Error(`Unsupported trace projection schema: ${projection.projectionSchema}`);
        return projection.payload;
      })();
      projections.set(digest,pending);
    }
    return pending;
  };
  const guarded:BindContext={loadTraceV5,skipOptional:true};
  for(const key of ["loadFixture","loadLocalCas","loadQuerySnapshot","loadRun","loadOptimizerRun","loadAnnotationEvidenceHead","loadVerifierResult"] as const){
    const load=ports[key];
    if(load)guarded[key]=async source=>{check();const value=await load(source);check();return value;};
  }
  const result=await bindTemplateSlots(template,bindings,guarded);check();
  if(result.errors.length)throw new Error(result.errors.join(" · "));
  return Object.fromEntries(Object.values(result.slots)
    .filter(slot=>!["inline","live_sse","optimizer_run"].includes(slot.kind))
    .map(slot=>[slot.input ?? slot.slot,slot.data]));
}
