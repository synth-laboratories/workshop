import type { TraceViewExtension } from './TraceViews.tsx';
import type { TraceItem } from './model.ts';
import { traceText } from './model.ts';

/** App semantics only. Every visible card keeps the shared canonical annotation anchor. */
export const craftaxTraceExtension: TraceViewExtension = {
  id: 'craftax', label: 'Craftax', base: 'react',
  filterItems(items) {
    const hasCalls = items.some(item => item.detail?.kind === 'policy.call');
    return items.filter(item => item.detail?.kind === 'policy.call' ||
      /^environment\.(action_executed|reward|terminal)$/.test(item.kind) ||
      (!hasCalls && (item.kind === 'environment.observation' || typeof item.detail?.ascii === 'string')) || Boolean(item.detail?.stopped_on));
  },
  groupItems(items) {
    const groups: {id: string; items: TraceItem[]}[] = [];
    const current = new Map<string, {id: string; items: TraceItem[]}>();
    for (const item of items) {
      const lane = JSON.stringify([item.actor_id, item.session_id]);
      let group = current.get(lane);
      if (item.detail?.kind === 'policy.call' || !group) {
        group = {id: item.item_id, items: []}; groups.push(group); current.set(lane, group);
      }
      group.items.push(item);
    }
    return groups;
  },
  renderItem(item) {
    const d = item.detail ?? {};
    if (d.kind === 'policy.call') {
      // Parse only the explicitly recorded reply format. Keep the exact reply available.
      const reply = typeof d.reply === 'string' ? d.reply : '';
      const match = reply.match(/^THOUGHT:\s*([\s\S]*?)\nACTIONS:\s*([\s\S]*)$/);
      return <>
        <strong>Decision {d.call_index} · {d.model}</strong>
        <details><summary>Observation supplied to agent</summary><pre className="atv-text">{d.prompt}</pre></details>
        {match ? <><strong>Thought</strong><p>{match[1]}</p><strong>Actions</strong><p>{match[2]}</p></> : <><strong>Recorded assistant reply</strong><pre className="atv-text">{reply}</pre></>}
        <details><summary>Exact reply and system prompt</summary><pre className="atv-text">{reply}</pre><pre className="atv-text">{d.prefix}</pre></details>
      </>;
    }
    if (item.kind === 'environment.action_executed' || item.kind === 'span.environment_step') return <>
      <strong>Step {d.step_index} · {d.action} → {d.transition ?? 'recorded'}</strong>
      {(d.payload?.reason ?? d.reason) && <p>{d.payload?.reason ?? d.reason}</p>}
      {d.payload?.resource && <p>{d.payload.resource}{d.payload.target ? ` at ${d.payload.target.join(', ')}` : ''}</p>}
    </>;
    if (item.kind === 'environment.reward') return <><strong>{d.payload?.achievement ? `Achievement · ${d.payload.achievement}` : 'Reward change'}</strong><pre className="atv-text">{traceText(d.payload ?? d)}</pre></>;
    if (typeof d.ascii === 'string') return <>
      <strong>Recorded frame · step {d.step_index}</strong>
      <p>Reward {d.total_reward} · position {d.player_pos?.join(', ')} · {d.achievement_count} achievements</p>
      <p>Health {d.vitals?.health} · food {d.vitals?.food} · drink {d.vitals?.drink}</p>
      <details><summary>Recorded ASCII frame</summary><pre className="atv-text">{d.ascii}</pre></details>
    </>;
    if (d.stopped_on) return <><strong>Episode ended · {d.stopped_on}</strong><p>{d.env_steps} steps · reward {d.reward}</p></>;
    return null;
  },
  renderContext(item) {
    const d = item?.detail;
    if (d?.kind === 'policy.call') return <section><h4>Observation at decision {d.call_index}</h4><pre className="atv-text">{d.prompt}</pre></section>;
    return d?.ascii ? <section><h4>Selected Craftax frame · step {d.step_index}</h4><pre style={{font:'8px/1.1 monospace',overflow:'auto',maxHeight:300}}>{d.ascii}</pre></section> : null;
  },
};
