import assert from 'node:assert/strict';
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const require = createRequire(import.meta.url);
const S = require('./shape-core.cjs');

function varint(value) {
  const out = [];
  let v = value;
  while (v > 127) {
    out.push((v % 128) | 0x80);
    v = Math.floor(v / 128);
  }
  out.push(v);
  return out;
}
const field = (number, bytes) => [...varint(number * 8 + 2), ...varint(bytes.length), ...bytes];
const varintField = (number, value) => [...varint(number * 8), ...varint(value)];

function sampleBody({ token = true } = {}) {
  const clientInfo = [...varintField(16, 67), ...field(17, [...Buffer.from('1.20260101.00.00')])];
  const entry = [...varintField(1, 3), ...field(2, [...Buffer.from('ctx')])];
  const context = [
    ...field(1, clientInfo),
    ...(token ? field(2, new Array(100).fill(7)) : []),
    ...field(5, entry),
    ...field(5, entry),
  ];
  return Uint8Array.from([
    ...field(1, [1, 2, 3]),
    ...field(5, new Array(944).fill(9)),
    ...field(16, [4, 5]),
    ...field(19, context),
  ]);
}

test('protobuf shape matches the Rust describer on the same sample', () => {
  const shape = S.describeMessage(sampleBody(), S.SABR_REQUEST_EXPAND);
  assert.deepEqual(shape.map((f) => f.number), [1, 5, 16, 19]);
  assert.deepEqual(shape.find((f) => f.number === 5).lenClasses, ['256-4095']);
  const ctx = shape.find((f) => f.number === 19);
  assert.deepEqual(ctx.lenClasses, ['16-255']); // ~150 bytes: client info + 100-byte token + 2 contexts
  assert.deepEqual(ctx.children.map((f) => f.number), [1, 2, 5]);
  assert.deepEqual(ctx.children.find((f) => f.number === 2).lenClasses, ['16-255']);
  assert.equal(ctx.children.find((f) => f.number === 5).count, 2);
  assert.deepEqual(ctx.children.find((f) => f.number === 1).children.map((f) => f.number), [16, 17]);
});

test('absence of the po token field is visible', () => {
  const has2 = (body) =>
    S.describeMessage(body, S.SABR_REQUEST_EXPAND)
      .find((f) => f.number === 19)
      .children.some((f) => f.number === 2);
  assert.equal(has2(sampleBody({ token: true })), true);
  assert.equal(has2(sampleBody({ token: false })), false);
});

test('url shape exports names and length classes, never values', () => {
  const shape = S.describeRequest(
    'https://r1.googlevideo.com/videoplayback?sig=SECRETSIGVALUE&n=abcdefghijk&sabr=1&weird%20name=x',
    ['Content-Type', 'accept', 'ACCEPT'],
    sampleBody(),
  );
  const json = JSON.stringify(shape);
  assert.ok(!json.includes('SECRETSIGVALUE') && !json.includes('abcdefghijk'));
  assert.deepEqual(shape.url.params.map((p) => p.name), ['sig', 'n', 'sabr', 'other']);
  assert.equal(shape.url.hostClass, 'googlevideo');
  assert.deepEqual(shape.headerNames, ['accept', 'content-type']);
});

test('malformed and oversized messages use fixed codes', () => {
  assert.throws(() => S.describeMessage(Uint8Array.of(0x08), []), /sabr_shape_invalid_protobuf/);
  const many = Uint8Array.from(Array.from({ length: 300 }, () => [0x08, 0x01]).flat());
  assert.throws(() => S.describeMessage(many, []), /sabr_shape_too_many_fields/);
  const nested = S.describeMessage(Uint8Array.from(field(19, [0x08])), S.SABR_REQUEST_EXPAND);
  assert.equal(nested[0].childrenUnparsed, 1);
});

