import { useEffect, useState } from 'react';
/** Bounded, checksum-verified transport for large inline read models. No network access. */
export type TraceArchive = { encoding: 'gzip+base64'; sha256: string; data: string };
export function useTraceArchive<T>(archive?: TraceArchive): { data?: T; error?: string; loading: boolean } {
  const [state, setState] = useState<{ data?: T; error?: string; loading: boolean }>({ loading: Boolean(archive) });
  useEffect(() => {
    let cancelled = false;
    if (!archive) { setState({ loading: false }); return; }
    setState({ loading: true });
    async function decode() {
      if (archive!.encoding !== 'gzip+base64' || archive!.data.length > 8 * 1024 * 1024) throw new Error('Unsupported or oversized trace archive');
      const bytes = Uint8Array.from(atob(archive!.data), c => c.charCodeAt(0));
      const reader = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip')).getReader();
      const chunks: Uint8Array[] = []; let size = 0;
      try {
        while (true) { const { done, value } = await reader.read(); if (done) break; size += value.length; if (size > 16 * 1024 * 1024) throw new Error('Trace archive exceeds 16 MiB'); chunks.push(value); }
      } finally { await reader.cancel(); }
      const expanded = new Uint8Array(size); let offset = 0;
      for (const chunk of chunks) { expanded.set(chunk, offset); offset += chunk.length; }
      const hash = Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256', expanded)), b => b.toString(16).padStart(2, '0')).join('');
      if (hash !== archive!.sha256) throw new Error('Trace archive checksum mismatch');
      return JSON.parse(new TextDecoder().decode(expanded)) as T;
    }
    void decode().then(data => { if (!cancelled) setState({ data, loading: false }); }, error => { if (!cancelled) setState({ error: String(error), loading: false }); });
    return () => { cancelled = true; };
  }, [archive?.encoding, archive?.sha256, archive?.data]);
  return state;
}
