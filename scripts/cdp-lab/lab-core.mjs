// Pure helpers for cdp-lab: no network, no browser. A "plan" lists logpoints; each logs a
// sanitized projection (length classes, presence, equality) and never pauses the page.

const CLASS_VOCAB = new Set(['0', '1-15', '16-255', '256-4095', '>=4096']);
const WORD_VOCAB = new Set(['absent', 'present', 'equal', 'different', 'unknown', 'none', 'error']);
const KEY = /^[A-Za-z][A-Za-z0-9_]{0,23}$/;

// Evaluated inside the target page, so keep it small and free of side effects.
const FORBIDDEN = [
  /\bfetch\b/, /\bXMLHttpRequest\b/, /\bcookie\b/, /\blocalStorage\b/, /\bsessionStorage\b/,
  /\bindexedDB\b/, /\beval\b/, /\bFunction\b/, /\bimport\b/, /\brequire\b/, /\bWebSocket\b/,
  /\bsendBeacon\b/, /\bnavigator\b/, /\bpostMessage\b/, /\bwindow\.open\b/, /\blocation\b/,
  /\bdebugger\b/, /\bwhile\b/, /\bfor\b/, /\bdelete\b/,
  /(?<![=!<>])=(?![=>])/, // assignments
];

/** Returns the list of problems with a plan; empty means acceptable. */
export function validatePlan(plan) {
  const problems = [];
  if (!plan || typeof plan !== 'object') return ['plan_not_object'];
  if (typeof plan.scriptUrlContains !== 'string' || plan.scriptUrlContains.length < 3) {
    problems.push('scriptUrlContains_missing');
  }
  if (!Array.isArray(plan.points) || plan.points.length === 0 || plan.points.length > 16) {
    problems.push('points_invalid');
    return problems;
  }
  const labels = new Set();
  plan.points.forEach((point, i) => {
    const at = `points[${i}]`;
    if (!KEY.test(String(point.label))) problems.push(`${at}.label_invalid`);
    if (labels.has(point.label)) problems.push(`${at}.label_duplicate`);
    labels.add(point.label);
    if (typeof point.needle !== 'string' || point.needle.length < 6 || point.needle.length > 200) {
      problems.push(`${at}.needle_invalid`);
    }
    if (point.offset !== undefined && !(Number.isInteger(point.offset) && point.offset >= 0 && point.offset <= 400)) {
      problems.push(`${at}.offset_invalid`);
    }
    if (typeof point.expression !== 'string' || point.expression.length === 0 || point.expression.length > 400) {
      problems.push(`${at}.expression_invalid`);
    } else if (FORBIDDEN.some((re) => re.test(point.expression))) {
      problems.push(`${at}.expression_forbidden`);
    }
  });
  return problems;
}

/** 0-based line/column of the single occurrence of `needle` (+ offset) in `source`. */
export function locateNeedle(source, needle, offset = 0) {
  const first = source.indexOf(needle);
  if (first === -1) throw new Error('needle_missing');
  if (source.indexOf(needle, first + 1) !== -1) throw new Error('needle_not_unique');
  const index = first + offset;
  let line = 0;
  let lastBreak = -1;
  for (let i = source.indexOf('\n'); i !== -1 && i < index; i = source.indexOf('\n', i + 1)) {
    line += 1;
    lastBreak = i;
  }
  return { lineNumber: line, columnNumber: index - lastBreak - 1 };
}

/** Breakpoint condition: logs a tagged line and returns false, so execution never pauses. */
export function buildCondition(label, expression) {
  return (
    '(function(){const L=globalThis.__labL;' +
    `try{console.debug('__lab',${JSON.stringify(label)},JSON.stringify((${expression})))}` +
    `catch(e){console.debug('__lab',${JSON.stringify(label)},'"error"')}return false})()`
  );
}

/** Helper installed in the page: maps a length to the same classes the Rust/JS shapes use. */
export const PAGE_HELPER =
  "globalThis.__labL=function(n){n=Number(n);if(!(n>=0))return 'unknown';" +
  "return n===0?'0':n<=15?'1-15':n<=255?'16-255':n<=4095?'256-4095':'>=4096'};true";

function sanitizeValue(value) {
  if (value === null || typeof value === 'boolean') return value;
  if (typeof value === 'number') return Number.isInteger(value) && value >= 0 && value <= 1_000_000 ? value : '[redacted]';
  if (typeof value === 'string') return CLASS_VOCAB.has(value) || WORD_VOCAB.has(value) ? value : '[redacted]';
  return '[redacted]';
}

/** Re-validates what the page logged; anything outside the vocabulary is replaced. */
export function sanitizeLogPayload(jsonText) {
  let parsed;
  try {
    parsed = JSON.parse(jsonText);
  } catch (_) {
    return { error: 'unparseable' };
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return { value: sanitizeValue(parsed) };
  }
  const out = {};
  for (const key of Object.keys(parsed).slice(0, 16)) {
    if (!KEY.test(key)) {
      out.other = '[redacted]';
      continue;
    }
    out[key] = sanitizeValue(parsed[key]);
  }
  return out;
}
