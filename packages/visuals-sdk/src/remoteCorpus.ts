import type { AggregateResult, CohortRef, CorpusRef, QueryResult, QuerySpec, QueryWindow, SamplingReceipt, SamplingStrategy } from "@synth/visuals-protocol";
import { stableValueDigest, type AsyncVisualQueryBackend, type CorpusRow } from "./query.ts";

export class RemoteCorpus<T extends CorpusRow> implements AsyncVisualQueryBackend<T> {
  readonly ref:CorpusRef;
  readonly request:(request:Record<string,unknown>)=>Promise<Record<string,unknown>>;
  constructor(ref:CorpusRef,request:(request:Record<string,unknown>)=>Promise<Record<string,unknown>>){this.ref=ref;this.request=request;}
  async ingest(rows:T[],signal?:AbortSignal):Promise<void>{
    if(rows.length!==this.ref.count)throw new Error("Corpus count does not match the supplied rows");
    for(let offset=0;offset<Math.max(1,rows.length);offset+=500){signal?.throwIfAborted();await this.request({operation:"corpus.put",corpus:this.ref,offset,rows:rows.slice(offset,offset+500)});}
  }
  async query(query:QuerySpec,window:QueryWindow={offset:0,limit:100},signal?:AbortSignal):Promise<QueryResult<T>>{
    signal?.throwIfAborted();const result=await this.request({operation:"corpus.query",corpus:this.ref,query,window});signal?.throwIfAborted();return result as QueryResult<T>;
  }
  async cohort(name:string,spec:QuerySpec,parent?:CohortRef,signal?:AbortSignal):Promise<CohortRef>{
    if(parent&&(parent.corpus.id!==this.ref.id||parent.corpus.revision!==this.ref.revision))throw new Error("Parent cohort belongs to another corpus revision");
    const query:QuerySpec=parent?{...spec,where:{op:"and",expressions:[parent.query.where,spec.where]}}:spec;
    const result=await this.query(query,{offset:0,limit:0},signal);const denominator=parent?.count??this.ref.count;
    return {id:`cohort:${stableValueDigest({corpus:this.ref.revision,parent:parent?.id,spec:query})}`,name,corpus:this.ref,query,count:result.total,denominator,parentCohortId:parent?.id,exactness:result.exactness,completeness:result.completeness,excluded:Math.max(0,denominator-result.total)};
  }
  async aggregate(cohort:CohortRef,field:string,signal?:AbortSignal):Promise<AggregateResult>{
    if(cohort.corpus.id!==this.ref.id||cohort.corpus.revision!==this.ref.revision)throw new Error("Cohort belongs to another corpus revision");
    signal?.throwIfAborted();const result=await this.request({operation:"corpus.aggregate",corpus:this.ref,query:cohort.query,cohortId:cohort.id,field});signal?.throwIfAborted();return result as AggregateResult;
  }
  async sample(cohort:CohortRef,strategy:SamplingStrategy,count:number,options:{seed?:number;scoreField?:string;failureField?:string}={},signal?:AbortSignal):Promise<{rows:T[];receipt:SamplingReceipt}>{
    if(cohort.corpus.id!==this.ref.id||cohort.corpus.revision!==this.ref.revision)throw new Error("Cohort belongs to another corpus revision");
    signal?.throwIfAborted();const result=await this.request({operation:"corpus.sample",corpus:this.ref,query:cohort.query,strategy,count,options});signal?.throwIfAborted();
    const rows=result.rows as T[],requested=Number(result.requested),seed=options.seed??1;
    return {rows,receipt:{id:`sample:${stableValueDigest({cohort:cohort.id,strategy,seed,requested,ids:rows.map(row=>row.id)})}`,strategy,sourceCohortId:cohort.id,seed:strategy==="random"||strategy==="diverse"?seed:undefined,requested,returned:rows.length,memberIds:rows.map(row=>row.id),exactness:"exact",parameters:options}};
  }
}
