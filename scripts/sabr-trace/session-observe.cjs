// Passive recorder for one full, accepted web-player session. Read-only: it wraps fetch/XHR,
// clones what the real player already sent and received, and keeps NUMBERS ONLY (rounds,
// timings, part ids, sizes, numeric policy fields). Strings, urls, tokens, cookies and media
// bytes are never stored. Install in the page, play a track normally, then read
// `__so.summary()`; remove with `__so.uninstall()`.
(function () {
  'use strict';
  const MAX_ROUNDS = 60;

  // ---- pure helpers (also exported for tests) ----
  function readVarint(b, o) {
    let v = 0, mul = 1, i = o;
    while (i < b.length) {
      const x = b[i++];
      v += (x & 0x7f) * mul;
      if (!(x & 0x80)) return [v, i];
      mul *= 128;
      if (mul > 2 ** 49) return null;
    }
    return null;
  }
  // Wire-level protobuf walk. Numeric fields keep their value; everything else only a length.
  function walk(b, keepValuesDepth, depth = 0) {
    const out = [];
    let o = 0;
    while (o < b.length) {
      const key = readVarint(b, o);
      if (!key) break;
      o = key[1];
      const f = Math.floor(key[0] / 8), wt = key[0] % 8;
      if (wt === 0) {
        const v = readVarint(b, o);
        if (!v) break;
        o = v[1];
        out.push({ f, v: v[0] });
      } else if (wt === 2) {
        const l = readVarint(b, o);
        if (!l || l[1] + l[0] > b.length) break;
        const entry = { f, len: l[0] };
        if (depth < keepValuesDepth) {
          const inner = walk(b.subarray(l[1], l[1] + l[0]), keepValuesDepth, depth + 1);
          if (inner.length && inner.every((x) => x.v !== undefined || x.len !== undefined)) entry.in = inner;
        }
        o = l[1] + l[0];
        out.push(entry);
      } else if (wt === 1) { o += 8; out.push({ f, fixed: 8 }); }
      else if (wt === 5) { o += 4; out.push({ f, fixed: 4 }); }
      else break;
    }
    return out;
  }
  // Request: top-level field 1 (client state) keeps numeric values; the rest only lengths.
  function describeRequestBody(bytes) {
    const top = walk(bytes, 0);
    const counts = {};
    for (const x of top) counts[x.f] = (counts[x.f] || 0) + 1;
    const state = bytes.length ? walk(bytes, 1).find((x) => x.f === 1) : null;
    const ranges = walk(bytes, 1).filter((x) => x.f === 3).length;
    const clientState = state && state.in ? state.in.filter((x) => x.v !== undefined).map((x) => [x.f, x.v]) : [];
    return { bytes: bytes.length, fieldCounts: counts, clientState, bufferedRanges: ranges };
  }
  function umpVarint(b, o) {
    if (o >= b.length) return null;
    const f = b[o];
    const size = f < 0x80 ? 1 : f < 0xc0 ? 2 : f < 0xe0 ? 3 : f < 0xf0 ? 4 : f < 0xf8 ? 5 : 0;
    if (!size || o + size > b.length) return null;
    let v;
    if (size === 1) v = f;
    else if (size === 2) v = (f & 0x3f) | (b[o + 1] << 6);
    else if (size === 3) v = (f & 0x1f) | (b[o + 1] << 5) | (b[o + 2] << 13);
    else if (size === 4) v = (f & 0x0f) | (b[o + 1] << 4) | (b[o + 2] << 12) | (b[o + 3] << 20);
    else v = (b[o + 1]) | (b[o + 2] << 8) | (b[o + 3] << 16) | (b[o + 4] * 2 ** 24);
    return [v, o + size];
  }
  const NUMERIC_PARTS = new Set([20, 35, 44, 45, 57, 58, 59]);
  function describeUmp(bytes) {
    const parts = {}, media = { segments: 0, bytes: 0 }, numeric = [];
    let o = 0, truncated = false;
    while (o < bytes.length) {
      const id = umpVarint(bytes, o);
      if (!id) { truncated = true; break; }
      const len = umpVarint(bytes, id[1]);
      if (!len || len[1] + len[0] > bytes.length) { truncated = true; break; }
      const payload = bytes.subarray(len[1], len[1] + len[0]);
      parts[id[0]] = (parts[id[0]] || 0) + 1;
      if (id[0] === 21) { media.segments += 1; media.bytes += payload.length; }
      if (NUMERIC_PARTS.has(id[0]) && numeric.length < 40) {
        numeric.push({ part: id[0], fields: walk(payload, 0).filter((x) => x.v !== undefined).map((x) => [x.f, x.v]) });
      }
      o = len[1] + len[0];
    }
    return { responseBytes: bytes.length, parts, media, numeric, truncated };
  }

  const core = { readVarint, walk, describeRequestBody, umpVarint, describeUmp };
  if (typeof module !== 'undefined' && module.exports) { module.exports = core; return; }

  // ---- page hook ----
  const root = globalThis;
  if (root.__so) return;
  const t0 = Date.now();
  const rounds = [];
  const isSabr = (u) => {
    try { const x = new URL(u, root.location.href); return x.hostname.endsWith('.googlevideo.com') && x.pathname === '/videoplayback'; }
    catch (_) { return false; }
  };
  const media = () => { const v = document.querySelector('video') || document.querySelector('audio'); return v ? { t: Math.round(v.currentTime * 1000), paused: v.paused, buffered: v.buffered.length ? Math.round(v.buffered.end(v.buffered.length - 1) * 1000) : 0 } : null; };
  const statusClass = (s) => (s >= 200 && s < 300 ? '2xx' : s === 401 || s === 403 ? '401_403' : s === 429 ? '429' : s >= 400 && s < 500 ? '4xx' : s >= 500 ? '5xx' : 'other');
  const begin = (url, bytes) => {
    const entry = { n: rounds.length, atMs: Date.now() - t0, rn: null, player: media(), req: null, res: null };
    try { entry.rn = Number(new URL(url).searchParams.get('rn')); } catch (_) {}
    try { entry.req = describeRequestBody(bytes); } catch (_) { entry.req = { error: 'shape_failed' }; }
    if (rounds.length < MAX_ROUNDS) rounds.push(entry);
    return entry;
  };
  const finish = (entry, status, buf) => {
    entry.res = { status: statusClass(status), doneMs: Date.now() - t0 };
    try { Object.assign(entry.res, describeUmp(new Uint8Array(buf))); } catch (_) { entry.res.error = 'parse_failed'; }
  };

  const origFetch = root.fetch;
  root.fetch = function (input, init) {
    const req = input instanceof Request ? input : null;
    const url = req ? req.url : String(input && input.url ? input.url : input);
    const method = String((init && init.method) || (req && req.method) || 'GET').toUpperCase();
    if (method !== 'POST' || !isSabr(url)) return origFetch.apply(this, arguments);
    let bodyP;
    try {
      const body = init && init.body;
      bodyP = body instanceof ArrayBuffer ? Promise.resolve(new Uint8Array(body))
        : ArrayBuffer.isView(body) ? Promise.resolve(new Uint8Array(body.buffer, body.byteOffset, body.byteLength))
        : req ? req.clone().arrayBuffer().then((b) => new Uint8Array(b)) : Promise.resolve(new Uint8Array());
    } catch (_) { bodyP = Promise.resolve(new Uint8Array()); }
    const holder = { e: null };
    bodyP.then((b) => { holder.e = begin(url, b); }).catch(() => {});
    const result = origFetch.apply(this, arguments);
    result.then((r) => r.clone().arrayBuffer().then((buf) => {
      const wait = () => (holder.e ? finish(holder.e, r.status, buf) : setTimeout(wait, 5));
      wait();
    })).catch(() => {});
    return result;
  };

  const XHR = root.XMLHttpRequest;
  const oOpen = XHR && XHR.prototype.open, oSend = XHR && XHR.prototype.send;
  if (XHR) {
    XHR.prototype.open = function (m, u) { this.__so = { m: String(m).toUpperCase(), u: String(u) }; return oOpen.apply(this, arguments); };
    XHR.prototype.send = function (body) {
      const st = this.__so;
      if (st && st.m === 'POST' && isSabr(st.u) && (body instanceof ArrayBuffer || ArrayBuffer.isView(body))) {
        const b = body instanceof ArrayBuffer ? new Uint8Array(body) : new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
        const e = begin(st.u, b);
        this.addEventListener('loadend', () => { try { finish(e, this.status, this.response instanceof ArrayBuffer ? this.response : new ArrayBuffer(0)); } catch (_) {} });
      }
      return oSend.apply(this, arguments);
    };
  }

  root.__so = {
    summary: () => JSON.parse(JSON.stringify({ rounds, count: rounds.length })),
    uninstall: () => { if (origFetch) root.fetch = origFetch; if (XHR) { XHR.prototype.open = oOpen; XHR.prototype.send = oSend; } delete root.__so; },
  };
})();
