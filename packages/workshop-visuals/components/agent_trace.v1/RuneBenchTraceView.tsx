import { GeneralTraceView, type TraceViewExtension, type TraceViewProps } from './TraceViews.tsx';
import { traceText } from './model.ts';

/** RuneBench semantics are an optional leaf; no game parsing enters the general view. */
export const runeBenchTraceExtension: TraceViewExtension = {
  id: 'runebench', label: 'RuneBench', base: 'react',
  focusItems(items) {
    const input = items.find(item => item.kind === 'model_call.started');
    const call = items.find(item => item.kind === 'tool.called') ?? items.find(item => item.kind === 'action.started');
    const result = items.find(item => item.kind === 'action.completed') ?? items.find(item => item.kind === 'tool.result');
    if (!call && !result) return items;
    const reasoning = items.find(item => item.kind === 'model_call.completed' && (item.detail?.reasoning || item.detail?.reasoning_details?.length));
    return [input, reasoning, call, result, ...items.filter(item => /reasoning|coordination|reward/.test(item.kind))].filter(Boolean) as typeof items;
  },
  renderItem(item) {
    const d = item.detail ?? {};
    if (item.kind === 'model_call.completed' && (d.reasoning || d.reasoning_details?.length)) {
      const summaries = d.reasoning_details?.filter((part:any) => part.type === 'reasoning.summary').map((part:any) => part.summary).join('\n\n');
      return <><strong>{summaries ? 'Reasoning summary' : 'Reasoning'}</strong><pre className="atv-text">{summaries || d.reasoning || 'Encrypted reasoning retained in source.'}</pre></>;
    }
    const action = d.action;
    if (d.result != null && /tool|action/.test(item.kind)) return <div style={{borderLeft:`3px solid ${d.result?.success === false ? '#aa4837' : '#327f69'}`,paddingLeft:10}}><strong>{d.result?.success === false ? '✕ Unsuccessful' : d.result?.success === true ? '✓ Completed' : 'Result'}</strong><p>{d.result?.message ?? traceText(d.result)}</p><details><summary>Full tool result</summary><pre className="atv-text">{traceText(d.result)}</pre></details></div>;
    if (action && typeof action === 'object') return <>
      {action.reason && <><strong>Reason</strong><p>{action.reason}</p></>}
      {action.x != null && action.z != null && <p>Target ({action.x}, {action.z})</p>}
      {action.text && <p>{action.text}</p>}
      <details><summary>Exact arguments</summary><pre className="atv-text">{traceText(action)}</pre></details>
      {d.result != null && <><strong>Result</strong><pre className="atv-text">{traceText(d.result)}</pre></>}
    </>;
    if (d.result != null && /tool|action/.test(item.kind)) return <><strong>Result · {item.status ?? 'Recorded'}</strong><pre className="atv-text">{traceText(d.result)}</pre></>;
    return null;
  },
  renderContext(item) {
    if (!item) return null;
    return <section><h4>RuneBench evidence</h4><p>{item.detail?.native_actor_id ?? item.actor_id} · {item.detail?.decision_id ?? item.title}</p></section>;
  },
};

export function RuneBenchTraceView(props: Omit<TraceViewProps, 'extension'>) {
  return <GeneralTraceView {...props} extension={runeBenchTraceExtension}/>;
}
