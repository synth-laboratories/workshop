import {
  VISUALS_PROTOCOL_VERSION,
  type AggregateResult,
  type CohortRef,
  type Completeness,
  type CorpusRef,
  type JsonValue,
  type QueryExpression,
  type QueryResult,
  type QuerySpec,
  type QueryWindow,
  type SamplingReceipt,
  type SamplingStrategy,
  type Scalar,
} from "@synth/visuals-protocol";

export type CorpusRow = { id: string; [key: string]: unknown };

export interface VisualQueryBackend<T extends CorpusRow> {
  readonly ref: CorpusRef;
  query(spec: QuerySpec, window?: QueryWindow): QueryResult<T>;
  cohort(name: string, spec: QuerySpec, parent?: CohortRef): CohortRef;
  aggregate(cohort: CohortRef, field: string): AggregateResult;
  sample(cohort: CohortRef, strategy: SamplingStrategy, count: number, options?: { seed?: number; scoreField?: string; failureField?: string }): { rows: T[]; receipt: SamplingReceipt };
}

export interface AsyncVisualQueryBackend<T extends CorpusRow> {
  readonly ref: CorpusRef;
  query(spec: QuerySpec, window?: QueryWindow, signal?: AbortSignal): Promise<QueryResult<T>>;
  cohort(name: string, spec: QuerySpec, parent?: CohortRef, signal?: AbortSignal): Promise<CohortRef>;
  aggregate(cohort: CohortRef, field: string, signal?: AbortSignal): Promise<AggregateResult>;
  sample(cohort: CohortRef, strategy: SamplingStrategy, count: number, options?: { seed?: number; scoreField?: string; failureField?: string }, signal?: AbortSignal): Promise<{ rows: T[]; receipt: SamplingReceipt }>;
}

export class AsyncQueryBackendAdapter<T extends CorpusRow> implements AsyncVisualQueryBackend<T> {
  readonly ref: CorpusRef;
  readonly backend: VisualQueryBackend<T>;
  constructor(backend: VisualQueryBackend<T>) { this.backend = backend; this.ref = backend.ref; }
  async query(spec: QuerySpec, window?: QueryWindow, signal?: AbortSignal): Promise<QueryResult<T>> { signal?.throwIfAborted(); return this.backend.query(spec, window); }
  async cohort(name: string, spec: QuerySpec, parent?: CohortRef, signal?: AbortSignal): Promise<CohortRef> { signal?.throwIfAborted(); return this.backend.cohort(name, spec, parent); }
  async aggregate(cohort: CohortRef, field: string, signal?: AbortSignal): Promise<AggregateResult> { signal?.throwIfAborted(); return this.backend.aggregate(cohort, field); }
  async sample(cohort: CohortRef, strategy: SamplingStrategy, count: number, options?: { seed?: number; scoreField?: string; failureField?: string }, signal?: AbortSignal): Promise<{ rows: T[]; receipt: SamplingReceipt }> { signal?.throwIfAborted(); return this.backend.sample(cohort, strategy, count, options); }
}

export function stableSerialize(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value);
  if (Array.isArray(value)) return `[${value.map(stableSerialize).join(",")}]`;
  return `{${Object.entries(value as Record<string, unknown>)
    .sort(([left], [right]) => left.localeCompare(right))
    .map(([key, child]) => `${JSON.stringify(key)}:${stableSerialize(child)}`)
    .join(",")}}`;
}

/** Stable state identity. Evidence stores should replace this with their cryptographic digest port. */
export function stableValueDigest(value: unknown): string {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(stableSerialize(value))) {
    hash ^= BigInt(byte);
    hash = BigInt.asUintN(64, hash * 0x100000001b3n);
  }
  return `fnv1a64:${hash.toString(16).padStart(16, "0")}`;
}

function fieldValue(row: CorpusRow, path: string): unknown {
  return path.split(".").reduce<unknown>((value, part) => {
    if (!value || typeof value !== "object" || !Object.hasOwn(value,part)) return undefined;
    return (value as Record<string, unknown>)[part];
  }, row);
}

function scalarCompare(left: unknown, right: Scalar | undefined): number {
  const rank=(value:unknown)=>value===undefined?0:value===null?1:typeof value==="boolean"?2:typeof value==="number"?3:typeof value==="string"?4:5;
  if(rank(left)!==rank(right))return rank(left)-rank(right);
  if (typeof left === "number" && typeof right === "number") return left - right;
  if(left===right)return 0;
  const a=Array.from(String(left),c=>c.codePointAt(0)!);const b=Array.from(String(right),c=>c.codePointAt(0)!);
  for(let i=0;i<Math.min(a.length,b.length);i++){if(a[i]!==b[i])return a[i]!-b[i]!;}
  return a.length-b.length;
}

