import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';

const html = readFileSync(new URL('../public/session-debug.html', import.meta.url), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];
function harness(invoke, confirmation = true) {
  const actions = ['open_login', 'capture_save', 'verify_snapshot', 'restore_verify'];
  const buttons = actions.map(action => ({ dataset: { action }, disabled: false,
    addEventListener(_event, handler) { this.click = handler; } }));
  const output = { textContent: '' };
  runInNewContext(script, {
    document: { querySelector: () => output, querySelectorAll: () => buttons },
    window: { __TAURI_INTERNALS__: { invoke } }, confirm: () => confirmation,
  });
  return { buttons, output };
}

test('entry page does not invoke anything until a button is used', async () => {
  const calls = [];
  const { buttons, output } = harness(async (...args) => { calls.push(args); return { stage: 'snapshot_loaded' }; });
  assert.equal(calls.length, 0);
  await buttons[2].click();
  assert.deepEqual(JSON.parse(JSON.stringify(calls)), [['debug_youtube_session', { action: 'verify_snapshot' }]]);
  assert.match(output.textContent, /snapshot_loaded/);
  assert.ok(buttons.every(button => !button.disabled));
});

test('capture confirmation cancellation never writes', async () => {
  let calls = 0;
  const { buttons } = harness(async () => { calls++; }, false);
  await buttons[1].click();
  assert.equal(calls, 0);
});

test('errors redact arbitrary provider data and never retry', async () => {
  let calls = 0;
  const { buttons, output } = harness(async () => { calls++; throw 'private-provider-cookie'; });
  await buttons[3].click();
  assert.equal(calls, 1);
  assert.equal(output.textContent, 'session_debug_unavailable');
  assert.ok(buttons.every(button => !button.disabled));
});
