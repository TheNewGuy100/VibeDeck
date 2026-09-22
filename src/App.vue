<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { type LibraryItem, type LibrarySource, type Playlist, type PlaylistMode } from './models/library';
import { youtubeMusicSource } from './services/music-source';
import { googleSyncService } from './services/google-sync';
import { clearAppLogs, exportAppLogs, getAppLogs, getAppWebviews, getYouTubePlaylistDebugState, listenAppLogs, setAppWebviewVisible, writeAppLog, type AppLogEntry, type AppWebviewInfo } from './services/app-logs';

type Screen = 'home' | 'libraries' | 'playlist' | 'logs';
type UserSettings = { darkMode: boolean; developerMode: boolean };
type RepeatMode = 'off' | 'playlist' | 'track';

const userSettingsKey = 'vibedeck:user-settings';
const savedSettings = loadUserSettings();

const screen = ref<Screen>('libraries');
const query = ref('');
const selectedPlaylistId = ref('local');
const localTracks = ref<LibraryItem[]>([]);
const favoriteTracks = ref<LibraryItem[]>([]);
const onlinePlaylists = ref<Playlist[]>([]);
const searchPlaylist = ref<Playlist | null>(null);
const currentTrack = ref<LibraryItem | null>(null);
const queue = ref<LibraryItem[]>([]);
const isPlaying = ref(false);
const repeatMode = ref<RepeatMode>('off');
const nativeTrackEnded = ref(false);
const googleConnected = ref(youtubeMusicSource.isConnected());
const darkMode = ref(savedSettings.darkMode);
const developerMode = ref(savedSettings.developerMode);
const settingsOpen = ref(false);
const audio = ref<HTMLAudioElement>();
const fileInput = ref<HTMLInputElement>();
const folderInput = ref<HTMLInputElement>();
const currentTime = ref(0);
const duration = ref(0);
const volume = ref(0.8);
const isMuted = ref(false);
const usingNativeAudio = ref(false);
const dropTarget = ref<string | null>(null);
const playlistModes = ref<Record<string, PlaylistMode>>({ local: 'local', favorites: 'mixed' });
const playlistSettingsOpen = ref(false);
const playlistSettingsId = ref<string | null>(null);
const playlistSettingsMode = ref<PlaylistMode>('local');
const toasts = ref<{ id: number; message: string; tone: 'success' | 'error' }[]>([]);
const loadingPlaylistId = ref<string | null>(null);
const loadingMorePlaylistId = ref<string | null>(null);
const fullyLoadedPlaylists = ref(new Set<string>());
const failedPrefetchPlaylists = ref(new Set<string>());
const playlistLoadPromises = new Map<string, Promise<LibraryItem[]>>();
const appLogs = ref<AppLogEntry[]>([]);
const logSearch = ref('');
const logLevelFilter = ref<'all' | 'info' | 'error'>('all');
const logSourceFilter = ref('all');
const appWebviews = ref<AppWebviewInfo[]>([]);
const changingWebviewLabel = ref<string | null>(null);
let unlistenLogs: (() => void) | undefined;
let logsRefreshTimer: number | undefined;
let audioPositionTimer: number | undefined;
let nativeSeekTimer: number | undefined;
let nativeSeekHoldUntil = 0;
let seekControlObserver: MutationObserver | undefined;
let trackListElement: HTMLElement | undefined;

const playlists = computed<Playlist[]>(() => [
  ...(searchPlaylist.value ? [searchPlaylist.value] : []),
  ...onlinePlaylists.value,
  { id: 'local', name: 'Biblioteca local', source: 'local', mode: playlistModes.value.local, tracks: localTracks.value, allowLocal: playlistModes.value.local !== 'online' },
  { id: 'favorites', name: 'Favoritos', source: playlistModes.value.favorites, mode: playlistModes.value.favorites, tracks: favoriteTracks.value, allowLocal: playlistModes.value.favorites !== 'online' },
]);
const activePlaylist = computed(() => playlists.value.find((playlist) => playlist.id === selectedPlaylistId.value) ?? playlists.value[0]);
const filteredTracks = computed(() => {
  const normalizedQuery = query.value.trim().toLowerCase();
  return activePlaylist.value.tracks.filter((item) => !normalizedQuery || `${item.title} ${item.artist} ${item.album}`.toLowerCase().includes(normalizedQuery));
});
const isActivePlaylistLoading = computed(() => loadingPlaylistId.value === activePlaylist.value.id && !activePlaylist.value.tracks.length);
const progressPercent = computed(() => duration.value > 0 ? (currentTime.value / duration.value) * 100 : 0);
const libraryCards = computed(() => [
  { id: 'local', name: 'Biblioteca local', description: 'Arquivos armazenados neste computador.', count: localTracks.value.length, source: 'local' as const, accent: '#4ecba6' },
  { id: 'youtube', name: 'YouTube Music', description: googleConnected.value ? 'Biblioteca sincronizada com sua conta Google.' : 'Conecte sua conta Google para sincronizar.', count: onlinePlaylists.value.reduce((total, playlist) => total + playlist.tracks.length, 0), source: 'youtube' as const, accent: '#ff5f55' },
]);
const logSources = computed(() => Array.from(new Set(appLogs.value.map((entry) => entry.source))).sort());
const filteredLogs = computed(() => {
  const search = logSearch.value.trim().toLowerCase();
  return appLogs.value.filter((entry) => {
    if (logLevelFilter.value !== 'all' && entry.level !== logLevelFilter.value) return false;
    if (logSourceFilter.value !== 'all' && entry.source !== logSourceFilter.value) return false;
    return !search || `${entry.source} ${entry.window ?? ''} ${entry.operation} ${entry.message}`.toLowerCase().includes(search);
  }).toReversed();
});

