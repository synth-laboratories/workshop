import {canonicalDigest,stableSerialize} from "@synth/visuals-sdk";

type Request=(request:Record<string,unknown>)=>Promise<Record<string,unknown>>;
const SCHEMA="synth.template-evidence-pages.v1";
const PAGE_CHARS=120_000; // Even all escaped characters stay below the native 1.5 MB limit.
const MAX_BYTES=16_000_000;
const MAX_PAGES=134;
type Manifest={schemaVersion:typeof SCHEMA;payloadDigest:string;pages:string[]};

/** A bounded derived render cache. Domain journals remain authoritative.
 * Each immutable page is retained before its manifest can be checkpointed. */
export async function retainTemplatePages(value:unknown,request:Request):Promise<Manifest>{
 const raw=stableSerialize(value);
 if(new TextEncoder().encode(raw).length>MAX_BYTES)throw new Error("Template evidence exceeds 16 MB; use the domain's paginated explorer.");
 const pages:string[]=[];
 for(let offset=0;offset<raw.length;){
  let end=Math.min(raw.length,offset+PAGE_CHARS);
  // Rust JSON correctly rejects lone surrogates; never split an emoji pair.
  const last=raw.charCodeAt(end-1);
  if(end<raw.length&&last>=0xd800&&last<=0xdbff)end--;
  const page=raw.slice(offset,end);offset=end;
  const expected=await canonicalDigest(page);
  const answer=await request({operation:"evidence.put",value:page});
  if(answer.digest!==expected)throw new Error("Template evidence page digest disagrees with native host");
  pages.push(expected);
 }
 return {schemaVersion:SCHEMA,payloadDigest:await canonicalDigest(value),pages};
}

export async function restoreTemplatePages<T>(value:unknown,request:Request):Promise<T>{
 // Existing checkpoints stored the complete payload directly.
 if(!value||typeof value!=="object"||(value as Manifest).schemaVersion!==SCHEMA)return value as T;
 const manifest=value as Manifest;
 if(!Array.isArray(manifest.pages)||!manifest.pages.length||manifest.pages.length>MAX_PAGES
  ||!/^sha256:[0-9a-f]{64}$/.test(manifest.payloadDigest))throw new Error("Invalid template evidence manifest");
 let raw="",bytes=0;
 for(const digest of manifest.pages){
  if(typeof digest!=="string"||!/^sha256:[0-9a-f]{64}$/.test(digest))throw new Error("Invalid template evidence page reference");
  const answer=await request({operation:"evidence.read",digest});
  if(typeof answer.value!=="string"||answer.value.length>PAGE_CHARS||await canonicalDigest(answer.value)!==digest)throw new Error("Template evidence page missing or corrupt");
  bytes+=new TextEncoder().encode(answer.value).length;
  if(bytes>MAX_BYTES)throw new Error("Template evidence exceeds its bounded read limit");
  raw+=answer.value;
 }
 const restored=JSON.parse(raw) as T;
 if(await canonicalDigest(restored)!==manifest.payloadDigest)throw new Error("Template evidence payload digest mismatch");
 return restored;
}
