import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

export interface AppLogEntry {
  id: number;
  timestamp: number;
  level: 'info' | 'error';
  source: string;
  window?: string | null;
  operation: string;
  message: string;
}

export interface AppWebviewInfo {
  label: string;
  name: string;
  visible: boolean;
  internal: boolean;
}

export interface PlaylistBridgeDebugState {
  playlistId?: string;
  requestId?: string;
  stage: string;
  initialTracks: number;
  lastPageTracks: number;
  totalTracks: number;
}

export function getAppLogs() {
  return invoke<AppLogEntry[]>('get_app_logs');
}

export function getYouTubePlaylistDebugState() {
  return invoke<PlaylistBridgeDebugState>('get_youtube_playlist_debug_state');
}

export function clearAppLogs() {
  return invoke('clear_app_logs');
}

export function writeAppLog(message: string) {
  return invoke('write_app_log', { message });
}

export function getAppWebviews() {
  return invoke<AppWebviewInfo[]>('get_app_webviews');
}

export function setAppWebviewVisible(label: string, visible: boolean) {
  return invoke('set_app_webview_visible', { label, visible });
}

export function exportAppLogs() {
  return invoke<string>('export_app_logs');
}

export function listenAppLogs(callback: (entry: AppLogEntry) => void): Promise<UnlistenFn> {
  return listen<AppLogEntry>('app-log', (event) => callback(event.payload));
}