function showToast(message: string, tone: 'success' | 'error' = 'error') {
  const id = Date.now() + Math.random();
  toasts.value.push({ id, message, tone });
  window.setTimeout(() => { toasts.value = toasts.value.filter((toast) => toast.id !== id); }, 4200);
}

function loadUserSettings(): UserSettings {
  try {
    const value = localStorage.getItem(userSettingsKey);
    if (!value) return { darkMode: true, developerMode: false };
    return { darkMode: true, developerMode: false, ...JSON.parse(value) } as UserSettings;
  } catch {
    return { darkMode: true, developerMode: false };
  }
}

function saveUserSettings() {
  localStorage.setItem(userSettingsKey, JSON.stringify({ darkMode: darkMode.value, developerMode: developerMode.value }));
}

function logUi(message: string) {
  void writeAppLog(message);
}

async function logPlaylistBridgeState(context: string) {
  try {
    const state = await getYouTubePlaylistDebugState();
    logUi(`playlist bridge state: context=${context}, playlist=${state.playlistId ?? 'none'}, request=${state.requestId ?? 'none'}, stage=${state.stage}, initial=${state.initialTracks}, page=${state.lastPageTracks}, total=${state.totalTracks}`);
  } catch {
    // Diagnostics must not affect playlist loading.
  }
}

function pushAppLog(entry: AppLogEntry) {
  if (appLogs.value.some((item) => item.id === entry.id)) return;
  appLogs.value = [...appLogs.value, entry].slice(-2000);
}

async function refreshAppLogs() {
  try {
    const logs = await getAppLogs();
    appLogs.value = logs;
  } catch {
    // The real-time listener is the primary path; polling is only a fallback.
  }
}

async function refreshAppWebviews() {
  try {
    appWebviews.value = await getAppWebviews();
  } catch {
    // The panel can remain usable while a transient native query fails.
  }
}

async function toggleWebviewVisibility(webview: AppWebviewInfo) {
  changingWebviewLabel.value = webview.label;
  try {
    await setAppWebviewVisible(webview.label, !webview.visible);
    await refreshAppWebviews();
  } catch (error) {
    showToast(error instanceof Error ? error.message : 'Não foi possível alterar a visibilidade da WebView.');
  } finally {
    changingWebviewLabel.value = null;
  }
}

function formatLogTime(timestamp: number) {
  return new Date(timestamp * 1000).toLocaleTimeString('pt-BR', { hour: '2-digit', minute: '2-digit', second: '2-digit' });
}

async function clearLogs() {
  await clearAppLogs();
  appLogs.value = [];
}

async function exportLogs() {
  try {
    const path = await exportAppLogs();
    showToast(`Logs exportados para ${path}`, 'success');
  } catch (error) {
    showToast(error instanceof Error ? error.message : 'Não foi possível exportar os logs.');
  }
}

function setDeveloperMode(enabled: boolean) {
  developerMode.value = enabled;
  if (!enabled && screen.value === 'logs') screen.value = 'libraries';
}

function updateLogsPolling() {
  if (logsRefreshTimer) {
    window.clearInterval(logsRefreshTimer);
    logsRefreshTimer = undefined;
  }
  if (developerMode.value && screen.value === 'logs') {
    void refreshAppLogs();
    void refreshAppWebviews();
    logsRefreshTimer = window.setInterval(() => { void refreshAppLogs(); void refreshAppWebviews(); }, 1000);
  }
}

function enableSeekControl() {
  const control = document.querySelector<HTMLInputElement>('input[aria-label="Posição da música"]');
  if (control && currentTrack.value) control.disabled = false;
}

async function openPlaylist(id: string) {
  const playlist = playlists.value.find((item) => item.id === id);
  logUi(`playlist open requested: id=${id}, found=${Boolean(playlist)}, source=${playlist?.source ?? 'unknown'}, tracks=${playlist?.tracks.length ?? 0}`);
  selectedPlaylistId.value = id;
  screen.value = 'playlist';
  query.value = '';
  if (playlist?.source === 'online' && playlist.id.startsWith('VLPL') && !playlist.tracks.length) {
    await ensurePlaylistStartLoaded(playlist, false);
  }
}

