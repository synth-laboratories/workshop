import type {LiveEvalEvent} from './types.ts';

/** Multiple declared inputs resolve to an array even when only one is bound.
 * Keep that binding contract; normalize retained event bodies at the domain
 * boundary instead of silently discarding the array as an unknown object.
 * Distinct streams retain their event/lane identities. No clock mapping or
 * shared run metadata is fabricated for a multi-stream collection. */
export function retainedStreamInput(raw:unknown):Record<string,unknown>&{events?:LiveEvalEvent[]}{
  const object=(value:unknown):value is Record<string,unknown>=>Boolean(value)&&typeof value==='object'&&!Array.isArray(value);
  if(!Array.isArray(raw))return object(raw)?raw:{};
  if(raw.length===1)return retainedStreamInput(raw[0]);
  const streams=raw.filter(object);
  const retained=streams.filter(stream=>Array.isArray(stream.events));
  return retained.length?{events:retained.flatMap(stream=>stream.events as LiveEvalEvent[])}:{};
}
