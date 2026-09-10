/** Only the two local demo services may receive annotation capabilities. */
export function evidenceEndpoint(serviceUrl: string, runId: string): string {
  if (![
    'http://127.0.0.1:8118/annotations',
    'http://127.0.0.1:8128/annotations',
  ].includes(serviceUrl)) throw new Error('Unsupported evidence service');
  return `${serviceUrl}?run=${encodeURIComponent(runId)}`;
}