async function ensurePlaylistStartLoaded(playlist: Playlist, silent: boolean) {
  if (!playlist.id.startsWith('VLPL')) {
    logUi(`playlist tracks skipped: id=${playlist.id}, reason=not-youtube-music-id`);
    return [];
  }
  if (playlist.tracks.length) {
    logUi(`playlist tracks skipped: id=${playlist.id}, reason=already-loaded, tracks=${playlist.tracks.length}`);
    return [];
  }
  if (silent && failedPrefetchPlaylists.value.has(playlist.id)) {
    logUi(`playlist prefetch skipped: id=${playlist.id}, reason=previous-failure`);
    return [];
  }
  let promise = playlistLoadPromises.get(playlist.id);
  if (!promise) {
    logUi(`playlist tracks load started: id=${playlist.id}, mode=${silent ? 'prefetch' : 'manual'}`);
    promise = googleSyncService.loadYouTubePlaylistTracks(playlist.id);
    playlistLoadPromises.set(playlist.id, promise);
  } else {
    logUi(`playlist tracks load reused: id=${playlist.id}, mode=${silent ? 'prefetch' : 'manual'}`);
  }
  if (!silent) loadingPlaylistId.value = playlist.id;
  try {
    const tracks = await promise;
    const target = playlists.value.find((item) => item.id === playlist.id);
    if (target && !target.tracks.length) target.tracks.push(...tracks);
    logUi(`playlist tracks load finished: id=${playlist.id}, received=${tracks.length}, applied=${Boolean(target && target.tracks.length)}`);
    void logPlaylistBridgeState('initial-loaded');
    return tracks;
  } catch (error) {
    logUi(`playlist tracks load failed: id=${playlist.id}, error=${error instanceof Error ? error.message : String(error)}`);
    if (silent) failedPrefetchPlaylists.value = new Set([...failedPrefetchPlaylists.value, playlist.id]);
    else showToast(error instanceof Error ? error.message : 'Não foi possível carregar as faixas da playlist.');
    return [];
  } finally {
    playlistLoadPromises.delete(playlist.id);
    if (loadingPlaylistId.value === playlist.id) loadingPlaylistId.value = null;
  }
}

async function loadMoreTracks() {
  const playlist = activePlaylist.value;
  if (!playlist || playlist.source !== 'online' || !playlist.id.startsWith('VLPL')) return;
  if (loadingPlaylistId.value || loadingMorePlaylistId.value || fullyLoadedPlaylists.value.has(playlist.id)) {
    logUi(`playlist more tracks skipped: id=${playlist.id}, initialLoading=${loadingPlaylistId.value === playlist.id}, loadingMore=${Boolean(loadingMorePlaylistId.value)}, fullyLoaded=${fullyLoadedPlaylists.value.has(playlist.id)}`);
    return;
  }
  logUi(`playlist more tracks load started: id=${playlist.id}, current=${playlist.tracks.length}`);
  loadingMorePlaylistId.value = playlist.id;
  try {
    const tracks = await googleSyncService.loadYouTubePlaylistMoreTracks(playlist.id);
    if (!tracks.length) {
      fullyLoadedPlaylists.value = new Set([...fullyLoadedPlaylists.value, playlist.id]);
      logUi(`playlist more tracks load finished: id=${playlist.id}, received=0, fullyLoaded=true`);
      return;
    }
    const existing = new Set(playlist.tracks.map((track) => track.id));
    const freshTracks = tracks.filter((track) => !existing.has(track.id));
    playlist.tracks.push(...freshTracks);
    if (!freshTracks.length) fullyLoadedPlaylists.value = new Set([...fullyLoadedPlaylists.value, playlist.id]);
    logUi(`playlist more tracks load finished: id=${playlist.id}, received=${tracks.length}, total=${playlist.tracks.length}`);
    void logPlaylistBridgeState('continuation-loaded');
    void nextTick(() => { void loadMoreTracksIfViewportHasSpace(); });
  } catch (error) {
    logUi(`playlist more tracks load failed: id=${playlist.id}, error=${error instanceof Error ? error.message : String(error)}`);
    showToast(error instanceof Error ? error.message : 'Não foi possível carregar mais faixas.');
  } finally {
    loadingMorePlaylistId.value = null;
  }
}

async function loadMoreTracksIfViewportHasSpace() {
  const playlist = activePlaylist.value;
  const list = document.querySelector<HTMLElement>('.track-list');
  if (!list || !playlist || playlist.source !== 'online' || !playlist.id.startsWith('VLPL')) return;
  if (list.scrollHeight <= list.clientHeight + 40) await loadMoreTracks();
}

function bindTrackListScroll() {
  const next = document.querySelector<HTMLElement>('.track-list');
  if (trackListElement === next) return;
  trackListElement?.removeEventListener('scroll', handleTrackScroll);
  trackListElement = next ?? undefined;
  trackListElement?.addEventListener('scroll', handleTrackScroll, { passive: true });
  if (trackListElement) void loadMoreTracksIfViewportHasSpace();
}

function handleTrackScroll(event: Event) {
  const target = event.currentTarget as HTMLElement;
  if (target.scrollTop + target.clientHeight >= target.scrollHeight - 180) void loadMoreTracks();
}

function handleCapturedScroll(event: Event) {
  const target = event.target as HTMLElement;
  if (target.classList?.contains('track-list')) handleTrackScroll(event);
}

function openPlaylistSettings(playlist: Playlist) {
  playlistSettingsId.value = playlist.id;
  playlistSettingsMode.value = playlist.mode;
  playlistSettingsOpen.value = true;
}

