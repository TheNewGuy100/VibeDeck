import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { PAGE_HELPER, buildCondition, locateNeedle, sanitizeLogPayload, validatePlan } from './lab-core.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

test('the shipped example plan is valid', () => {
  const plan = JSON.parse(fs.readFileSync(path.join(here, 'plans', 'streamer-context.example.json'), 'utf8'));
  assert.deepEqual(validatePlan(plan), []);
});

test('plans with side effects, network or storage are rejected', () => {
  const base = { scriptUrlContains: '/s/player/', points: [{ label: 'a', needle: 'abcdefgh', expression: 'true' }] };
  assert.deepEqual(validatePlan(base), []);
  for (const expression of [
    'fetch("/x")', 'document.cookie', 'localStorage.x', 'x = 1', 'eval("1")', 'a.b = c', 'navigator.userAgent',
    'new WebSocket("ws://x")', 'import("x")', 'for(;;){}', 'delete a.b', 'location.href',
  ]) {
    assert.notDeepEqual(validatePlan({ ...base, points: [{ ...base.points[0], expression }] }), [], expression);
  }
  assert.deepEqual(validatePlan({ ...base, points: [{ ...base.points[0], expression: "a === b ? 'x' : (a >= 1)" }] }), []);
  assert.ok(validatePlan({ ...base, points: [] }).includes('points_invalid'));
  assert.ok(validatePlan({ ...base, points: [{ ...base.points[0], label: 'bad label' }] }).includes('points[0].label_invalid'));
  assert.ok(validatePlan({ ...base, scriptUrlContains: 'x' }).includes('scriptUrlContains_missing'));
});

test('locateNeedle finds a unique position across lines and columns', () => {
  const source = 'line0\nline1 foo(needle)bar\nline2\n';
  assert.deepEqual(locateNeedle(source, 'needle'), { lineNumber: 1, columnNumber: 10 });
  assert.deepEqual(locateNeedle(source, 'needle', 3), { lineNumber: 1, columnNumber: 13 });
  assert.deepEqual(locateNeedle('abcdef', 'cde'), { lineNumber: 0, columnNumber: 2 });
  assert.throws(() => locateNeedle(source, 'absent'), /needle_missing/);
  assert.throws(() => locateNeedle('xx needle xx needle', 'needle'), /needle_not_unique/);
});

test('the condition logs a tagged projection and always evaluates to false', () => {
  const logs = [];
  const sandbox = { console: { debug: (...a) => logs.push(a) }, globalThis: {} };
  vm.createContext(sandbox);
  vm.runInContext(PAGE_HELPER, sandbox);
  sandbox.b = { QG: new Uint8Array(100) };
  const expression = "({token: b.QG ? L(b.QG.length) : 'absent'})";
  assert.equal(vm.runInContext(buildCondition('streamerCtxToken', expression), sandbox), false);
  sandbox.b = {};
  vm.runInContext(buildCondition('streamerCtxToken', expression), sandbox);
  assert.deepEqual(logs[0], ['__lab', 'streamerCtxToken', '{"token":"16-255"}']);
  assert.deepEqual(logs[1], ['__lab', 'streamerCtxToken', '{"token":"absent"}']);
  // A throwing expression is reported as an error, not propagated and never pauses.
  assert.equal(vm.runInContext(buildCondition('boom', 'b.x.y.z'), sandbox), false);
  assert.deepEqual(logs[2], ['__lab', 'boom', '"error"']);
});

test('output is re-sanitized to a fixed vocabulary, so a leaked token cannot pass', () => {
  assert.deepEqual(sanitizeLogPayload('{"token":"16-255","kind":"present","n":3,"flag":true,"x":null}'), {
    token: '16-255', kind: 'present', n: 3, flag: true, x: null,
  });
  const leaked = sanitizeLogPayload('{"token":"AbCdEf123456","big":99999999,"neg":-1,"o":{"a":1},"bad key":"present"}');
  assert.deepEqual(leaked, { token: '[redacted]', big: '[redacted]', neg: '[redacted]', o: '[redacted]', other: '[redacted]' });
  assert.deepEqual(sanitizeLogPayload('not json'), { error: 'unparseable' });
  assert.deepEqual(sanitizeLogPayload('"abcdef"'), { value: '[redacted]' });
});
