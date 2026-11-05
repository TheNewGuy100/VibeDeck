import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import test from 'node:test';
const require = createRequire(import.meta.url);
const so = require('./session-observe.cjs');

const varint = (n) => { const o = []; while (n > 127) { o.push((n & 127) | 128); n = Math.floor(n / 128); } o.push(n); return o; };
const field = (f, v) => [...varint(f * 8), ...varint(v)];
const bytesField = (f, b) => [...varint(f * 8 + 2), ...varint(b.length), ...b];

test('request: client state keeps numbers, other fields keep only lengths', () => {
  const state = [...field(28, 12345), ...field(4, 1)];
  const secret = Array.from(new TextEncoder().encode('SECRETTOKENVALUE'));
  const body = Uint8Array.from([...bytesField(1, state), ...bytesField(3, [8, 1]), ...bytesField(19, bytesField(2, secret))]);
  const d = so.describeRequestBody(body);
  assert.deepEqual(d.clientState, [[28, 12345], [4, 1]]);
  assert.equal(d.bufferedRanges, 1);
  assert.equal(d.fieldCounts[19], 1);
  assert.ok(!JSON.stringify(d).includes('SECRET'));
});

test('ump: counts parts, sums media bytes and keeps numeric policy fields only', () => {
  const ump = (n) => (n < 128 ? [n] : [0x80 | (n & 0x3f), n >> 6]);
  const part = (id, payload) => [...ump(id), ...ump(payload.length), ...payload];
  const policy = [...field(4, 5000), ...bytesField(9, [65, 66])];
  const bytes = Uint8Array.from([
    ...part(35, policy), ...part(21, new Array(300).fill(7)), ...part(21, new Array(5).fill(7)), ...part(58, field(1, 2)), ...part(62, []),
  ]);
  const d = so.describeUmp(bytes);
  assert.deepEqual(d.parts, { 35: 1, 21: 2, 58: 1, 62: 1 });
  assert.deepEqual(d.media, { segments: 2, bytes: 305 });
  assert.deepEqual(d.numeric, [{ part: 35, fields: [[4, 5000]] }, { part: 58, fields: [[1, 2]] }]);
  assert.equal(d.truncated, false);
});

test('ump: a cut response is flagged, not thrown', () => {
  assert.equal(so.describeUmp(Uint8Array.from([21, 50, 1, 2])).truncated, true);
});

test('two-byte ump ids and sizes decode like the Rust parser', () => {
  assert.deepEqual(so.umpVarint(Uint8Array.from([0x80 | 5, 2]), 0), [5 | (2 << 6), 2]);
});