function savePlaylistSettings() {
  if (playlistSettingsId.value) playlistModes.value[playlistSettingsId.value] = playlistSettingsMode.value;
  playlistSettingsOpen.value = false;
}

async function syncGoogle() {
  try {
    await googleSyncService.connectYouTubeMusic();
    onlinePlaylists.value = await googleSyncService.loadYouTubePlaylists();
    googleConnected.value = true;
    showToast(`${onlinePlaylists.value.length} playlists carregadas.`, 'success');
  } catch (error) {
    try {
      await googleSyncService.connectYouTubeMusic(true);
      onlinePlaylists.value = await googleSyncService.loadYouTubePlaylists();
      googleConnected.value = true;
      showToast(`${onlinePlaylists.value.length} playlists carregadas.`, 'success');
    } catch (retryError) {
      showToast(retryError instanceof Error ? retryError.message : 'Não foi possível conectar ao YouTube Music.');
    }
  }
}

async function restoreGoogleSession() {
  try {
    googleConnected.value = await googleSyncService.restoreYouTubeSession();
    if (googleConnected.value) {
      onlinePlaylists.value = await googleSyncService.loadYouTubePlaylists();
    }
  } catch (error) {
    googleConnected.value = false;
    showToast(error instanceof Error ? error.message : 'A sessão do YouTube Music não pôde ser restaurada.');
  }
}

async function searchLibrary() {
  if (!googleConnected.value || !query.value.trim()) return;
  try {
    const tracks = await googleSyncService.searchYouTube(query.value.trim());
    searchPlaylist.value = { id: 'youtube-search', name: `Busca: ${query.value.trim()}`, source: 'online', mode: 'online', tracks, allowLocal: false };
    openPlaylist('youtube-search');
  } catch (error) {
    showToast(error instanceof Error ? error.message : String(error));
  }
}

async function play(item: LibraryItem) {
  if (currentTrack.value?.id === item.id) {
    if (usingNativeAudio.value) {
      if (!isPlaying.value) {
        await googleSyncService.setAudioPaused(false);
        isPlaying.value = true;
      }
    } else if (audio.value && audio.value.paused) {
      await audio.value.play();
      isPlaying.value = true;
    }
    return;
  }
  if (usingNativeAudio.value) await googleSyncService.stopAudio();
  currentTrack.value = item;
  queue.value = [...activePlaylist.value.tracks];
  isPlaying.value = false;
  usingNativeAudio.value = false;
  currentTime.value = 0;
  duration.value = item.durationSeconds ?? 0;
  if (!item.audioUrl && item.youtubeId) {
    try {
      const stream = await googleSyncService.playAudio(item.youtubeId);
      if (stream.durationSeconds) { duration.value = Number(stream.durationSeconds); item.durationSeconds = duration.value; item.duration = formatTime(duration.value); }
      usingNativeAudio.value = true;
      isPlaying.value = true;
    } catch (error) {
      isPlaying.value = false;
      showToast(error instanceof Error ? error.message : 'Não foi possível resolver o áudio online.');
    }
    return;
  }
  if (!audio.value || !item.audioUrl) return;
  nextTick(() => {
    if (!audio.value) return;
    audio.value.src = item.audioUrl as string;
    audio.value.volume = volume.value;
    void audio.value.play();
  });
}

async function togglePlayback() {
  if (!currentTrack.value) return;
  if (usingNativeAudio.value) {
    if (isPlaying.value) {
      try { await googleSyncService.setAudioPaused(true); isPlaying.value = false; }
      catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível pausar o áudio.'); }
    } else {
      try { await googleSyncService.setAudioPaused(false); isPlaying.value = true; }
      catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível retomar o áudio.'); }
    }
    return;
  }
  if (!audio.value) return;
  if (isPlaying.value) audio.value.pause();
  else void audio.value.play();
  isPlaying.value = !isPlaying.value;
}

function moveInQueue(direction: 1 | -1) {
  if (!queue.value.length || !currentTrack.value) return;
  const index = queue.value.findIndex((item) => item.id === currentTrack.value?.id);
  play(queue.value[(index + direction + queue.value.length) % queue.value.length]);
}

function cycleRepeatMode() {
  repeatMode.value = repeatMode.value === 'off' ? 'playlist' : repeatMode.value === 'playlist' ? 'track' : 'off';
}

function moveAfterTrackEnded() {
  if (!currentTrack.value || !queue.value.length) return;
  if (repeatMode.value === 'track') {
    void play(currentTrack.value);
    return;
  }
  const index = queue.value.findIndex((item) => item.id === currentTrack.value?.id);
  const isLast = index >= queue.value.length - 1;
  if (isLast && repeatMode.value === 'off') {
    isPlaying.value = false;
    void googleSyncService.setAudioPaused(true);
    return;
  }
  void play(queue.value[(index + 1) % queue.value.length]);
}

function toggleFavorite(item: LibraryItem) {
  const index = favoriteTracks.value.findIndex((track) => track.id === item.id);
  if (index >= 0) favoriteTracks.value.splice(index, 1);
  else favoriteTracks.value.push(item);
}

