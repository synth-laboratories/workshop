import type { ClockDomain, TemporalCorrespondence, TemporalCursor, TimeContract } from "@synth/visuals-protocol";

export class ClockController {
  readonly #domains = new Map<string, ClockDomain>();
  readonly #correspondences: TemporalCorrespondence[];
  readonly #cursors = new Map<string, TemporalCursor>();

  constructor(contract: TimeContract) {
    for (const domain of contract.domains) {
      if (this.#domains.has(domain.id)) throw new Error(`Duplicate clock domain ${domain.id}`);
      this.#domains.set(domain.id, Object.freeze({ ...domain }));
    }
    this.#correspondences = structuredClone(contract.correspondences ?? []);
    for (const mapping of this.#correspondences) {
      if (!this.#domains.has(mapping.from.domain) || !this.#domains.has(mapping.to.domain)) throw new Error("Clock correspondence references an unknown domain");
    }
  }

  domains(): ClockDomain[] { return [...this.#domains.values()]; }
  cursors(): TemporalCursor[] { return [...this.#cursors.values()].map((cursor) => ({ ...cursor })); }
  cursor(domain: string): TemporalCursor | undefined { return this.#cursors.get(domain); }

  seek(cursor: TemporalCursor): void {
    if (!this.#domains.has(cursor.domain)) throw new Error(`Unknown clock domain ${cursor.domain}`);
    this.#cursors.set(cursor.domain, Object.freeze({ ...cursor, mode: "fixed" }));
  }

  follow(domain: string, value: number | string, scopeId?: string): void {
    if (!this.#domains.has(domain)) throw new Error(`Unknown clock domain ${domain}`);
    this.#cursors.set(domain, Object.freeze({ domain, value, scopeId, mode: "follow" }));
  }

  corresponding(cursor: TemporalCursor, targetDomain: string): TemporalCursor | undefined {
    const mapping = this.#correspondences.find((entry) =>
      entry.from.domain === cursor.domain && entry.from.value === cursor.value && entry.to.domain === targetDomain
      && (entry.from.scopeId === undefined || entry.from.scopeId === cursor.scopeId));
    return mapping ? { ...mapping.to, mode: "fixed" } : undefined;
  }
}

