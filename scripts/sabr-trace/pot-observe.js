// Observes how the REAL web player uses its PO token. Paste it into the Console of a page on
// music.youtube.com, then play two different tracks for ~30 s each. It only watches requests the
// page already makes; it changes none of them and mints nothing.
//
// Token values never leave this closure. Each distinct token gets a small index (1, 2, ...) and the
// summary reports indexes, length classes, counts and seconds. Where the token travels:
//   - SABR POST body: StreamerContext (field 19) -> field 2
//   - `pot` query parameter of googlevideo URLs
//   - /youtubei/v1/player request body: serviceIntegrityDimensions.poToken (gzip is unpacked here)
// plus the lifecycle calls to the attestation service (path name and status class only).
//
//   __po.summary()    counts, indexes, length classes and timings; no values
//   __po.uninstall()  restores fetch and XMLHttpRequest
(function () {
  const root = globalThis;
  if (root.__po) {
    console.log('[po] already installed');
    return;
  }
  const startedAt = Date.now();
  const secs = () => Math.round((Date.now() - startedAt) / 100) / 10;
  const MAX_EVENTS = 200;

  const tokens = new Map(); // key (base64url of the bytes) -> record
  const videos = new Map(); // opaque id from the SABR URL -> index
  const events = [];
  const waa = [];
  const counts = { sabrPosts: 0, sabrWithToken: 0, sabrWithoutToken: 0, potParams: 0, playerBodies: 0, playerBodiesGzip: 0, playerBodiesWithToken: 0 };

  const lenClass = (n) => (n === 0 ? '0' : n < 16 ? '1-15' : n < 256 ? '16-255' : n < 4096 ? '256-4095' : '4096+');
  const b64url = (bytes) => {
    let s = '';
    for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
    return btoa(s).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  };
  const fromB64url = (text) => {
    try {
      const std = String(text).replace(/-/g, '+').replace(/_/g, '/');
      const bin = atob(std + '='.repeat((4 - (std.length % 4)) % 4));
      const out = new Uint8Array(bin.length);
      for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
      return out;
    } catch (_) {
      return null;
    }
  };

  // How a token that travels as TEXT is written: never the value, only the alphabet and lengths.
  function textForm(text) {
    const t = String(text);
    const form = /^[A-Za-z0-9+/]+={0,2}$/.test(t) && /[+/]/.test(t)
      ? 'base64_standard'
      : /^[A-Za-z0-9_-]+={0,2}$/.test(t)
        ? (/[_-]/.test(t) ? 'base64url' : 'base64_alphanumeric_only')
        : 'not_base64';
    return { form, textLen: t.length, padded: /=$/.test(t), decodes: fromB64url(t) !== null };
  }

  function record(key, byteLength, where, video, text) {
    let t = tokens.get(key);
    if (!t) {
      t = { index: tokens.size + 1, bytes: byteLength, textForms: new Map(), firstSeenSecs: secs(), lastSeenSecs: secs(), sabrField2: 0, potParam: 0, playerBody: 0, videos: new Set() };
      tokens.set(key, t);
    }
    t.lastSeenSecs = secs();
    t[where] += 1;
    if (text !== undefined) {
      const tf = textForm(text);
      t.textForms.set(where + ':' + tf.form + ':' + tf.textLen + ':' + (tf.padded ? 'padded' : 'unpadded'), tf);
    }
    if (video) t.videos.add(video);
    return t.index;
  }
  function videoIndex(url) {
    try {
      const id = new URL(url).searchParams.get('id');
      if (!id) return 0;
      if (!videos.has(id)) videos.set(id, videos.size + 1);
      return videos.get(id);
    } catch (_) {
      return 0;
    }
  }
  function push(event) {
    events.push(event);
    if (events.length > MAX_EVENTS) events.splice(MAX_EVENTS / 2, 1);
  }

  // --- minimal protobuf reader: returns the first length-delimited value of `want` ---
  function readVarint(bytes, at) {
    let value = 0;
    let shift = 0;
    for (let i = at; i < bytes.length && shift < 35; i++) {
      value += (bytes[i] & 0x7f) * Math.pow(2, shift);
      if (!(bytes[i] & 0x80)) return [value, i + 1];
      shift += 7;
    }
    return null;
  }
  function field(bytes, want) {
    let at = 0;
    while (at < bytes.length) {
      const tag = readVarint(bytes, at);
      if (!tag) return null;
      at = tag[1];
      const number = Math.floor(tag[0] / 8);
      const wire = tag[0] % 8;
      if (wire === 0) {
        const v = readVarint(bytes, at);
        if (!v) return null;
        at = v[1];
      } else if (wire === 1) at += 8;
      else if (wire === 5) at += 4;
      else if (wire === 2) {
        const len = readVarint(bytes, at);
        if (!len) return null;
        const start = len[1];
        const end = start + len[0];
        if (end > bytes.length) return null;
        if (number === want) return bytes.subarray(start, end);
        at = end;
      } else return null;
    }
    return null;
  }

  const toBytes = (body) => {
    if (body instanceof ArrayBuffer) return new Uint8Array(body);
    if (ArrayBuffer.isView(body)) return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
    return null;
  };
  const isSabrUrl = (u) => {
    try {
      const url = new URL(u, root.location && root.location.href);
      return url.hostname.endsWith('.googlevideo.com') && url.pathname === '/videoplayback';
    } catch (_) {
      return false;
    }
  };
  const isGoogleVideo = (u) => {
    try {
      return new URL(u, root.location && root.location.href).hostname.endsWith('.googlevideo.com');
    } catch (_) {
      return false;
    }
  };
  const isPlayerUrl = (u) => /\/youtubei\/v1\/player(\?|$)/.test(String(u));
  const waaName = (u) => {
    try {
      const url = new URL(u, root.location && root.location.href);
      if (url.hostname !== 'jnn-pa.googleapis.com') return null;
      const last = url.pathname.split('/').pop();
      return /^[A-Za-z]{1,24}$/.test(last) ? last : 'other';
    } catch (_) {
      return null;
    }
  };

  function onSabrBody(url, bytes) {
    counts.sabrPosts += 1;
    const video = videoIndex(url);
    const context = field(bytes, 19);
    const token = context && field(context, 2);
    if (token && token.length) {
      counts.sabrWithToken += 1;
      const index = record(b64url(token), token.length, 'sabrField2', video);
      push({ t: secs(), kind: 'sabr', video, token: index, tokenLenClass: lenClass(token.length) });
    } else {
      counts.sabrWithoutToken += 1;
      push({ t: secs(), kind: 'sabr', video, token: 0 });
    }
  }
  function onUrl(url) {
    try {
      const pot = new URL(url, root.location && root.location.href).searchParams.get('pot');
      if (!pot) return;
      counts.potParams += 1;
      const bytes = fromB64url(pot);
      const index = bytes ? record(b64url(bytes), bytes.length, 'potParam', videoIndex(url), pot) : 0;
      push({ t: secs(), kind: 'pot_param', video: videoIndex(url), token: index, tokenLenClass: bytes ? lenClass(bytes.length) : 'undecodable' });
    } catch (_) {}
  }
  async function gunzip(bytes) {
    const stream = new Blob([bytes]).stream().pipeThrough(new DecompressionStream('gzip'));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  }
  async function onPlayerBody(bytesOrText) {
    try {
      let text = null;
      if (typeof bytesOrText === 'string') text = bytesOrText;
      else if (bytesOrText) {
        let bytes = bytesOrText;
        if (bytes.length > 2 && bytes[0] === 0x1f && bytes[1] === 0x8b) {
          counts.playerBodiesGzip += 1;
          bytes = await gunzip(bytes);
        }
        text = new TextDecoder().decode(bytes);
      }
      if (text === null) return;
      counts.playerBodies += 1;
      const token = JSON.parse(text)?.serviceIntegrityDimensions?.poToken;
      if (token) {
        counts.playerBodiesWithToken += 1;
        const bytes = fromB64url(token);
        const index = bytes ? record(b64url(bytes), bytes.length, 'playerBody', 0, token) : 0;
        push({ t: secs(), kind: 'player_body', token: index, tokenLenClass: bytes ? lenClass(bytes.length) : 'undecodable' });
      } else {
        push({ t: secs(), kind: 'player_body', token: 0 });
      }
    } catch (_) {
      push({ t: secs(), kind: 'player_body', token: 0, unreadable: true });
    }
  }
  // Any response whose URL contains one of these names is searched, in memory, for text that is or
  // contains a token already seen. Only key names, counts and the matched token index are reported.
  const watchNames = ['verify_session'];
  const watched = []; // { name, calls, statusClasses, strings: [{path, value}] }
  const watchFor = (url) => {
    const lower = String(url).toLowerCase();
    return watchNames.find((name) => lower.includes(name)) || null;
  };
  function collectStrings(value, path, out, depth) {
    if (out.length >= 400 || depth > 8) return;
    if (typeof value === 'string') {
      if (value.length >= 8 && value.length <= 4096) out.push({ path, value });
    } else if (Array.isArray(value)) {
      value.slice(0, 50).forEach((item, i) => collectStrings(item, path + '[]', out, depth + 1));
    } else if (value && typeof value === 'object') {
      for (const key of Object.keys(value).slice(0, 80)) {
        collectStrings(value[key], path + '.' + String(key).replace(/[^A-Za-z0-9_]/g, '_').slice(0, 40), out, depth + 1);
      }
    }
  }
  function onWatched(name, status, text) {
    let entry = watched.find((w) => w.name === name);
    if (!entry) watched.push((entry = { name, calls: 0, statusClasses: [], strings: [], notJson: 0 }));
    entry.calls += 1;
    entry.statusClasses.push(status);
    try {
      collectStrings(JSON.parse(text), '$', entry.strings, 0);
    } catch (_) {
      entry.notJson += 1;
      const raw = String(text).trim();
      if (raw.length >= 8 && raw.length <= 4096) entry.strings.push({ path: '(body)', value: raw });
    }
  }
  function watchedReport() {
    return watched.map((w) => {
      const matches = [];
      let base64Like = 0;
      for (const { path, value } of w.strings) {
        const bytes = fromB64url(value);
        if (bytes) base64Like += 1;
        const exact = bytes ? tokens.get(b64url(bytes)) : null;
        if (exact) matches.push({ path, token: exact.index, how: 'equal_after_base64_decode' });
        else {
          for (const [key, t] of tokens) {
            if (value.length > key.length && value.includes(key)) matches.push({ path, token: t.index, how: 'contained_as_text' });
          }
        }
      }
      return { name: w.name, calls: w.calls, statusClasses: w.statusClasses, notJson: w.notJson, stringsExamined: w.strings.length, base64Like, matches };
    });
  }

  function onWaa(name, statusClass) {
    waa.push({ t: secs(), call: name, statusClass });
    if (waa.length > 40) waa.shift();
  }
  const statusClass = (s) => (s >= 200 && s < 300 ? '2xx' : s === 401 || s === 403 ? '401_403' : s === 429 ? '429' : s >= 400 && s < 500 ? '4xx' : s >= 500 ? '5xx' : 'other');

  // --- fetch (Request objects are cloned before the native fetch consumes them) ---
  const origFetch = root.fetch;
  root.fetch = function (input, init) {
    const req = typeof Request !== 'undefined' && input instanceof Request ? input : null;
    const url = req ? req.url : typeof input === 'string' ? input : String(input && input.url ? input.url : input);
    const method = String((init && init.method) || (req && req.method) || 'GET').toUpperCase();
    try {
      if (isGoogleVideo(url)) onUrl(url);
      if (method === 'POST' && (isSabrUrl(url) || isPlayerUrl(url))) {
        const sabr = isSabrUrl(url);
        if (sabr) videoIndex(url); // numbered in call order, not in body-read order
        const initBody = init && init.body;
        let read;
        if (initBody !== undefined && initBody !== null) {
          const bytes = typeof initBody === 'string' ? initBody : toBytes(initBody);
          read = Promise.resolve(bytes);
          if (!bytes && typeof Blob !== 'undefined' && initBody instanceof Blob) read = initBody.arrayBuffer().then((b) => new Uint8Array(b));
        } else if (req) read = req.clone().arrayBuffer().then((b) => new Uint8Array(b));
        if (read) read.then((body) => (sabr ? body && typeof body !== 'string' && onSabrBody(url, body) : onPlayerBody(body))).catch(() => {});
      }
    } catch (_) {}
    const result = origFetch.apply(this, arguments);
    const watchName = watchFor(url);
    if (watchName) {
      result
        .then((r) => r.clone().text().then((text) => onWatched(watchName, statusClass(r.status), text)))
        .catch(() => {});
    }
    const name = waaName(url);
    if (name) result.then((r) => onWaa(name, statusClass(r.status))).catch(() => onWaa(name, 'network_error'));
    return result;
  };

  // --- XMLHttpRequest ---
  const XHR = root.XMLHttpRequest;
  const origOpen = XHR.prototype.open;
  const origSend = XHR.prototype.send;
  XHR.prototype.open = function (method, url) {
    this.__po = { method: String(method).toUpperCase(), url: String(url) };
    return origOpen.apply(this, arguments);
  };
  XHR.prototype.send = function (body) {
    const st = this.__po;
    try {
      if (st) {
        if (isGoogleVideo(st.url)) onUrl(st.url);
        if (st.method === 'POST' && isSabrUrl(st.url)) {
          const bytes = toBytes(body);
          if (bytes) onSabrBody(st.url, bytes);
        } else if (st.method === 'POST' && isPlayerUrl(st.url)) {
          onPlayerBody(typeof body === 'string' ? body : toBytes(body));
        }
        const name = waaName(st.url);
        if (name) this.addEventListener('loadend', () => onWaa(name, statusClass(this.status)));
        const watchName = watchFor(st.url);
        if (watchName) {
          this.addEventListener('loadend', () => {
            try {
              onWatched(watchName, statusClass(this.status), this.responseType === '' || this.responseType === 'text' ? this.responseText : '');
            } catch (_) {}
          });
        }
      }
    } catch (_) {}
    return origSend.apply(this, arguments);
  };

  function summary() {
    const list = [...tokens.values()].map((t) => ({
      token: t.index,
      bytes: t.bytes,
      lenClass: lenClass(t.bytes),
      firstSeenSecs: t.firstSeenSecs,
      lastSeenSecs: t.lastSeenSecs,
      seenInSabrField2: t.sabrField2,
      seenAsPotParam: t.potParam,
      seenInPlayerBody: t.playerBody,
      videos: [...t.videos].sort(),
      textForms: [...t.textForms.values()],
    }));
    const sabrTokenIndexes = new Set(events.filter((e) => e.kind === 'sabr' && e.token).map((e) => e.token));
    const perVideo = new Map();
    for (const e of events) {
      if (e.kind !== 'sabr' || !e.video) continue;
      if (!perVideo.has(e.video)) perVideo.set(e.video, new Set());
      if (e.token) perVideo.get(e.video).add(e.token);
    }
    const tokensPerVideo = [...perVideo.entries()]
      .map(([video, set]) => ({ video, distinctTokens: set.size }))
      .sort((a, b) => a.video - b.video);
    const shared = [...sabrTokenIndexes].some((idx) => list.find((t) => t.token === idx && t.videos.length > 1));
    return {
      installedSecsAgo: secs(),
      counts: { ...counts, distinctTokens: tokens.size, videosSeen: videos.size },
      tokens: list,
      tokensPerVideo,
      verdict: {
        everySabrPostCarriesAToken: counts.sabrPosts > 0 && counts.sabrWithoutToken === 0,
        sameTokenAcrossVideos: videos.size > 1 && shared,
        tokenChangesWithinAVideo: tokensPerVideo.some((v) => v.distinctTokens > 1),
        sabrTokenAlsoSeenAsPotParam: list.some((t) => t.seenInSabrField2 > 0 && t.seenAsPotParam > 0),
        sabrTokenAlsoSeenInPlayerBody: list.some((t) => t.seenInSabrField2 > 0 && t.seenInPlayerBody > 0),
      },
      attestationCalls: waa.slice(),
      watched: watchedReport(),
      timeline: events.slice(-40),
    };
  }

  root.__po = {
    summary,
    // Also search responses whose URL contains `name` (letters, digits, _ and - only).
    watch(name) {
      if (/^[A-Za-z0-9_-]{3,40}$/.test(String(name)) && !watchNames.includes(String(name).toLowerCase())) watchNames.push(String(name).toLowerCase());
      return watchNames.slice();
    },
    uninstall() {
      root.fetch = origFetch;
      XHR.prototype.open = origOpen;
      XHR.prototype.send = origSend;
      tokens.clear();
      delete root.__po;
    },
  };
  console.log('[po] installed: play a track for ~30 s, then another one for ~30 s');
})();