function isFavorite(item: LibraryItem) {
  return favoriteTracks.value.some((track) => track.id === item.id);
}

function addFilesToPlaylist(files: File[], playlistId = selectedPlaylistId.value) {
  const supported = files.filter((file) => file.type.startsWith('audio/'));
  const target = playlists.value.find((playlist) => playlist.id === playlistId);
  if (!target || !target.allowLocal || !supported.length) return;
  const existing = new Set(localTracks.value.map((track) => track.fileName));
  const imported = supported.filter((file) => !existing.has(file.name)).map((file, index): LibraryItem => ({
    id: `local-${file.name}-${file.lastModified}`,
    title: file.name.replace(/\.[^/.]+$/, ''),
    artist: 'Arquivo local',
    album: 'Biblioteca local',
    duration: '--:--',
    source: 'local',
    accent: ['#4ecba6', '#e6a85c', '#c1a6ff'][index % 3],
    audioUrl: URL.createObjectURL(file),
    fileName: file.name,
  }));
  if (!imported.length) return;
  localTracks.value.push(...imported);
  if (playlistId === 'favorites') favoriteTracks.value.push(...imported);
  queue.value = [...(playlists.value.find((playlist) => playlist.id === playlistId)?.tracks ?? [])];
}

function importFromInput(event: Event) {
  addFilesToPlaylist(Array.from((event.target as HTMLInputElement).files ?? []));
  (event.target as HTMLInputElement).value = '';
}

function handleDrop(event: DragEvent, playlistId = selectedPlaylistId.value) {
  dropTarget.value = null;
  addFilesToPlaylist(Array.from(event.dataTransfer?.files ?? []), playlistId);
}

function handleMetadata() {
  if (!audio.value || !currentTrack.value) return;
  duration.value = audio.value.duration;
  currentTrack.value.durationSeconds = audio.value.duration;
}

function handleTimeUpdate() { if (audio.value) currentTime.value = audio.value.currentTime; }
async function seek(event: Event) {
  if (!duration.value) return;
  const seconds = (Number((event.target as HTMLInputElement).value) / 100) * duration.value;
  if (usingNativeAudio.value) {
    currentTime.value = seconds;
    nativeSeekHoldUntil = Date.now() + 1200;
    if (nativeSeekTimer) window.clearTimeout(nativeSeekTimer);
    nativeSeekTimer = window.setTimeout(async () => {
      try { await googleSyncService.seekAudio(seconds); }
      catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível buscar a posição da música.'); }
    }, 180);
    return;
  }
  if (!audio.value) return;
  audio.value.currentTime = seconds;
  currentTime.value = audio.value.currentTime;
}
async function updateVolume() {
  isMuted.value = volume.value === 0;
  if (usingNativeAudio.value) {
    try { await googleSyncService.setAudioVolume(volume.value); }
    catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível alterar o volume.'); }
  }
  if (audio.value) audio.value.volume = volume.value;
}

async function refreshNativeAudioPosition() {
  if (!usingNativeAudio.value) return;
  if (Date.now() < nativeSeekHoldUntil) return;
  try {
    currentTime.value = await googleSyncService.getAudioPosition();
    const nativeDuration = await googleSyncService.getAudioDuration();
    if (nativeDuration > 0) duration.value = nativeDuration;
    if (duration.value > 0 && currentTime.value >= duration.value - 0.35 && !nativeTrackEnded.value) {
      nativeTrackEnded.value = true;
      moveAfterTrackEnded();
    } else if (currentTime.value < duration.value - 1) {
      nativeTrackEnded.value = false;
    }
  } catch {
    // Native position is best-effort while the stream is being replaced.
  }
}
async function toggleMute() {
  isMuted.value = !isMuted.value;
  if (usingNativeAudio.value) {
    try { await googleSyncService.setAudioVolume(isMuted.value ? 0 : volume.value); }
    catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível silenciar o áudio.'); }
  }
  if (audio.value) audio.value.muted = isMuted.value;
}
function handleEnded() { moveInQueue(1); }
function handleAudioError() { isPlaying.value = false; showToast('Não foi possível reproduzir esta faixa online. Consulte o log do VibeDeck.'); }
function formatTime(value: number) { return !Number.isFinite(value) || value <= 0 ? '--:--' : `${Math.floor(value / 60)}:${String(Math.floor(value % 60)).padStart(2, '0')}`; }
function openFilePicker() { fileInput.value?.click(); }
function openFolderPicker() { folderInput.value?.click(); }
function sourceLabel(source: LibrarySource | PlaylistMode) { return source === 'local' ? 'Local' : source === 'youtube' ? 'YouTube Music' : source === 'online' ? 'Online' : 'Misto'; }

