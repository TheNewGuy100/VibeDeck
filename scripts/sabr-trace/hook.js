// Browser-side reference trace. Requires shape-core.cjs to have run first in the same page
// (it defines globalThis.sabrShape). Records, for the SABR POSTs and the /player exchange
// that a real play performs: parameter names, protobuf field numbers/length classes, header
// names, status class and, for `n`, only whether the value sent equals the value received.
// Nothing with a value is stored on window; the received `n` lives in this closure only.
(function () {
  const root = globalThis;
  if (!root.sabrShape) throw new Error('shape-core.cjs must be loaded first');
  if (root.__sabrTrace) return;
  const S = root.sabrShape;
  const MAX_SABR = 12;

  const trace = { installedAt: Date.now(), sabr: [], player: [], dropped: 0 };
  let playerN = null; // closure only
  let playerParamNames = null;

  const isSabrUrl = (u) => {
    try {
      const url = new URL(u, root.location && root.location.href);
      return url.hostname.endsWith('.googlevideo.com') && url.pathname === '/videoplayback';
    } catch (_) {
      return false;
    }
  };
  const isPlayerUrl = (u) => /\/youtubei\/v1\/player(\?|$)/.test(String(u));

  const toBytes = (body) => {
    if (body instanceof ArrayBuffer) return new Uint8Array(body);
    if (ArrayBuffer.isView(body)) return new Uint8Array(body.buffer, body.byteOffset, body.byteLength);
    return null;
  };

  function remember(list, entry) {
    list.push(entry);
    if (list.length > MAX_SABR) {
      list.splice(MAX_SABR / 2, 1);
      trace.dropped += 1;
    }
  }

  function nRelation(rawUrl) {
    if (playerN === null) return 'unknown';
    try {
      const n = new URL(rawUrl).searchParams.get('n');
      if (n === null) return 'absent_in_request';
      return n === playerN ? 'equal' : 'different';
    } catch (_) {
      return 'unknown';
    }
  }

  function paramNameDiff(rawUrl) {
    if (!playerParamNames) return null;
    const names = new Set([...new URL(rawUrl).searchParams.keys()].map(S.safeName));
    return {
      addedByClient: [...names].filter((n) => !playerParamNames.has(n)).sort(),
      removedByClient: [...playerParamNames].filter((n) => !names.has(n)).sort(),
    };
  }

  function recordSabr(url, headerNames, bodyBytes) {
    const entry = { request: null, nRelation: nRelation(url), paramNameDiff: null, response: null };
    try {
      entry.request = S.describeRequest(url, headerNames, bodyBytes);
      entry.paramNameDiff = paramNameDiff(url);
    } catch (error) {
      entry.request = { error: String(error.message || 'shape_failed') };
    }
    remember(trace.sabr, entry);
    return entry;
  }

  function onPlayerExchange(requestText, responseText) {
    const entry = { request: null, response: null };
    try {
      const request = JSON.parse(requestText);
      entry.request = {
        keys: S.describeJsonKeys(request, 3),
        hasServiceIntegrityPoToken: Boolean(
          request && request.serviceIntegrityDimensions && request.serviceIntegrityDimensions.poToken,
        ),
        poTokenLenClass:
          request && request.serviceIntegrityDimensions && request.serviceIntegrityDimensions.poToken
            ? S.lenClass(String(request.serviceIntegrityDimensions.poToken).length)
            : null,
      };
    } catch (_) {
      entry.request = { error: 'not_json' };
    }
    try {
      const response = JSON.parse(responseText);
      const sd = response && response.streamingData;
      const abr = sd && sd.serverAbrStreamingUrl;
      entry.response = {
        status: response && response.playabilityStatus && response.playabilityStatus.status,
        hasServerAbrUrl: Boolean(abr),
        responsePoTokenPresent: Boolean(response && response.serviceIntegrityDimensions),
      };
      if (abr) {
        const url = new URL(abr);
        playerN = url.searchParams.get('n');
        playerParamNames = new Set([...url.searchParams.keys()].map(S.safeName));
        entry.response.abrUrl = S.describeUrl(abr);
      }
    } catch (_) {
      entry.response = { error: 'not_json' };
    }
    trace.player.push(entry);
    if (trace.player.length > 6) trace.player.shift();
  }

  // --- XMLHttpRequest ---
  const XHR = root.XMLHttpRequest;
  const origOpen = XHR && XHR.prototype.open;
  const origSend = XHR && XHR.prototype.send;
  const origSet = XHR && XHR.prototype.setRequestHeader;
  if (XHR) {
    XHR.prototype.open = function (method, url) {
      this.__st = { method: String(method).toUpperCase(), url: String(url), headers: [] };
      return origOpen.apply(this, arguments);
    };
    XHR.prototype.setRequestHeader = function (name) {
      if (this.__st) this.__st.headers.push(String(name));
      return origSet.apply(this, arguments);
    };
    XHR.prototype.send = function (body) {
      const st = this.__st;
      let entry = null;
      if (st && st.method === 'POST' && isSabrUrl(st.url)) {
        const bytes = toBytes(body);
        if (bytes) entry = recordSabr(st.url, st.headers, bytes);
      }
      if (st && st.method === 'POST' && isPlayerUrl(st.url) && typeof body === 'string') {
        this.addEventListener('loadend', () => {
          try {
            onPlayerExchange(body, this.responseType === '' || this.responseType === 'text' ? this.responseText : '');
          } catch (_) {}
        });
      }
      if (entry) {
        this.addEventListener('loadend', () => {
          try {
            const size = this.response && this.response.byteLength;
            entry.response = {
              statusClass: S.statusClass(this.status),
              contentTypeClass: S.contentTypeClass(this.getResponseHeader('content-type')),
              headerNames: (this.getAllResponseHeaders() || '')
                .split(/\r?\n/)
                .map((l) => l.split(':')[0].trim().toLowerCase())
                .filter(Boolean)
                .map(S.safeName)
                .sort(),
              bodyLenClass: typeof size === 'number' ? S.lenClass(size) : null,
            };
          } catch (_) {}
        });
      }
      return origSend.apply(this, arguments);
    };
  }

  // --- fetch ---
  // The player often calls fetch(new Request(...)), so the body is on the Request, not in
  // init. The Request is cloned synchronously, before the native fetch consumes it.
  const namesOf = (h) => {
    if (!h) return [];
    if (typeof Headers !== "undefined" && h instanceof Headers) return [...h.keys()];
    if (Array.isArray(h)) return h.map((pair) => pair[0]);
    return Object.keys(h);
  };
  const readBody = (req, initBody, kind) => {
    if (initBody !== undefined && initBody !== null) {
      if (typeof initBody === "string") return Promise.resolve(kind === "text" ? initBody : null);
      const bytes = toBytes(initBody);
      if (bytes) return Promise.resolve(kind === "bytes" ? bytes : new TextDecoder().decode(bytes));
      if (typeof Blob !== "undefined" && initBody instanceof Blob) {
        return initBody.arrayBuffer().then((b) => (kind === "bytes" ? new Uint8Array(b) : new TextDecoder().decode(b)));
      }
      return Promise.resolve(null);
    }
    if (req) {
      const clone = req.clone();
      return kind === "bytes" ? clone.arrayBuffer().then((b) => new Uint8Array(b)) : clone.text();
    }
    return Promise.resolve(null);
  };
  const origFetch = root.fetch;
  if (origFetch) {
    root.fetch = function (input, init) {
      const req = typeof Request !== "undefined" && input instanceof Request ? input : null;
      const url = req ? req.url : typeof input === "string" ? input : String(input && input.url ? input.url : input);
      const method = String((init && init.method) || (req && req.method) || "GET").toUpperCase();
      const sabr = method === "POST" && isSabrUrl(url);
      const player = method === "POST" && isPlayerUrl(url);
      const headerNames = namesOf((init && init.headers) || (req && req.headers));
      let entry = null;
      let bodyPromise = null;
      if (sabr || player) {
        try {
          bodyPromise = readBody(req, init && init.body, sabr ? "bytes" : "text");
        } catch (_) {
          bodyPromise = Promise.resolve(null);
        }
      }
      if (sabr) {
        entry = { request: null, nRelation: nRelation(url), paramNameDiff: null, response: null };
        remember(trace.sabr, entry);
        bodyPromise
          .then((bytes) => {
            if (!bytes) return;
            try {
              entry.request = S.describeRequest(url, headerNames, bytes);
              entry.paramNameDiff = paramNameDiff(url);
            } catch (error) {
              entry.request = { error: String(error.message || "shape_failed") };
            }
          })
          .catch(() => {});
      }
      const result = origFetch.apply(this, arguments);
      if (player) {
        Promise.all([bodyPromise, result.then((r) => r.clone().text())])
          .then(([requestText, responseText]) => onPlayerExchange(requestText || "", responseText))
          .catch(() => {});
      }
      if (entry) {
        result
          .then((r) => {
            entry.response = {
              statusClass: S.statusClass(r.status),
              contentTypeClass: S.contentTypeClass(r.headers.get("content-type")),
              headerNames: [...r.headers.keys()].map(S.safeName).sort(),
              bodyLenClass: null,
            };
          })
          .catch(() => {});
      }
      return result;
    };
  }

  root.__sabrTrace = trace;
  root.__sabrTraceSummary = () => JSON.parse(JSON.stringify(trace));
  root.__sabrTraceUninstall = () => {
    if (XHR) {
      XHR.prototype.open = origOpen;
      XHR.prototype.send = origSend;
      XHR.prototype.setRequestHeader = origSet;
    }
    if (origFetch) root.fetch = origFetch;
    playerN = null;
    playerParamNames = null;
    delete root.__sabrTrace;
    delete root.__sabrTraceSummary;
    delete root.__sabrTraceUninstall;
    delete root.sabrShape;
  };
})();