export function matchesQuery(row: CorpusRow, expression: QueryExpression): boolean {
  switch (expression.op) {
    case "all": return true;
    case "and": return expression.expressions.every((child) => matchesQuery(row, child));
    case "or": return expression.expressions.some((child) => matchesQuery(row, child));
    case "not": return !matchesQuery(row, expression.expression);
    case "exists": return (fieldValue(row, expression.field) !== undefined) === (expression.exists ?? true);
    case "any": {
      const values=fieldValue(row,expression.field);
      return Array.isArray(values)&&values.some(value=>value!==null&&typeof value==="object"&&!Array.isArray(value)&&matchesQuery(value,expression.where));
    }
    case "sequence": {
      if(expression.maxGap!==undefined && (!Number.isFinite(expression.maxGap)||expression.maxGap<0))throw new Error("maxGap must be a nonnegative finite number");
      const values=fieldValue(row,expression.field);
      if(!Array.isArray(values))return false;
      const events=values.filter(value=>value!==null&&typeof value==="object"&&!Array.isArray(value));
      return events.some(before=>{
        const a=fieldValue(before,expression.sequenceField);
        return typeof a==="number"&&Number.isFinite(a)&&matchesQuery(before,expression.before)&&events.some(after=>{
          const b=fieldValue(after,expression.sequenceField);
          return typeof b==="number"&&Number.isFinite(b)&&b>a&&(expression.maxGap===undefined||b-a<=expression.maxGap)&&matchesQuery(after,expression.after);
        });
      });
    }
  }
  const left = fieldValue(row, expression.field);
  const right = expression.value;
  switch (expression.op) {
    case "eq": return left === right;
    case "neq": return left !== right;
    case "gt": return !Array.isArray(right) && typeof left===typeof right && (typeof left==="number"||typeof left==="string") && scalarCompare(left, right) > 0;
    case "gte": return !Array.isArray(right) && typeof left===typeof right && (typeof left==="number"||typeof left==="string") && scalarCompare(left, right) >= 0;
    case "lt": return !Array.isArray(right) && typeof left===typeof right && (typeof left==="number"||typeof left==="string") && scalarCompare(left, right) < 0;
    case "lte": return !Array.isArray(right) && typeof left===typeof right && (typeof left==="number"||typeof left==="string") && scalarCompare(left, right) <= 0;
    case "contains": return !Array.isArray(right) && (Array.isArray(left) ? left.includes(right) : typeof left==="string"&&typeof right==="string"&&left.includes(right));
    case "in": return Array.isArray(right) && right.includes(left as Scalar);
  }
}

function queryWith(expression: QueryExpression): QuerySpec {
  return { schemaVersion: VISUALS_PROTOCOL_VERSION, where: expression };
}

export class InMemoryCorpus<T extends CorpusRow> implements VisualQueryBackend<T> {
  readonly ref: CorpusRef;
  readonly #rows: readonly T[];
  readonly #completeness: Completeness;

  constructor(input: { id: string; schema: string; rows: T[]; revision?: string; completeness?: Completeness }) {
    this.#rows = Object.freeze(input.rows.map((row) => Object.freeze({ ...row }))) as readonly T[];
    this.#completeness = input.completeness ?? "complete";
    this.ref = Object.freeze({
      id: input.id,
      schema: input.schema,
      revision: input.revision ?? stableValueDigest(input.rows),
      count: input.rows.length,
    });
  }