onMounted(async () => {
  document.addEventListener('scroll', handleCapturedScroll, true);
  void refreshAppLogs();
  void refreshAppWebviews();
  audioPositionTimer = window.setInterval(refreshNativeAudioPosition, 250);
  unlistenLogs = await listenAppLogs(pushAppLog);
  void restoreGoogleSession();
  bindTrackListScroll();
  seekControlObserver = new MutationObserver(enableSeekControl);
  seekControlObserver.observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['disabled'] });
  enableSeekControl();
});
watch([usingNativeAudio, currentTrack], () => {
  void nextTick(() => {
    const control = document.querySelector<HTMLInputElement>('input[aria-label="Posição da música"]');
    if (control && currentTrack.value) control.disabled = false;
  });
});
watch([screen, selectedPlaylistId, () => activePlaylist.value?.tracks.length], () => {
  void nextTick(bindTrackListScroll);
});
watch([darkMode, developerMode], saveUserSettings);
watch([screen, developerMode], updateLogsPolling);
onBeforeUnmount(() => {
  document.removeEventListener('scroll', handleCapturedScroll, true);
  unlistenLogs?.();
  if (logsRefreshTimer) window.clearInterval(logsRefreshTimer);
  if (audioPositionTimer) window.clearInterval(audioPositionTimer);
  seekControlObserver?.disconnect();
});
</script>

