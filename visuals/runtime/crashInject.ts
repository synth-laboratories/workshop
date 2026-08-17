/**
 * Test/harness hook: inject a renderer crash once per visual identity+revision.
 * Retry remounts the same identity after valid data is restored; a second throw
 * at that identity would make recovery impossible.
 */

const consumed = new Set<string>();

export function crashInjectKey(
  visualId: string,
  revision: number | null | undefined
): string {
  return `${visualId}:${revision ?? "none"}`;
}

export function consumeInjectedRendererCrash(
  visualId: string,
  revision: number | null | undefined,
  requested: boolean
): boolean {
  if (!requested) return false;
  const key = crashInjectKey(visualId, revision);
  if (consumed.has(key)) return false;
  consumed.add(key);
  return true;
}

export function resetInjectedRendererCrashes(): void {
  consumed.clear();
}
