// Dynamic tracing with logpoints over CDP, for obfuscated players that static reading
// cannot follow. Attaches to a page on a loopback debugging port, sets non-pausing
// breakpoints from a plan file and prints sanitized JSON lines. Never evaluates anything
// outside the plan, never pauses, never prints source or values.
//
//   node scripts/cdp-lab/cdp-lab.mjs targets [--port 9222]
//   node scripts/cdp-lab/cdp-lab.mjs run --plan <file> --origin https://music.youtube.com [--port 9222] [--seconds 120]
import fs from 'node:fs';
import process from 'node:process';
import { PAGE_HELPER, buildCondition, locateNeedle, sanitizeLogPayload, validatePlan } from './lab-core.mjs';

const args = process.argv.slice(2);
const command = args[0];
const option = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i === -1 ? fallback : args[i + 1];
};
const port = Number(option('port', process.env.VIBEDECK_CDP_PORT || '9222'));

async function listTargets() {
  const response = await fetch(`http://127.0.0.1:${port}/json/list`);
  return response.json();
}
const originOf = (url) => {
  try {
    return new URL(url).origin;
  } catch (_) {
    return 'unknown';
  }
};

if (command === 'targets') {
  for (const target of await listTargets()) console.log(JSON.stringify({ type: target.type, origin: originOf(target.url) }));
  process.exit(0);
}

if (command !== 'run') {
  console.error('usage: targets | run --plan <file> --origin <origin> [--port N] [--seconds N]');
  process.exit(2);
}

const plan = JSON.parse(fs.readFileSync(option('plan'), 'utf8'));
const problems = validatePlan(plan);
if (problems.length) {
  console.log(JSON.stringify({ error: 'plan_invalid', problems }));
  process.exit(2);
}
const origin = option('origin');
const target = (await listTargets()).find((t) => t.type === 'page' && originOf(t.url) === origin);
if (!target) {
  console.log(JSON.stringify({ error: 'target_not_found' }));
  process.exit(3);
}

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  ws.onopen = resolve;
  ws.onerror = () => reject(new Error('ws_open_failed'));
});
let nextId = 0;
const pending = new Map();
const scripts = [];
const started = Date.now();
ws.onmessage = (event) => {
  const message = JSON.parse(event.data);
  if (message.id && pending.has(message.id)) {
    pending.get(message.id)(message);
    pending.delete(message.id);
  } else if (message.method === 'Debugger.scriptParsed') {
    if (message.params.url.includes(plan.scriptUrlContains)) scripts.push(message.params.scriptId);
  } else if (message.method === 'Runtime.consoleAPICalled' && message.params.type === 'debug') {
    const [tag, label, payload] = message.params.args.map((a) => a.value);
    if (tag === '__lab') {
      console.log(JSON.stringify({ t: Date.now() - started, label, ...sanitizeLogPayload(String(payload)) }));
    }
  }
};
const send = (method, params = {}) =>
  new Promise((resolve) => {
    const id = ++nextId;
    pending.set(id, resolve);
    ws.send(JSON.stringify({ id, method, params }));
  });

await send('Runtime.enable');
await send('Debugger.enable');
await send('Runtime.evaluate', { expression: PAGE_HELPER });
await new Promise((r) => setTimeout(r, 1500)); // let scriptParsed events for existing scripts arrive

const installed = [];
for (const scriptId of scripts) {
  const source = (await send('Debugger.getScriptSource', { scriptId })).result?.scriptSource;
  if (!source) continue;
  for (const point of plan.points) {
    let location;
    try {
      location = locateNeedle(source, point.needle, point.offset ?? 0);
    } catch (_) {
      continue; // not in this script
    }
    const response = await send('Debugger.setBreakpoint', {
      location: { scriptId, ...location },
      condition: buildCondition(point.label, point.expression),
    });
    if (response.result?.breakpointId) installed.push(response.result.breakpointId);
  }
}
console.log(JSON.stringify({ installed: installed.length, of: plan.points.length }));

const seconds = Number(option('seconds', '120'));
await new Promise((r) => setTimeout(r, seconds * 1000));
for (const breakpointId of installed) await send('Debugger.removeBreakpoint', { breakpointId });
await send('Debugger.disable');
ws.close();
