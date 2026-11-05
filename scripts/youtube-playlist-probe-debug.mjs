import { spawn } from 'node:child_process';
import { createConnection } from 'node:net';
import process from 'node:process';

const port = process.env.VIBEDECK_CDP_PORT || '9222';
const npmCommand = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const probeServerPort = 1420;

function launch() {
  const child = spawn(npmCommand, ['run', 'tauri', '--', 'dev'], {
    env: {
      ...process.env,
      VITE_YOUTUBE_PLAYLIST_PROBE: '1',
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
    shell: process.platform === 'win32',
    stdio: 'inherit',
  });

  console.log('[VibeDeck] Debug-only, read-only YouTube playlist probe enabled.');
  console.log(`[VibeDeck] WebView2 remote debugging: http://127.0.0.1:${port}`);

  function stopChild(signal) {
    if (!child.killed) child.kill(signal);
  }

  process.once('SIGINT', () => stopChild('SIGINT'));
  process.once('SIGTERM', () => stopChild('SIGTERM'));

  child.once('exit', (code, signal) => {
    process.exitCode = code ?? (signal ? 1 : 0);
  });
}

const server = createConnection({ host: 'localhost', port: probeServerPort });
server.once('connect', () => {
  server.destroy();
  console.error(`[VibeDeck] Port ${probeServerPort} is already in use. Close the active debug app before starting the probe.`);
  process.exitCode = 1;
});
server.once('error', (error) => {
  if (error.code === 'ECONNREFUSED') {
    launch();
    return;
  }
  console.error(`[VibeDeck] Could not check port ${probeServerPort}: ${error.code || 'unknown_error'}`);
  process.exitCode = 1;
});
