import { commands } from '../generated/protocol';
import { fromGenerated } from '../bridge';
import { bridges } from './desktopBridge';
import type { TraceResearchClient } from '@synth/visuals/components/agent_trace.v1';
/** Thin consumer adapter: no query evaluation, credentials, or alternate store. */
export const traceResearchClient: TraceResearchClient = {
 async request(operation, arguments_) {
  if (!['query','page','snapshot','source','prepare_annotations','window'].includes(operation)) throw new Error('Unsupported trace research operation');
  if (window.location.protocol === 'tauri:' || '__TAURI_INTERNALS__' in window) {
   return JSON.parse(await fromGenerated(commands.dataTraceResearchRequest(operation, JSON.stringify(arguments_))));
  }
  if (!bridges.runtime) throw new Error('Workshop query transport unavailable');
  return bridges.runtime.request(`/v1/traces/${operation}`, { method: 'POST', body: arguments_ });
 }
};
