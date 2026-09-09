export type OptimizerProjectionSnapshot={
 state:"loading"|"replaying"|"subscribed"|"reconnecting"|"stale"|"terminal"|"interrupted"|"failed"|"unavailable";
 run:unknown|null;viewV2?:unknown;error?:string|null;
};
export type OptimizerVisualFrame<P,G>={payload:P|null;progress:G|null;error:string|null;connection:Exclude<OptimizerProjectionSnapshot['state'],'unavailable'>};

/** Domain orchestration over host-owned durable read models. No private polling,
 * bridge globals, journal reconstruction, or second optimizer authority. A
 * receipt is read and compared BEFORE a new ready receipt can replace it. */
export function observeOptimizerVisual<S extends OptimizerProjectionSnapshot,P,G,R,V>(ports:{
 subscribe:(listener:(snapshot:S)=>void)=>()=>void;
 project:(snapshot:S)=>{payload:P|null;progress:G|null};
 onFrame:(frame:OptimizerVisualFrame<P,G>)=>void;
 onDiagnostic:(kind:"interrupted"|"stale",snapshot:S)=>void;
 readReceipt:()=>Promise<R>;
 verifyReceipt:(receipt:R,snapshot:S)=>V;
 onReceipt:(verdict:V)=>void;
 recordReady:(snapshot:S,signal:AbortSignal)=>Promise<unknown>;
 onReceiptError?:(error:unknown)=>void;
}):()=>void{
 const controller=new AbortController();
 let retained:P|null=null,receiptStarted=false,receiptComplete=false,latest:S|undefined;
 const unsubscribe=ports.subscribe(snapshot=>{
  if(controller.signal.aborted)return;
  latest=snapshot;
  const {payload,progress}=ports.project(snapshot);
  let error:string|null=null;
  let connection:OptimizerVisualFrame<P,G>['connection'];
  if(snapshot.state==='unavailable'){retained=payload;error=snapshot.error??'Optimizer bridge is unavailable';connection='failed';}
  else if(snapshot.state==='interrupted'||snapshot.state==='failed'){
   if(payload)retained=payload;error=snapshot.error??'Optimizer stream interrupted';connection=snapshot.state;
   ports.onDiagnostic('interrupted',snapshot);
  }else if(snapshot.state==='stale'){
   if(payload)retained=payload;connection='stale';ports.onDiagnostic('stale',snapshot);
  }else if(!snapshot.run||!payload){connection=snapshot.state==='loading'?'loading':'replaying';}
  else{retained=payload;connection=['terminal','reconnecting','replaying'].includes(snapshot.state)?snapshot.state as 'terminal'|'reconnecting'|'replaying':'subscribed';}
  ports.onFrame({payload:retained,progress,error,connection});
  if(!snapshot.run||!payload||!snapshot.viewV2||['unavailable','interrupted','failed','stale'].includes(snapshot.state)||receiptStarted||receiptComplete)return;
  receiptStarted=true;
  void (async()=>{
   const receipt=await ports.readReceipt();if(controller.signal.aborted)return;
   const current=latest;
   if(!current?.run||!current.viewV2||['unavailable','interrupted','failed','stale'].includes(current.state))return;
   const verdict=ports.verifyReceipt(receipt,current);if(controller.signal.aborted)return;
   ports.onReceipt(verdict);
   await ports.recordReady(current,controller.signal);
   if(!controller.signal.aborted)receiptComplete=true;
  })().catch(error=>{if(!controller.signal.aborted)ports.onReceiptError?.(error);})
   .finally(()=>{receiptStarted=false;});
 });
 return()=>{controller.abort();unsubscribe();};
}
