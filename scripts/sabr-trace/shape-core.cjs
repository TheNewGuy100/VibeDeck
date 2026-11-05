// Sanitized structural description of an HTTP request: parameter names, protobuf field
// numbers, wire types and length classes. Mirrors src-tauri/src/sabr/request_shape.rs so
// a browser trace and a native dry run can be compared field by field. Values are never
// copied into the output.
(function (root) {
  const MAX_PARAMS = 64;
  const MAX_HEADERS = 24;
  const MAX_NAME = 24;
  const MAX_FIELDS = 256;

  function lenClass(len) {
    if (len === 0) return '0';
    if (len <= 15) return '1-15';
    if (len <= 255) return '16-255';
    if (len <= 4095) return '256-4095';
    return '>=4096';
  }

  function safeName(name) {
    return /^[A-Za-z0-9_.-]+$/.test(name) && name.length <= MAX_NAME ? name : 'other';
  }

  const utf8Length = (text) => new TextEncoder().encode(text).length;

  function describeUrl(rawUrl) {
    const url = new URL(rawUrl);
    const params = [];
    let truncated = false;
    for (const [name, value] of url.searchParams) {
      if (params.length === MAX_PARAMS) {
        truncated = true;
        break;
      }
      params.push({ name: safeName(name), lenClass: lenClass(utf8Length(value)) });
    }
    return {
      hostClass: url.hostname.endsWith('.googlevideo.com') ? 'googlevideo' : 'other',
      pathIsVideoplayback: url.pathname === '/videoplayback',
      params,
      paramsTruncated: truncated,
    };
  }

  // Same field numbers as the Rust SABR_REQUEST_EXPAND: streamer context (19) -> client info (1).
  const SABR_REQUEST_EXPAND = [{ number: 19, children: [{ number: 1, children: [] }] }];

  function readVarint(bytes, offset) {
    let value = 0;
    let shift = 0;
    for (let i = 0; i < 10; i++) {
      if (offset + i >= bytes.length) throw new Error('sabr_shape_invalid_protobuf');
      const byte = bytes[offset + i];
      value += (byte & 0x7f) * 2 ** shift;
      shift += 7;
      if ((byte & 0x80) === 0) return [value, offset + i + 1];
    }
    throw new Error('sabr_shape_invalid_protobuf');
  }

  function mergeShapes(into, from) {
    for (const incoming of from) {
      const existing = into.find((f) => f.number === incoming.number);
      if (!existing) {
        into.push(incoming);
        continue;
      }
      existing.count += incoming.count;
      existing.childrenUnparsed += incoming.childrenUnparsed;
      for (const cls of incoming.lenClasses) {
        if (!existing.lenClasses.includes(cls)) existing.lenClasses.push(cls);
      }
      mergeShapes(existing.children, incoming.children);
    }
    into.sort((a, b) => a.number - b.number);
  }

  function describeMessage(bytes, expand) {
    const byNumber = new Map();
    let offset = 0;
    let seen = 0;
    while (offset < bytes.length) {
      let tag;
      [tag, offset] = readVarint(bytes, offset);
      const number = Math.floor(tag / 8);
      const wireType = tag % 8;
      if (++seen > MAX_FIELDS) throw new Error('sabr_shape_too_many_fields');
      let wire;
      let inner = null;
      if (wireType === 0) {
        wire = 'varint';
        [, offset] = readVarint(bytes, offset);
      } else if (wireType === 1) {
        wire = 'fixed64';
        offset += 8;
      } else if (wireType === 5) {
        wire = 'fixed32';
        offset += 4;
      } else if (wireType === 2) {
        wire = 'bytes';
        let length;
        [length, offset] = readVarint(bytes, offset);
        if (offset + length > bytes.length) throw new Error('sabr_shape_invalid_protobuf');
        inner = bytes.subarray(offset, offset + length);
        offset += length;
      } else {
        throw new Error('sabr_shape_invalid_protobuf');
      }
      if (offset > bytes.length) throw new Error('sabr_shape_invalid_protobuf');
      let entry = byNumber.get(number);
      if (!entry) {
        entry = { number, wire, count: 0, lenClasses: [], children: [], childrenUnparsed: 0 };
        byNumber.set(number, entry);
      }
      entry.count += 1;
      if (inner) {
        const cls = lenClass(inner.length);
        if (!entry.lenClasses.includes(cls)) entry.lenClasses.push(cls);
        const spec = expand.find((e) => e.number === number);
        if (spec) {
          try {
            mergeShapes(entry.children, describeMessage(inner, spec.children));
          } catch (_) {
            entry.childrenUnparsed += 1;
          }
        }
      }
    }
    return [...byNumber.values()].sort((a, b) => a.number - b.number);
  }

  function describeRequest(rawUrl, headerNames, body) {
    const names = [...new Set(headerNames.map((h) => safeName(String(h).toLowerCase())))]
      .sort()
      .slice(0, MAX_HEADERS);
    let shape = [];
    let bodyParseError = null;
    try {
      shape = describeMessage(body, SABR_REQUEST_EXPAND);
    } catch (error) {
      bodyParseError = String(error.message || 'sabr_shape_invalid_protobuf');
    }
    return {
      url: describeUrl(rawUrl),
      headerNames: names,
      bodyLenClass: lenClass(body.length),
      body: shape,
      bodyParseError,
    };
  }

  // JSON bodies (the /player request): key names only, bounded depth, plus the length
  // class of string leaves. Used to see whether serviceIntegrityDimensions.poToken is sent.
  function describeJsonKeys(value, depth) {
    if (depth === 0 || value === null || typeof value !== 'object') {
      return typeof value === 'string' ? { string: lenClass(utf8Length(value)) } : typeof value;
    }
    if (Array.isArray(value)) return { array: value.length === 0 ? 0 : 1 };
    const out = {};
    for (const key of Object.keys(value).slice(0, 48)) {
      out[safeName(key)] = describeJsonKeys(value[key], depth - 1);
    }
    return out;
  }

  function statusClass(status) {
    if (status >= 200 && status <= 299) return '2xx';
    if (status >= 300 && status <= 399) return '3xx';
    if (status === 401 || status === 403) return '401_403';
    if (status === 429) return '429';
    if (status >= 400 && status <= 499) return '4xx';
    if (status >= 500 && status <= 599) return '5xx';
    return 'other';
  }

  function contentTypeClass(value) {
    const v = String(value || '').toLowerCase();
    if (!v) return 'none';
    if (v.includes('vnd.yt-ump')) return 'ump';
    if (v.includes('protobuf')) return 'protobuf';
    if (v.includes('html')) return 'html';
    if (v.includes('json')) return 'json';
    if (v.startsWith('text/')) return 'text';
    return 'other';
  }

  root.sabrShape = {
    lenClass,
    safeName,
    describeUrl,
    describeMessage,
    describeRequest,
    describeJsonKeys,
    statusClass,
    contentTypeClass,
    SABR_REQUEST_EXPAND,
  };
  if (typeof module !== 'undefined' && module.exports) module.exports = root.sabrShape;
})(typeof globalThis !== 'undefined' ? globalThis : this);
