// Page-side half of the `n` validation. Self-contained; paste it into the console of a page on
// music.youtube.com, then play a track that starts AFTER the paste. It remembers, in this closure
// only, the `n` values the /player response hands out and the `n` values the SABR POSTs actually
// send. Nothing with a value is written anywhere: `compare` answers with booleans and lengths.
//
//   __nv.received()      the received `n` values, for the LOCAL solver (copy them yourself)
//   __nv.stats()         counts only
//   __nv.compare(pairs)  pairs = [[received, solverOutput], ...] from the local solver; returns
//                        booleans: did each solver output appear among the `n` the page really sent
//   __nv.uninstall()     restores fetch and XMLHttpRequest
(function () {
  const root = globalThis;
  if (root.__nv) {
    console.log('[nv] already installed');
    return;
  }
  const received = new Set();
  const sent = new Set();
  const stats = { playerExchanges: 0, sabrRequests: 0 };

  const isSabrUrl = (u) => {
    try {
      const url = new URL(u, root.location && root.location.href);
      return url.hostname.endsWith('.googlevideo.com') && url.pathname === '/videoplayback';
    } catch (_) {
      return false;
    }
  };
  const isPlayerUrl = (u) => /\/youtubei\/v1\/player(\?|$)/.test(String(u));
  const nOf = (u) => {
    try {
      return new URL(u, root.location && root.location.href).searchParams.get('n');
    } catch (_) {
      return null;
    }
  };

  function onPlayerText(text) {
    try {
      const abr = JSON.parse(text)?.streamingData?.serverAbrStreamingUrl;
      stats.playerExchanges += 1;
      const n = abr && nOf(abr);
      if (n) received.add(n);
    } catch (_) {}
  }
  function onSabrUrl(url) {
    stats.sabrRequests += 1;
    const n = nOf(url);
    if (n) sent.add(n);
  }

  const origFetch = root.fetch;
  root.fetch = function (input, init) {
    const req = typeof Request !== 'undefined' && input instanceof Request ? input : null;
    const url = req ? req.url : String(input && input.url ? input.url : input);
    const method = String((init && init.method) || (req && req.method) || 'GET').toUpperCase();
    const result = origFetch.apply(this, arguments);
    if (method === 'POST' && isSabrUrl(url)) onSabrUrl(url);
    if (method === 'POST' && isPlayerUrl(url)) {
      result.then((r) => r.clone().text()).then(onPlayerText).catch(() => {});
    }
    return result;
  };

  const XHR = root.XMLHttpRequest;
  const origOpen = XHR.prototype.open;
  const origSend = XHR.prototype.send;
  XHR.prototype.open = function (method, url) {
    this.__nv = { method: String(method).toUpperCase(), url: String(url) };
    return origOpen.apply(this, arguments);
  };
  XHR.prototype.send = function () {
    const st = this.__nv;
    if (st && st.method === 'POST' && isSabrUrl(st.url)) onSabrUrl(st.url);
    if (st && st.method === 'POST' && isPlayerUrl(st.url)) {
      this.addEventListener('loadend', () => {
        try {
          onPlayerText(this.responseType === '' || this.responseType === 'text' ? this.responseText : '');
        } catch (_) {}
      });
    }
    return origSend.apply(this, arguments);
  };

  root.__nv = {
    received: () => [...received],
    stats: () => ({ ...stats, receivedUnique: received.size, sentUnique: sent.size }),
    compare(pairs) {
      // Say what was pasted without echoing any of it, so a mix-up is easy to diagnose.
      if (typeof pairs === 'string') {
        try {
          pairs = JSON.parse(pairs);
        } catch (_) {
          return { error: 'pasted_text_not_json' };
        }
      }
      if (pairs && typeof pairs === 'object' && !Array.isArray(pairs)) {
        return { error: 'solver_reported_error', code: String(pairs.error || 'unknown').slice(0, 80) };
      }
      if (!Array.isArray(pairs)) return { error: 'not_an_array' };
      if (pairs.length === 0) return { error: 'empty_pairs' };
      if (pairs.every((p) => typeof p === 'string')) {
        return {
          error: 'got_the_received_list_not_pairs',
          hint: 'the local solver has not replaced the clipboard yet (wait for Done) or it failed',
        };
      }
      if (!pairs.every((p) => Array.isArray(p) && p.length === 2 && typeof p[0] === 'string' && typeof p[1] === 'string')) {
        return { error: 'not_pairs' };
      }
      const perPair = pairs.map(([input, output], index) => ({
        index,
        inputWasReceived: received.has(input),
        outputWasSent: sent.has(output),
        inLen: String(input).length,
        outLen: String(output).length,
      }));
      const matched = perPair.filter((p) => p.outputWasSent).length;
      const explained = new Set(pairs.map((p) => p[1]).filter((o) => sent.has(o)));
      return {
        pairs: pairs.length,
        matchedPairs: matched,
        sentUnique: sent.size,
        sentExplainedBySolver: explained.size,
        verdict:
          pairs.length > 0 && matched === pairs.length
            ? 'all_received_map_to_sent'
            : matched > 0
              ? 'partial'
              : 'none',
        perPair,
      };
    },
    uninstall() {
      root.fetch = origFetch;
      XHR.prototype.open = origOpen;
      XHR.prototype.send = origSend;
      delete root.__nv;
    },
  };
  console.log('[nv] installed: play a NEW track now');
})();
