import test from 'node:test';
import assert from 'node:assert/strict';
import {LIMITS, validateRequest} from './web/bridge-policy.js';
const wire = value => JSON.stringify({id:1,op:'echo',value});
test('UTF-8 wire boundary includes envelope and multibyte data', () => {
  const overhead = wire('').length;
  assert.equal(validateRequest(wire('x'.repeat(LIMITS.wireBytes-overhead))).id, 1);
  assert.throws(() => validateRequest(wire('x'.repeat(LIMITS.wireBytes-overhead+1))));
  assert.throws(() => validateRequest(wire('é'.repeat(17000))));
});
test('reject structural and schema abuse before host effects', () => {
  for (const raw of ['null','[]','{',
    '{"id":1,"op":"write","value":1e400}',
    '{"id":0,"op":"write","value":7}',
    '{"id":1,"op":"write","value":7,"generation":2}',
    '{"id":1,"op":"read","value":"secret"}',
    '{"id":1,"op":"__proto__","value":null}']) assert.throws(() => validateRequest(raw), raw);
});
test('bound deep and wide JSON independently of byte size', () => {
  let value = null;
  for (let i = 0; i < 15; i++) value = [value];
  assert.doesNotThrow(() => validateRequest(wire(value)));
  assert.throws(() => validateRequest(wire([[value]])));
  assert.throws(() => validateRequest(wire(Array(2100).fill(null))));
});
