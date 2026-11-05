import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import test from 'node:test';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, 'validate-n.js'), 'utf8');

class FakeXhr {
  constructor() {
    this.listeners = {};
    this.responseType = '';
    this.responseText = '';
  }
  open(method, url) {
    this.m = method;
    this.u = url;
  }
  send() {}
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  finish() {
    for (const fn of this.listeners.loadend || []) fn();
  }
}

const playerResponse = (n) =>
  new Response(
    JSON.stringify({
      playabilityStatus: { status: 'OK' },
      streamingData: { serverAbrStreamingUrl: `https://rr1.googlevideo.com/videoplayback?sabr=1&n=${n}&sig=SIGSECRET` },
    }),
    { status: 200 },
  );

async function withHook(run) {
  const saved = { fetch: globalThis.fetch, XMLHttpRequest: globalThis.XMLHttpRequest };
  globalThis.XMLHttpRequest = FakeXhr;
  globalThis.fetch = async (input) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url.includes('/youtubei/v1/player')) return playerResponse(url.includes('first') ? 'RECEIVEDONE12345' : 'RECEIVEDTWO12345');
    return new Response('x', { status: 200 });
  };
  delete globalThis.__nv;
  try {
    vm.runInThisContext(source);
    await run();
  } finally {
    globalThis.__nv?.uninstall?.();
    globalThis.fetch = saved.fetch;
    globalThis.XMLHttpRequest = saved.XMLHttpRequest;
  }
}

test('compare answers with booleans: which solver outputs the page really sent', async () => {
  await withHook(async () => {
    await globalThis.fetch(new Request('https://music.youtube.com/youtubei/v1/player?first', { method: 'POST', body: '{}' }));
    await globalThis.fetch(new Request('https://music.youtube.com/youtubei/v1/player?second', { method: 'POST', body: '{}' }));
    await new Promise((r) => setTimeout(r, 20));
    assert.deepEqual(globalThis.__nv.received().sort(), ['RECEIVEDONE12345', 'RECEIVEDTWO12345']);

    // The page sends a transformed n for the first track only (a prefetched second one has not played yet).
    await globalThis.fetch(new Request('https://rr1.googlevideo.com/videoplayback?sabr=1&n=TRANSFORMED1&sig=S', { method: 'POST', body: 'b' }));
    const xhr = new FakeXhr();
    xhr.open('POST', 'https://rr1.googlevideo.com/videoplayback?sabr=1&n=TRANSFORMED1&rn=2');
    xhr.send();

    assert.deepEqual(globalThis.__nv.stats(), { playerExchanges: 2, sabrRequests: 2, receivedUnique: 2, sentUnique: 1 });

    const result = globalThis.__nv.compare([
      ['RECEIVEDONE12345', 'TRANSFORMED1'],
      ['RECEIVEDTWO12345', 'SOMETHINGELSE'],
    ]);
    assert.equal(result.matchedPairs, 1);
    assert.equal(result.verdict, 'partial');
    assert.deepEqual(result.perPair.map((p) => p.outputWasSent), [true, false]);
    assert.ok(result.perPair.every((p) => p.inputWasReceived));
    const json = JSON.stringify(result);
    for (const secret of ['RECEIVEDONE12345', 'RECEIVEDTWO12345', 'TRANSFORMED1', 'SOMETHINGELSE', 'SIGSECRET']) {
      assert.ok(!json.includes(secret), `${secret} leaked into the verdict`);
    }
    assert.equal(globalThis.__nv.compare([['RECEIVEDONE12345', 'TRANSFORMED1']]).verdict, 'all_received_map_to_sent');
    assert.equal(globalThis.__nv.compare([['RECEIVEDONE12345', 'NOPE']]).verdict, 'none');
    // A wrong paste is named, never echoed.
    const compare = (v) => globalThis.__nv.compare(v);
    assert.deepEqual(compare({ error: 'cache_empty' }), { error: 'solver_reported_error', code: 'cache_empty' });
    assert.equal(compare(['abcdefgh12', 'ijklmnop34']).error, 'got_the_received_list_not_pairs');
    assert.deepEqual(compare([]), { error: 'empty_pairs' });
    assert.deepEqual(compare('not json'), { error: 'pasted_text_not_json' });
    assert.deepEqual(compare('"just a string"'), { error: 'not_an_array' });
    assert.deepEqual(compare([[1, 2]]), { error: 'not_pairs' });
    assert.equal(compare('[["RECEIVEDONE12345","TRANSFORMED1"]]').verdict, 'all_received_map_to_sent');
    assert.ok(!JSON.stringify(compare(['RECEIVEDONE12345', 'TRANSFORMED1'])).includes('RECEIVEDONE12345'));
  });
});

test('install is idempotent and uninstall restores fetch', async () => {
  await withHook(async () => {
    const patched = globalThis.fetch;
    vm.runInThisContext(source); // second paste must not stack another wrapper
    assert.equal(globalThis.fetch, patched);
    globalThis.__nv.uninstall();
    assert.notEqual(globalThis.fetch, patched);
    assert.equal(globalThis.__nv, undefined);
  });
});