  #matching(spec: QuerySpec): T[] {
    if (spec.schemaVersion !== VISUALS_PROTOCOL_VERSION) throw new Error(`Unsupported query schema ${spec.schemaVersion}`);
    const rows = this.#rows.filter((row) => matchesQuery(row, spec.where));
    if (spec.orderBy?.length) {
      rows.sort((left, right) => {
        for (const order of spec.orderBy ?? []) {
          const compared = scalarCompare(fieldValue(left, order.field), fieldValue(right, order.field) as Scalar);
          if (compared) return order.direction === "asc" ? compared : -compared;
        }
        return scalarCompare(left.id,right.id);
      });
    }
    return rows;
  }

  query(spec: QuerySpec, window: QueryWindow = { offset: 0, limit: 100 }): QueryResult<T> {
    if (window.offset < 0 || window.limit < 0 || window.limit > 1_000) throw new Error("Query window must be within 0..1000");
    const rows = this.#matching(spec);
    return {
      corpus: this.ref,
      query: spec,
      total: rows.length,
      window,
      rows: rows.slice(window.offset, window.offset + window.limit),
      exactness: "exact",
      completeness: this.#completeness,
      excluded: this.ref.count - rows.length,
    };
  }

  cohort(name: string, spec: QuerySpec, parent?: CohortRef): CohortRef {
    const effective = parent
      ? { ...spec, where: { op: "and" as const, expressions: [parent.query.where, spec.where] } }
      : spec;
    const result = this.query(effective, { offset: 0, limit: 0 });
    const denominator = parent?.count ?? this.ref.count;
    return {
      id: `cohort:${stableValueDigest({ corpus: this.ref.revision, parent: parent?.id, spec: effective })}`,
      name,
      corpus: this.ref,
      query: effective,
      count: result.total,
      denominator,
      parentCohortId: parent?.id,
      completeness: result.completeness,
      exactness: result.exactness,
      excluded: Math.max(0, denominator - result.total),
    };
  }

  aggregate(cohort: CohortRef, field: string): AggregateResult {
    const members = this.#matching(cohort.query);
    const counts = new Map<string, { value: Scalar | undefined; count: number }>();
    let missing = 0;
    for (const row of members) {
      const value = fieldValue(row, field);
      if (value === undefined) missing += 1;
      const keys = Array.isArray(value) ? [...new Set(value)] : [value];
      for (const raw of keys) {
        if(raw!==null&&typeof raw==="object")throw new Error("Aggregate fields must contain scalars or scalar arrays");
        const value = raw as Scalar|undefined;
        const encoded = value===undefined?"missing":stableSerialize(value);
        const previous = counts.get(encoded);
        counts.set(encoded, { value, count: (previous?.count ?? 0) + 1 });
      }
    }
    return {
      corpus: this.ref,
      sourceCohortId: cohort.id,
      field,
      exactness: "exact",
      completeness: this.#completeness,
      missing,
      buckets: [...counts.values()]
        .map(({ value, count }) => ({
          key: value === undefined ? "(missing)" : value===null?"(null)":String(value),
          value,
          count,
          denominator: cohort.count,
          ratio: cohort.count ? count / cohort.count : 0,
          query: queryWith({ op: "and", expressions: [cohort.query.where, value === undefined
            ? { op: "exists", field, exists: false }
            : {op:"or",expressions:[{op:"eq",field,value},{op:"contains",field,value}]}] }),
        }))
        .sort((left, right) => right.count - left.count || scalarCompare(left.value, right.value)),
    };
  }

  sample(cohort: CohortRef, strategy: SamplingStrategy, count: number, options: { seed?: number; scoreField?: string; failureField?: string } = {}): { rows: T[]; receipt: SamplingReceipt } {
    const members = this.#matching(cohort.query);
    const requested = Math.max(0, Math.min(count, 100));
    const seed = options.seed ?? 1;
    let ranked = [...members];
    const numeric = (row: T) => fieldValue(row, options.scoreField ?? "reward") as number;
    if(["outlier","boundary","representative"].includes(strategy))ranked=ranked.filter(row=>typeof numeric(row)==="number"&&Number.isFinite(numeric(row)));
    if (strategy === "failure") ranked = ranked.filter((row) => fieldValue(row, options.failureField ?? "failed")===true);
    if (strategy === "outlier") {
      const values = ranked.map(numeric).filter(Number.isFinite);
      const mean = values.reduce((sum, value) => sum + value, 0) / Math.max(1, values.length);
      ranked.sort((a, b) => Math.abs(numeric(b) - mean) - Math.abs(numeric(a) - mean) || scalarCompare(a.id,b.id));
    } else if (strategy === "boundary") {
      ranked.sort((a, b) => Math.abs(numeric(a)) - Math.abs(numeric(b)) || scalarCompare(a.id,b.id));
    } else if (strategy === "representative") {
      const values = ranked.map(numeric).filter(Number.isFinite).sort((a, b) => a - b);
      const median = values[Math.floor(values.length / 2)] ?? 0;
      ranked.sort((a, b) => Math.abs(numeric(a) - median) - Math.abs(numeric(b) - median) || scalarCompare(a.id,b.id));
    } else if (strategy === "diverse") {
      ranked.sort((a, b) => stableValueDigest({ seed, id: a.id }).localeCompare(stableValueDigest({ seed, id: b.id })));
      const stride = Math.max(1, Math.floor(ranked.length / Math.max(1, requested)));
      ranked = ranked.filter((_, index) => index % stride === 0);
    } else if (strategy === "random") {
      ranked.sort((a, b) => stableValueDigest({ seed, id: a.id }).localeCompare(stableValueDigest({ seed, id: b.id })));
    }
    const rows = ranked.slice(0, requested);
    const receipt: SamplingReceipt = {
      id: `sample:${stableValueDigest({ cohort: cohort.id, strategy, seed, requested, ids: rows.map((row) => row.id) })}`,
      strategy,
      sourceCohortId: cohort.id,
      seed: strategy === "random" || strategy === "diverse" ? seed : undefined,
      requested,
      returned: rows.length,
      memberIds: rows.map((row) => row.id),
      exactness: "exact",
      parameters: Object.fromEntries(Object.entries(options).filter(([, value]) => value !== undefined)) as Record<string, JsonValue>,
    };
    return { rows, receipt };
  }
}

export function allQuery(): QuerySpec { return queryWith({ op: "all" }); }
export function andQuery(...expressions: QueryExpression[]): QuerySpec { return queryWith({ op: "and", expressions }); }
