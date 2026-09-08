import test from 'node:test';
import assert from 'node:assert/strict';
import {sourceText,renderSourceText} from './source-text.mjs';
test('source text resolves only supplied fields, preserving action IDs and source records',()=>{
  const input={id:'private',subject:'',subject_key:'be.msg.delegatedRenewals.subject',body:'',body_key:'be.msg.delegatedRenewals.body',i18n_params:{team:'London',successes:'2',stalled:'1',failures:'0'},actions:[{id:'respond',label:'',label_key:'unknown.key',resolved:false}]};
  const before=structuredClone(input);const output=renderSourceText(input);
  assert.equal(output.id,'private');assert.match(output.body,/London/);assert.match(output.body,/2/);
  assert.equal(output.actions[0].id,'respond');assert.equal(output.actions[0].label,'unknown.key');
  assert.deepEqual(input,before);assert.equal(sourceText('__proto__.toString'),'__proto__.toString');
});
