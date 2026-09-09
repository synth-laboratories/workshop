import test from 'node:test';
import assert from 'node:assert/strict';
import {VisualSession,initialSession} from '@synth/visuals-sdk';
import {eventCursorSchema,composeCursorSchema,pageMapSchema,pageTrailSchema,swarmHistorySchema,resolveEventCursor} from '../../packages/workshop-visuals/runtime/presentationSchemas.ts';
import {envelopeIdentity} from '../../packages/workshop-visuals/runtime/liveStream.ts';

test('persisted event selections rehydrate current evidence and reject copied bodies',()=>{
 const session=new VisualSession(initialSession({visualId:'cursor',revision:1,viewKey:'default'},{id:'cursor',version:'1'}));
 session.register({id:'cursor',label:'Cursor',...eventCursorSchema},null);
 const event={event_id:'one',run_id:'run',kind:'test',payload:{text:'original'}};
 const identity=envelopeIdentity(event,0);
 assert.equal(resolveEventCursor([event],identity),event);
 const current={...event,payload:{text:'current'}};
 assert.equal(resolveEventCursor([current],identity),current);
 assert.equal(resolveEventCursor([],identity),null);
 assert.equal(resolveEventCursor([event,event],identity),null);
 assert.throws(()=>session.execute({id:'bad',kind:'presentation.patch',expectedStateVersion:0,payload:{values:{cursor:{identity,event}}}}),/not allowed/);
});

test('navigation containers and compose cursors reject malformed imported values',()=>{
 for(const [schema,initial,bad] of [
  [composeCursorSchema,null,{identity:'a'}],
  [pageMapSchema,{}, {group:'one'}],
  [pageTrailSchema,[null],[{}]],
  [swarmHistorySchema,[],[{view:{filters:{outcome:'invented'},label:'Bad'},strategy:'random',eventIndex:0}]],
  [swarmHistorySchema,[],[{view:{filters:{},label:'All'},strategy:'random',eventIndex:0,cohort:{count:999}}]],
 ]){
  const session=new VisualSession(initialSession({visualId:'cursor',revision:1,viewKey:'default'},{id:'cursor',version:'1'}));
  session.register({id:'cursor',label:'Cursor',...schema},initial);
  assert.throws(()=>session.execute({id:'bad',kind:'presentation.patch',expectedStateVersion:0,payload:{values:{cursor:bad}}}));
  assert.deepEqual(session.state.values.cursor,initial);
 }
});

test('swarm history restores intent without trusting persisted aggregate counts',()=>{
 const session=new VisualSession(initialSession({visualId:'history',revision:1,viewKey:'default'},{id:'history',version:'1'}));
 session.register({id:'history',label:'History',...swarmHistorySchema},[]);
 const history=[{view:{filters:{outcome:'success'},label:'Success'},strategy:'representative',eventIndex:4,selectedId:'trajectory-1'}];
 session.execute({id:'restore',kind:'presentation.patch',expectedStateVersion:0,payload:{values:{history}}});
 assert.deepEqual(session.state.values.history,history);
 assert.throws(()=>session.execute({id:'too-deep',kind:'presentation.patch',expectedStateVersion:1,payload:{values:{history:Array(17).fill(history[0])}}}));
});
