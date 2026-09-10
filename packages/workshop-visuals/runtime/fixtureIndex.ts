/** Exact fixture paths are identities. A missing family path must never fall
 * back to another family's identically named events.json. Bare-name legacy
 * bindings are accepted only when the catalog has exactly one such file. */
export function createFixtureIndex(entries:Iterable<readonly [string,unknown]>){
  const paths=new Map<string,unknown>(),names=new Map<string,string[]>();
  for(const [path,value] of entries){
    const normalized=path.replace(/^.*\/(?:packages\/workshop-visuals|visuals)\//,"");
    if(paths.has(normalized))throw new Error("Duplicate fixture path "+normalized);
    paths.set(normalized,value);
    const name=normalized.split("/").at(-1)!;
    names.set(name,[...(names.get(name) ?? []),normalized]);
  }
  return {
    names:()=>[...paths.keys()].sort(),
    load:(source:string|undefined):unknown=>{
      const wanted=(source ?? "").trim().replace(/^\.?\//,"");
      if(!wanted)throw new Error("A fixture binding carries no source path");
      let exact=wanted;
      if(!paths.has(exact)&&!wanted.includes("/")){
        const candidates=names.get(wanted) ?? [];
        if(candidates.length>1)throw new Error("Ambiguous fixture name; use its full family path: "+wanted);
        exact=candidates[0] ?? wanted;
      }
      if(!paths.has(exact))throw new Error("No packaged fixture named "+wanted);
      return structuredClone(paths.get(exact));
    },
  };
}
