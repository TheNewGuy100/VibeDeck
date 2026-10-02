import { spawn } from 'node:child_process';
import process from 'node:process';

const port = process.env.VIBEDECK_CDP_PORT || '9222';
const npmCommand = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const additionalArgs = `--remote-debugging-port=${port}`;

const child = spawn(npmCommand, ['run', 'tauri', '--', 'dev'], {
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: additionalArgs,
  },
  shell: process.platform === 'win32',
  stdio: 'inherit',
});

console.log(`[VibeDeck] Tauri WebView2 remote debugging: http://127.0.0.1:${port}`);
console.log('[VibeDeck] Use Playwright connectOverCDP against this endpoint in development only.');

function stopChild(signal) {
  if (!child.killed) child.kill(signal);
}

process.once('SIGINT', () => stopChild('SIGINT'));
process.once('SIGTERM', () => stopChild('SIGTERM'));

child.once('exit', (code, signal) => {
  process.exitCode = code ?? (signal ? 1 : 0);
});
