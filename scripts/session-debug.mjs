// Use a separate entry page from the initial navigation: App.vue never mounts.
import { spawn } from 'node:child_process';
import process from 'node:process';

if (process.platform !== 'win32') throw new Error('session_debug_windows_only');
const config = JSON.stringify({ app: { windows: [{
  label: 'main', title: 'VibeDeck — debug de sessão', url: 'session-debug.html',
  width: 960, height: 720,
}] } });
// Invoke node CLI directly: no shell interpolation of inline JSON.
const child = spawn(process.execPath, ['node_modules/@tauri-apps/cli/tauri.js',
  'dev', '--config', config], { stdio: 'inherit', env: process.env });
child.once('error', () => { process.exitCode = 1; });
child.once('exit', code => { process.exitCode = code ?? 1; });