test('status and content-type classes mirror the Rust helpers', () => {
  assert.deepEqual(
    [200, 302, 401, 403, 429, 404, 503, 100].map(S.statusClass),
    ['2xx', '3xx', '401_403', '401_403', '429', '4xx', '5xx', 'other'],
  );
  assert.deepEqual(
    ['', 'application/vnd.yt-ump', 'application/x-protobuf', 'text/html', 'application/json', 'text/plain', 'image/png'].map(S.contentTypeClass),
    ['none', 'ump', 'protobuf', 'html', 'json', 'text', 'other'],
  );
});

test('hook reads fetch(Request) and Blob bodies, which the player uses, without leaking values', async () => {
  const saved = { XMLHttpRequest: globalThis.XMLHttpRequest, fetch: globalThis.fetch };
  const consumed = [];
  globalThis.XMLHttpRequest = undefined;
  // Like the native fetch: consumes a Request body, so the hook must clone it first.
  globalThis.fetch = async (input) => {
    if (input instanceof Request) consumed.push(new Uint8Array(await input.arrayBuffer()).length);
    const url = input instanceof Request ? input.url : String(input);
    if (url.includes('/youtubei/v1/player')) {
      return new Response(
        JSON.stringify({
          playabilityStatus: { status: 'OK' },
          streamingData: { serverAbrStreamingUrl: 'https://rr1.googlevideo.com/videoplayback?sabr=1&n=NVALUE123&sig=SIGSECRET' },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      );
    }
    return new Response('denied', { status: 403, headers: { 'content-type': 'text/html' } });
  };
  delete globalThis.__sabrTrace;
  globalThis.sabrShape = S; // each hook test starts from a page that loaded shape-core
  try {
    vm.runInThisContext(fs.readFileSync(path.join(here, 'hook.js'), 'utf8'));
    await globalThis.fetch(
      new Request('https://music.youtube.com/youtubei/v1/player?key=APIKEYSECRET', {
        method: 'POST',
        body: JSON.stringify({ serviceIntegrityDimensions: { poToken: 'POTOKENSECRETVALUE' } }),
      }),
    );
    await new Promise((r) => setTimeout(r, 20));
    await globalThis.fetch(
      new Request('https://rr1.googlevideo.com/videoplayback?sabr=1&n=NVALUE123&sig=SIGSECRET&rn=1', {
        method: 'POST',
        body: sampleBody(),
      }),
    );
    await globalThis.fetch('https://rr1.googlevideo.com/videoplayback?sabr=1&n=DIFFERENT&sig=SIGSECRET', {
      method: 'POST',
      body: new Blob([sampleBody({ token: false })]),
    });
    await new Promise((r) => setTimeout(r, 20));

    assert.deepEqual(consumed.length >= 2, true, 'the stub did consume the Request bodies');
    const summary = globalThis.__sabrTraceSummary();
    assert.equal(summary.player.length, 1);
    assert.equal(summary.player[0].request.hasServiceIntegrityPoToken, true);
    assert.equal(summary.sabr.length, 2);
    assert.equal(summary.sabr[0].nRelation, 'equal');
    assert.equal(summary.sabr[1].nRelation, 'different');
    const ctx = (s) => s.request.body.find((f) => f.number === 19);
    assert.ok(ctx(summary.sabr[0]).children.some((f) => f.number === 2));
    assert.ok(!ctx(summary.sabr[1]).children.some((f) => f.number === 2));
    assert.equal(summary.sabr[0].response.statusClass, '401_403');
    const json = JSON.stringify(summary);
    for (const secret of ['NVALUE123', 'DIFFERENT', 'SIGSECRET', 'APIKEYSECRET', 'POTOKENSECRETVALUE']) {
      assert.ok(!json.includes(secret), `${secret} leaked`);
    }
    globalThis.__sabrTraceUninstall();
  } finally {
    globalThis.XMLHttpRequest = saved.XMLHttpRequest;
    globalThis.fetch = saved.fetch;
    delete globalThis.sabrShape;
  }
});

class FakeXhr {
  constructor() {
    this.listeners = {};
    this.responseType = '';
    this.responseText = '';
    this.response = null;
    this.status = 0;
    this.respHeaders = {};
  }
  open(method, url) {
    this.m = method;
    this.u = url;
  }
  setRequestHeader() {}
  send() {}
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  getResponseHeader(name) {
    return this.respHeaders[name.toLowerCase()] ?? null;
  }
  getAllResponseHeaders() {
    return Object.entries(this.respHeaders).map(([k, v]) => `${k}: ${v}`).join('\r\n');
  }
  finish() {
    for (const fn of this.listeners.loadend || []) fn();
  }
}

test('hook records shapes and the n relation without storing any value', () => {
  const saved = { XMLHttpRequest: globalThis.XMLHttpRequest, fetch: globalThis.fetch };
  globalThis.XMLHttpRequest = FakeXhr;
  globalThis.fetch = undefined;
  delete globalThis.__sabrTrace;
  globalThis.sabrShape = S; // each hook test starts from a page that loaded shape-core
  try {
    vm.runInThisContext(fs.readFileSync(path.join(here, 'hook.js'), 'utf8'));

    const player = new FakeXhr();
    player.open('POST', 'https://music.youtube.com/youtubei/v1/player?key=APIKEYSECRET');
    player.send(JSON.stringify({ context: { client: { clientName: 'WEB_REMIX' } }, serviceIntegrityDimensions: { poToken: 'POTOKENSECRETVALUE' } }));
    player.responseText = JSON.stringify({
      playabilityStatus: { status: 'OK' },
      streamingData: { serverAbrStreamingUrl: 'https://rr1.googlevideo.com/videoplayback?sabr=1&n=NVALUE123&sig=SIGSECRET' },
    });
    player.finish();

    const sabr = new FakeXhr();
    sabr.open('POST', 'https://rr1.googlevideo.com/videoplayback?sabr=1&n=NVALUE123&sig=SIGSECRET&rn=1');
    sabr.send(sampleBody().buffer);
    sabr.status = 403;
    sabr.respHeaders = { 'content-type': 'text/html' };
    sabr.finish();

    const other = new FakeXhr();
    other.open('POST', 'https://rr1.googlevideo.com/videoplayback?sabr=1&n=DIFFERENT&sig=SIGSECRET');
    other.send(sampleBody({ token: false }).buffer);
    other.finish();

    const summary = globalThis.__sabrTraceSummary();
    assert.equal(summary.sabr.length, 2);
    assert.equal(summary.sabr[0].nRelation, 'equal');
    assert.equal(summary.sabr[1].nRelation, 'different');
    assert.deepEqual(summary.sabr[0].paramNameDiff, { addedByClient: ['rn'], removedByClient: [] });
    assert.equal(summary.sabr[0].response.statusClass, '401_403');
    assert.equal(summary.sabr[0].response.contentTypeClass, 'html');
    assert.equal(summary.player[0].request.hasServiceIntegrityPoToken, true);
    assert.equal(summary.player[0].request.poTokenLenClass, '16-255');
    assert.equal(summary.player[0].response.hasServerAbrUrl, true);
    const ctx = (s) => s.request.body.find((f) => f.number === 19);
    assert.ok(ctx(summary.sabr[0]).children.some((f) => f.number === 2));
    assert.ok(!ctx(summary.sabr[1]).children.some((f) => f.number === 2));

    const json = JSON.stringify(summary);
    for (const secret of ['NVALUE123', 'DIFFERENT', 'SIGSECRET', 'APIKEYSECRET', 'POTOKENSECRETVALUE']) {
      assert.ok(!json.includes(secret), `${secret} leaked`);
    }
    assert.ok(!('playerN' in globalThis.__sabrTrace));

    globalThis.__sabrTraceUninstall();
    assert.equal(globalThis.__sabrTrace, undefined);
    assert.equal(globalThis.sabrShape, undefined);
  } finally {
    globalThis.XMLHttpRequest = saved.XMLHttpRequest;
    globalThis.fetch = saved.fetch;
    delete globalThis.sabrShape;
  }
});
