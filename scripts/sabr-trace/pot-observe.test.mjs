import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, 'pot-observe.js'), 'utf8');

const varint = (n) => {
  const out = [];
  while (n > 127) {
    out.push((n & 127) | 128);
    n = Math.floor(n / 128);
  }
  out.push(n);
  return out;
};
const lenField = (number, bytes) => [...varint(number * 8 + 2), ...varint(bytes.length), ...bytes];
const sabrBody = (tokenBytes) => {
  const context = [...lenField(1, [1, 2, 3]), ...(tokenBytes ? lenField(2, tokenBytes) : [])];
  return new Uint8Array([...lenField(1, [9]), ...lenField(19, context)]);
};
const b64url = (bytes) => Buffer.from(bytes).toString('base64url');
const TOKEN_A = Array.from({ length: 40 }, (_, i) => (i * 7 + 3) & 255);
const TOKEN_B = Array.from({ length: 40 }, (_, i) => (i * 11 + 5) & 255);

class FakeXhr {
  constructor() {
    this.listeners = {};
    this.status = 200;
  }
  open(method, url) {
    this.m = method;
    this.u = url;
  }
  send() {}
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
}

async function withHook(run) {
  const saved = { fetch: globalThis.fetch, XMLHttpRequest: globalThis.XMLHttpRequest };
  globalThis.XMLHttpRequest = FakeXhr;
  globalThis.fetch = async () => new Response('x', { status: 200 });
  delete globalThis.__po;
  try {
    vm.runInThisContext(source);
    await run();
  } finally {
    globalThis.__po?.uninstall?.();
    globalThis.fetch = saved.fetch;
    globalThis.XMLHttpRequest = saved.XMLHttpRequest;
  }
}

const sabrPost = (id, body) =>
  globalThis.fetch(new Request(`https://rr1.googlevideo.com/videoplayback?sabr=1&id=${id}`, { method: 'POST', body }));
const settle = () => new Promise((r) => setTimeout(r, 30));

test('reports where the token travels and whether it is per video, without any value', async () => {
  await withHook(async () => {
    await sabrPost('videoOne', sabrBody(TOKEN_A));
    await sabrPost('videoOne', sabrBody(TOKEN_A));
    await sabrPost('videoTwo', sabrBody(TOKEN_B));
    await globalThis.fetch(`https://rr1.googlevideo.com/videoplayback?id=videoTwo&pot=${b64url(TOKEN_B)}`);
    const gz = zlib.gzipSync(JSON.stringify({ serviceIntegrityDimensions: { poToken: b64url(TOKEN_A) } }));
    await globalThis.fetch('https://music.youtube.com/youtubei/v1/player?prettyPrint=false', { method: 'POST', body: new Uint8Array(gz) });
    await globalThis.fetch('https://jnn-pa.googleapis.com/$rpc/google.internal.waa.v1.Waa/GenerateIT', { method: 'POST', body: '[]' });
    await settle();

    const s = globalThis.__po.summary();
    assert.equal(s.counts.sabrPosts, 3);
    assert.equal(s.counts.distinctTokens, 2);
    assert.equal(s.counts.videosSeen, 2);
    assert.equal(s.counts.playerBodiesGzip, 1);
    assert.equal(s.counts.playerBodiesWithToken, 1);
    assert.deepEqual(s.tokensPerVideo, [
      { video: 1, distinctTokens: 1 },
      { video: 2, distinctTokens: 1 },
    ]);
    assert.equal(s.verdict.everySabrPostCarriesAToken, true);
    assert.equal(s.verdict.sameTokenAcrossVideos, false);
    assert.equal(s.verdict.tokenChangesWithinAVideo, false);
    assert.equal(s.verdict.sabrTokenAlsoSeenAsPotParam, true);
    assert.equal(s.verdict.sabrTokenAlsoSeenInPlayerBody, true);
    assert.deepEqual(s.attestationCalls.map((c) => [c.call, c.statusClass]), [['GenerateIT', '2xx']]);
    const tokenA = s.tokens.find((t) => t.seenInPlayerBody === 1);
    assert.equal(tokenA.seenInSabrField2, 2);
    assert.equal(tokenA.lenClass, '16-255');

    const json = JSON.stringify(s);
    for (const secret of [b64url(TOKEN_A), b64url(TOKEN_B), 'videoOne', 'videoTwo']) {
      assert.ok(!json.includes(secret), 'a value leaked into the summary');
    }
  });
});

