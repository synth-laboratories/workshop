import assert from 'node:assert/strict';
import test from 'node:test';
import {observeOptimizerVisual} from '../../packages/workshop-visuals/runtime/optimizerOrchestration.ts';
const settle=()=>new Promise(resolve=>setTimeout(resolve,0));
test('optimizer orchestration retains evidence and verifies before writing readiness',async()=>{
 let emit;const order=[],frames=[],diagnostics=[];
 const stop=observeOptimizerVisual({subscribe:listener=>{emit=listener;return()=>order.push('unsubscribed');},project:s=>({payload:s.run?{id:s.run.id}:null,progress:null}),onFrame:f=>frames.push(f),onDiagnostic:kind=>diagnostics.push(kind),readReceipt:async()=>{order.push('read');return 'previous';},verifyReceipt:receipt=>{order.push('verify '+receipt);return 'verified';},onReceipt:()=>order.push('publish'),recordReady:async()=>order.push('ready')});
 emit({run:{id:'r'},state:'subscribed',viewV2:{}});await settle();emit({run:null,state:'interrupted'});
 assert.deepEqual(order,['read','verify previous','publish','ready']);assert.equal(frames.at(-1).payload.id,'r');assert.deepEqual(diagnostics,['interrupted']);
 emit({run:{id:'r'},state:'terminal',viewV2:{}});await settle();assert.equal(order.filter(item=>item==='ready').length,1);stop();
});
test('unmounted or revised optimizer projections cannot write late readiness',async()=>{
 let emit,finish;const order=[];
 const stop=observeOptimizerVisual({subscribe:listener=>{emit=listener;return()=>{};},project:s=>({payload:s.run,progress:null}),onFrame:()=>{},onDiagnostic:()=>{},readReceipt:()=>new Promise(resolve=>finish=resolve),verifyReceipt:()=>order.push('verify'),onReceipt:()=>order.push('publish'),recordReady:async()=>order.push('ready')});
 emit({run:{id:'r'},state:'terminal',viewV2:{}});stop();finish(null);await settle();assert.deepEqual(order,[]);
});