<template>
  <div :class="['app-shell', { 'light-theme': !darkMode, 'loading-more-tracks': loadingMorePlaylistId === activePlaylist.id }]">
    <aside class="sidebar">
      <div class="brand"><img class="brand-logo" src="/vibedeck-logo.png" alt="" /><span>VibeDeck</span></div>
      <nav class="main-nav" aria-label="Navegacao principal"><button :class="['nav-item', { active: screen === 'home' }]" @click="screen = 'home'"><span>⌂</span> Inicio</button><button :class="['nav-item', { active: screen === 'libraries' }]" @click="screen = 'libraries'"><span>▱</span> Fontes</button></nav>
      <div class="source-section"><div class="section-label">FONTES</div><button v-for="library in libraryCards" :key="library.id" :class="['source-item', { active: screen === 'playlist' && selectedPlaylistId === library.id }]" @click="library.id === 'local' ? openPlaylist('local') : (screen = 'libraries')" @dragenter.prevent="dropTarget = library.id" @dragover.prevent="dropTarget = library.id" @dragleave.prevent="dropTarget = null" @drop.prevent="handleDrop($event, library.id)"><span class="source-dot" :style="{ background: library.accent }"></span>{{ library.name }}</button></div>
      <div class="playlist-section"><div class="section-label">PLAYLISTS</div><button v-for="playlist in playlists" :key="playlist.id" :class="['playlist-item', { active: screen === 'playlist' && selectedPlaylistId === playlist.id, 'drop-target-item': dropTarget === playlist.id }]" @click="openPlaylist(playlist.id)" @dragenter.prevent="dropTarget = playlist.id" @dragover.prevent="dropTarget = playlist.id" @dragleave.prevent="dropTarget = null" @drop.prevent="handleDrop($event, playlist.id)"><span>♫</span>{{ playlist.name }}<span class="playlist-gear" aria-label="Configurar playlist" @click.stop="openPlaylistSettings(playlist)">⚙</span></button></div>
      <div class="sidebar-footer"></div>
    </aside>

    <main class="content"><header class="topbar"><label class="search-box"><span>⌕</span><input v-model="query" :placeholder="screen === 'playlist' ? 'Buscar nesta playlist' : 'Buscar em suas bibliotecas'" @keydown.enter="searchLibrary" /></label><button v-if="developerMode" class="settings header-settings header-logs" @click="screen = 'logs'">≣ Logs</button><button class="settings header-settings" @click="settingsOpen = true">⚙ Configurações</button><button class="account-card header-account" @click="syncGoogle"><div class="avatar">P</div><div><strong>Conta Google</strong><small>{{ googleConnected ? 'Sincronização ativa' : 'Sincronizar agora' }}</small></div><span :class="['online-dot', { disconnected: !googleConnected }]" /></button></header>
      <template v-if="screen === 'libraries'">
        <section class="hero"><div><p class="eyebrow">SEU ACERVO</p><h1>Fontes</h1><p class="hero-copy">Suas fontes ficam separadas aqui. As faixas podem se encontrar nas playlists.</p></div><div class="hero-orbit"><span></span><span></span><span></span></div></section>
        <section class="library-section"><div class="section-heading"><div><p class="eyebrow">VISÃO GERAL</p><h2>Suas fontes estão no menu lateral</h2></div></div></section>
      </template>
      <template v-else-if="screen === 'logs'">
        <section class="hero logs-hero"><div><p class="eyebrow">DIAGNÓSTICO EM TEMPO REAL</p><h1>Logs do VibeDeck</h1><p class="hero-copy">Acompanhe janelas visíveis e invisíveis, brokers, sessão, playlists, streams e áudio sem sair do app.</p></div></section>
        <section class="logs-panel">
          <div class="logs-toolbar"><label class="log-search"><span>⌕</span><input v-model="logSearch" placeholder="Filtrar por mensagem, janela ou operação" /></label><select v-model="logLevelFilter"><option value="all">Todos níveis</option><option value="info">Info</option><option value="error">Erros</option></select><select v-model="logSourceFilter"><option value="all">Todas fontes</option><option v-for="source in logSources" :key="source" :value="source">{{ source }}</option></select><button @click="exportLogs">Exportar</button><button @click="clearLogs">Limpar</button></div>
          <div class="log-stats"><span><strong>{{ appLogs.length }}</strong> eventos</span><span><strong>{{ filteredLogs.length }}</strong> visíveis</span><span><strong>{{ appLogs.filter((entry) => entry.level === 'error').length }}</strong> erros</span></div>
          <div class="log-list" aria-live="polite"><article v-for="entry in filteredLogs" :key="entry.id" :class="['log-entry', entry.level]"><time>{{ formatLogTime(entry.timestamp) }}</time><span class="log-level">{{ entry.level }}</span><span class="log-source">{{ entry.source }}</span><span class="log-operation">{{ entry.operation }}</span><span class="log-window">{{ entry.window ?? 'main/native' }}</span><p>{{ entry.message }}</p></article><div v-if="!filteredLogs.length" class="empty-state"><strong>Nenhum log para estes filtros</strong><span>Execute uma ação no app para ver os eventos chegarem em tempo real.</span></div></div>
          <section class="webviews-panel"><div class="webviews-heading"><div><p class="eyebrow">WEBVIEWS</p><strong>Janelas e brokers ativos</strong></div><button @click="refreshAppWebviews">Atualizar</button></div><div class="webview-list"><article v-for="webview in appWebviews" :key="webview.label" class="webview-row"><span :class="['webview-status', { visible: webview.visible }]"></span><span><strong>{{ webview.name }}</strong><small>{{ webview.label }}</small></span><span class="webview-kind">{{ webview.internal ? 'Interna' : 'Aplicativo' }}</span><span :class="['webview-visibility', { visible: webview.visible }]">{{ webview.visible ? 'Visível' : 'Invisível' }}</span><button v-if="webview.internal" :disabled="changingWebviewLabel === webview.label" @click="toggleWebviewVisibility(webview)">{{ changingWebviewLabel === webview.label ? 'Alterando...' : webview.visible ? 'Ocultar' : 'Mostrar' }}</button></article><div v-if="!appWebviews.length" class="empty-state"><strong>Nenhuma WebView ativa</strong><span>As janelas aparecerão aqui quando forem criadas.</span></div></div></section>
        </section>
      </template>
       <template v-else-if="screen === 'playlist'">
        <section class="hero"><div><p class="eyebrow">PLAYLIST {{ sourceLabel(activePlaylist.source).toUpperCase() }}</p><h1>{{ activePlaylist.name }}</h1><p class="hero-copy">{{ isActivePlaylistLoading ? 'Carregando faixas do YouTube Music...' : activePlaylist.tracks.length ? `${activePlaylist.tracks.length} faixas nesta fila.` : 'Esta playlist ainda está vazia.' }}</p></div><div class="hero-orbit"><span></span><span></span><span></span></div></section>
        <div v-if="isActivePlaylistLoading" class="empty-state playlist-loading" role="status" aria-live="polite"><span class="loading-spinner" aria-hidden="true"></span><strong>Carregando {{ activePlaylist.name }}</strong><span>Buscando as faixas no YouTube Music...</span></div>
         <section class="library-section"><div class="section-heading"><div><p class="eyebrow">FAIXAS · {{ sourceLabel(activePlaylist.mode) }}</p><h2>{{ query ? 'Resultados da busca' : activePlaylist.name }}</h2></div><button v-if="activePlaylist.id !== 'favorites'" class="heading-gear" aria-label="Configurar playlist" @click="openPlaylistSettings(activePlaylist)">⚙</button></div><div class="track-list" @dragenter.prevent="dropTarget = activePlaylist.id" @dragover.prevent="dropTarget = activePlaylist.id" @dragleave.prevent="dropTarget = null" @drop.prevent="handleDrop($event)"><div v-for="(item, index) in filteredTracks" :key="item.id" class="track-row" role="button" tabindex="0" @click="play(item)" @keydown.enter="play(item)" @keydown.space.prevent="play(item)"><span class="track-index">{{ currentTrack?.id === item.id && isPlaying ? '▶' : String(index + 1).padStart(2, '0') }}</span><span class="cover" :style="{ background: item.accent }"><img v-if="item.coverUrl" :src="item.coverUrl" alt="" /><template v-else>{{ item.title.slice(0, 1) }}</template></span><span class="track-info"><strong>{{ item.title }}</strong><small>{{ item.artist }} · {{ item.album }}</small></span><span :class="['origin-badge', item.source]"><i></i>{{ sourceLabel(item.source) }}</span><button class="favorite-button" :aria-label="isFavorite(item) ? 'Remover dos favoritos' : 'Adicionar aos favoritos'" @click.stop="toggleFavorite(item)">{{ isFavorite(item) ? '♥' : '♡' }}</button><span class="duration">{{ item.durationSeconds ? formatTime(item.durationSeconds) : item.duration }}</span></div><div v-if="!filteredTracks.length" class="empty-state"><strong>Playlist vazia</strong><span>Use a área abaixo para adicionar músicas locais.</span></div><button v-if="activePlaylist.allowLocal" class="playlist-drop-hint" :class="{ 'drop-target': dropTarget === activePlaylist.id }" @click="openFilePicker"><span class="drop-icon">＋</span><strong>Adicione músicas a esta playlist</strong><small>Clique para escolher arquivos ou arraste músicas e pastas para esta área</small></button></div></section>
      </template>
      <section v-else class="hero home-hero"><div><p class="eyebrow">VIBEDECK</p><h1>Seu som, do seu jeito.</h1><p class="hero-copy">Abra uma playlist na lateral para começar.</p></div></section>
     <button v-if="currentTrack" class="repeat-control" :class="repeatMode" :title="repeatMode === 'off' ? 'Sem repetição' : repeatMode === 'playlist' ? 'Repetir playlist' : 'Repetir faixa'" @click="cycleRepeatMode">{{ repeatMode === 'track' ? '↻1' : '↻' }}</button></main>

    <footer class="player-bar"><div class="now-playing"><span class="mini-cover" :style="{ background: currentTrack?.accent || '#2a2f3a' }">{{ currentTrack?.title.slice(0, 1) || '♪' }}</span><div><strong>{{ currentTrack?.title || 'Nenhuma música selecionada' }}</strong><small>{{ currentTrack?.artist || 'Abra uma playlist para começar' }}</small></div><button :disabled="!currentTrack" @click="currentTrack && toggleFavorite(currentTrack)">{{ currentTrack && isFavorite(currentTrack) ? '♥' : '♡' }}</button></div><div class="player-controls"><div class="control-buttons"><button :disabled="!currentTrack" @click="moveInQueue(-1)">↶</button><button class="play-button" :disabled="!currentTrack" @click="togglePlayback">{{ isPlaying ? 'Ⅱ' : '▶' }}</button><button :disabled="!currentTrack" @click="moveInQueue(1)">↷</button></div><div class="progress"><input aria-label="Posição da música" type="range" min="0" max="100" :value="progressPercent" :disabled="!currentTrack || usingNativeAudio" :title="usingNativeAudio ? 'Seek nativo ainda não está disponível' : undefined" @input="seek" /></div><div class="time-label"><span>{{ formatTime(currentTime) }}</span><span>{{ formatTime(duration) }}</span></div></div><div class="player-tools"><span class="player-origin"><i :class="currentTrack?.source || 'local'"></i>{{ currentTrack ? sourceLabel(currentTrack.source) : 'Local' }}</span><span>⌕</span><span>▣</span><button class="mute-button" :disabled="!currentTrack" :aria-label="isMuted ? 'Ativar som' : 'Silenciar'" @click="toggleMute">{{ isMuted ? '🔇' : '🔊' }}</button><input v-model.number="volume" aria-label="Volume" class="volume-control" type="range" min="0" max="1" step="0.01" @input="updateVolume" /></div></footer>
    <div class="toast-stack" aria-live="polite"><div v-for="toast in toasts" :key="toast.id" :class="['toast', toast.tone]">{{ toast.message }}</div></div>
    <audio ref="audio" @loadedmetadata="handleMetadata" @ended="handleEnded" @timeupdate="handleTimeUpdate" @error="handleAudioError" />
    <div v-if="settingsOpen" class="settings-overlay" @click.self="settingsOpen = false"><section class="settings-panel"><button class="close-settings" @click="settingsOpen = false">×</button><p class="eyebrow">PREFERÊNCIAS</p><h2>Configurações</h2><label class="setting-row"><span><strong>Tema</strong><small>Escolha a aparência do VibeDeck.</small></span><span class="theme-switch"><button :class="{ selected: darkMode }" @click="darkMode = true">Escuro</button><button :class="{ selected: !darkMode }" @click="darkMode = false">Claro</button></span></label><label class="setting-row"><span><strong>Modo desenvolvedor</strong><small>Exibe ferramentas internas, como o painel de logs em tempo real.</small></span><span class="switch-control"><input type="checkbox" :checked="developerMode" @change="setDeveloperMode(($event.target as HTMLInputElement).checked)" /><i></i></span></label></section></div><div v-if="playlistSettingsOpen" class="settings-overlay" @click.self="playlistSettingsOpen = false"><section class="settings-panel"><button class="close-settings" @click="playlistSettingsOpen = false">×</button><p class="eyebrow">PLAYLIST</p><h2>Configurar playlist</h2><p class="settings-description">Defina quais tipos de música podem entrar nesta playlist.</p><div class="playlist-mode-options"><label v-for="mode in (['local', 'online', 'mixed'] as PlaylistMode[])" :key="mode" :class="['mode-option', { selected: playlistSettingsMode === mode }]" @click="playlistSettingsMode = mode"><input v-model="playlistSettingsMode" type="radio" :value="mode" /><span><strong>{{ mode === 'local' ? 'Somente local' : mode === 'online' ? 'Somente online' : 'Online e local' }}</strong><small>{{ mode === 'local' ? 'Aceita arquivos deste computador.' : mode === 'online' ? 'Aceita apenas faixas sincronizadas.' : 'Mistura as duas bibliotecas.' }}</small></span></label></div><button class="save-settings" @click="savePlaylistSettings">Salvar configuração</button></section></div>
  </div>
</template>