test('a token shared across videos, a missing token and a changing token are all told apart', async () => {
  await withHook(async () => {
    await sabrPost('videoOne', sabrBody(TOKEN_A));
    await sabrPost('videoTwo', sabrBody(TOKEN_A));
    await sabrPost('videoTwo', sabrBody(null));
    await sabrPost('videoTwo', sabrBody(TOKEN_B));
    await settle();
    const s = globalThis.__po.summary();
    assert.equal(s.verdict.sameTokenAcrossVideos, true);
    assert.equal(s.verdict.everySabrPostCarriesAToken, false);
    assert.equal(s.verdict.tokenChangesWithinAVideo, true);
    assert.equal(s.counts.sabrWithoutToken, 1);
  });
});

test('xhr bodies are read and uninstall restores the originals', async () => {
  await withHook(async () => {
    const patchedFetch = globalThis.fetch;
    const xhr = new FakeXhr();
    xhr.open('POST', 'https://rr1.googlevideo.com/videoplayback?id=videoOne');
    xhr.send(sabrBody(TOKEN_A));
    assert.equal(globalThis.__po.summary().counts.sabrWithToken, 1);
    vm.runInThisContext(source); // a second paste must not stack another wrapper
    assert.equal(globalThis.fetch, patchedFetch);
    globalThis.__po.uninstall();
    assert.notEqual(globalThis.fetch, patchedFetch);
    assert.equal(globalThis.__po, undefined);
  });
});

test('says how a text token is written and finds tokens inside watched responses, by index only', async () => {
  await withHook(async () => {
    const saved = globalThis.fetch;
    const textToken = b64url(TOKEN_A); // never padded: base64url alphabet
    const gz = zlib.gzipSync(JSON.stringify({ serviceIntegrityDimensions: { poToken: textToken } }));
    await globalThis.fetch('https://music.youtube.com/youtubei/v1/player', { method: 'POST', body: new Uint8Array(gz) });
    await sabrPost('videoOne', sabrBody(TOKEN_B));
    await settle();
    const before = globalThis.__po.summary();
    const playerToken = before.tokens.find((t) => t.seenInPlayerBody === 1);
    assert.ok(playerToken.textForms.some((f) => f.form.startsWith('base64') && f.decodes && f.textLen === textToken.length));
    assert.ok(!JSON.stringify(before).includes(textToken));

    // The watched call arrives later than the tokens; matching happens at summary time.
    const inner = JSON.stringify({ session: { token: textToken, other: 'short' }, note: `prefix-${b64url(TOKEN_B)}-suffix` });
    const realFetch = globalThis.fetch;
    globalThis.fetch = async () => new Response(inner, { status: 200 });
    // The hook wrapped the original fetch; replace what it wraps by calling through a fresh install.
    globalThis.__po.uninstall();
    globalThis.fetch = async () => new Response(inner, { status: 200 });
    vm.runInThisContext(source);
    await globalThis.fetch(new Request('https://music.youtube.com/youtubei/v1/verify_session', { method: 'POST', body: '{}' }));
    await sabrPost('videoOne', sabrBody(TOKEN_B));
    await globalThis.fetch(new Request('https://music.youtube.com/youtubei/v1/player', { method: 'POST', body: new Uint8Array(gz) }));
    await settle();
    const s = globalThis.__po.summary();
    const watched = s.watched.find((w) => w.name === 'verify_session');
    assert.equal(watched.calls, 1);
    assert.ok(watched.stringsExamined >= 2);
    assert.ok(watched.matches.some((m) => m.how === 'contained_as_text' || m.how === 'equal_after_base64_decode'));
    const json = JSON.stringify(s);
    for (const secret of [textToken, b64url(TOKEN_B)]) assert.ok(!json.includes(secret));
    globalThis.fetch = saved ?? realFetch;
  });
});
