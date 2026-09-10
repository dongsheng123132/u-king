import assert from 'node:assert/strict';
import test from 'node:test';
import {readFile} from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join, resolve } from 'node:path';
const root=resolve(fileURLToPath(new URL('.',import.meta.url)),'..','..');
const patch=await readFile(join(root,'third_party','opentu','patches','0001-uking-local-project-bridge.patch'),'utf8');
const helperStart=patch.indexOf('+  const enqueueUkingSave = useCallback(');
const helperEnd=patch.indexOf('\n   const updateLatestBoardData',helperStart);
assert.ok(helperStart>=0&&helperEnd>helperStart,'patch must ship enqueueUkingSave');
const helper=patch.slice(helperStart,helperEnd).split('\n').filter(line=>line.startsWith('+')).map(line=>line.slice(1)).join('\n');
const bodyStart=helper.indexOf('      const uking = ukingRef.current;');
const bodyEnd=helper.indexOf('\n    },\n    [requestUking]',bodyStart);
assert.ok(bodyStart>=0&&bodyEnd>bodyStart,'test must execute the shipped save queue');
// Run the shipped queue with a delayed host, not a copy of its logic.
const body=helper.slice(bodyStart,bodyEnd).replace('return true;','return uking.saves;');
const enqueue=new Function('ukingRef','snapshot','window','requestUking',body);
function fixture(rejectFirst=false){
 const messages=[],calls=[];let version='v0';
 const bridge={projectId:'demo',parentOrigin:'http://localhost',stateVersion:version,ready:true,snapshot:{children:[]},saves:Promise.resolve(),pendingSaves:0};
 const window={parent:{postMessage:m=>messages.push(m.type)}};
 const request=async(_,input)=>{calls.push(input);await new Promise(r=>setTimeout(r,5));if(rejectFirst){rejectFirst=false;return {ok:false};}assert.equal(input.expected_state_version,version);version='v'+calls.length;return {ok:true,result:{state_version:version}};};
 return {bridge,messages,calls,save:data=>enqueue({current:bridge},data,window,request)};
}
test('rapid changes wait for acknowledged versions and keep immutable snapshots',async()=>{
 const f=fixture(),a={children:[{text:'first'}]};const one=f.save(a);a.children[0].text='mutated';const two=f.save({children:[{text:'second'}]});await Promise.all([one,two]);
 assert.deepEqual(f.calls.map(c=>c.expected_state_version),['v0','v1']);assert.equal(f.calls[0].canvas.children[0].text,'first');assert.equal(f.calls[1].canvas.children[0].text,'second');assert.equal(f.bridge.pendingSaves,0);assert.deepEqual(f.messages,['uking:bridge:saving','uking:bridge:saving','uking:bridge:saved']);
});
test('a rejected save remains visible and the next edit can retry without a poisoned queue',async()=>{
 const f=fixture(true);await f.save({children:[]});assert.equal(f.messages.at(-1),'uking:bridge:save-error');assert.equal(f.bridge.stateVersion,'v0');await f.save({children:[{text:'retry'}]});assert.equal(f.messages.at(-1),'uking:bridge:saved');assert.equal(f.bridge.pendingSaves,0);
});
test('upstream bootstrap cannot save before the host has restored its project',async()=>{
 const f=fixture();f.bridge.ready=false;await f.save({children:[]});assert.equal(f.calls.length,0);
});
function hunk(start,next){const from=patch.indexOf(start);assert.ok(from>=0,`missing ${start}`);const to=next?patch.indexOf(next,from):patch.length;return patch.slice(from,to<0?undefined:to);}
test('host mode blocks upstream switches and routes viewport through the bridge queue',()=>{
 const switchHunk=hunk('@@ -592','@@ -643');
 const navigationHunk=hunk('@@ -643','@@ -680');
 const syncHunk=hunk('@@ -680','@@ -787');
 const elementHunk=hunk('@@ -787','@@ -799');
 const viewportHunk=hunk('@@ -806','@@ -855');
 for(const part of [switchHunk,navigationHunk,syncHunk])assert.match(part,/\+\s*if \(isUkingHostMode\) return;/);
 assert.match(elementHunk,/\+\s*if \(isUkingHostMode\)[\s\S]*?\+\s*enqueueUkingSave\(uking\.snapshot\);[\s\S]*?\+\s*return;/);
 assert.match(viewportHunk,/\+\s*enqueueUkingSave\(uking\.snapshot\);/);
 assert.doesNotMatch(viewportHunk,/WorkspaceService/);
});
