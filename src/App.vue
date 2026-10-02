<script setup lang="ts">
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { type LibraryItem, type LibrarySource, type Playlist, type PlaylistMode } from './models/library';
import type { LocalImportFileResult, LocalImportProgress, LocalLibraryCatalog, PlaylistCatalogSnapshot } from './interfaces/library';
import { isCurrentPlayerEndEvent, type NativeSeekIntent, type NativeSeekRequest, type PlayerConnection, type PlayerQueuePanelEvents, type PlayerQueueTrack, type PlayerSnapshot, type VolumeIntent } from './interfaces/player';
import { youtubeMusicSource } from './services/music-source';
import { googleSyncService } from './services/google-sync';
import { clearAppLogs, exportAppLogs, getAppLogs, getAppWebviews, getYouTubePlaylistDebugState, listenAppLogs, setAppWebviewVisible, writeAppLog, type AppLogEntry, type AppWebviewInfo } from './services/app-logs';
import { Heart, Pause, Play, Repeat, SkipBack, SkipForward, Volume2, VolumeX } from '@lucide/vue';
import VisualizerCanvas, { type VisualizerKind } from './visualizers/VisualizerCanvas.vue';
import VisualizerControls from './visualizers/VisualizerControls.vue';
import VisualizerPicker from './visualizers/VisualizerPicker.vue';
import PlaylistShuffleButton from './components/PlaylistShuffleButton.vue';
import QueuePanel from './components/QueuePanel.vue';
import PlaylistLoadingOverlay from './components/PlaylistLoadingOverlay.vue';
import AccountMenu from './components/AccountMenu.vue';
import MusicPickerMenu from './components/MusicPickerMenu.vue';
import VisualizerTrackArtwork from './visualizers/VisualizerTrackArtwork.vue';
import { DEFAULT_SERPENT_RESOLUTION } from './visualizers/visualizer-performance';
import { isCurrentPlaylistRequest, loadAllPages, PlaylistOperationRegistry, shouldRevealPlaylistTracks, type PaginationPage } from './services/playlist-loader';
import { createQueueContext, createQueueMoveIntent, createSelectedTrackQueue, isCurrentPlayerRequest, nextQueueIndex, nextRepeatMode, queueForRepeatTransition, queueIndexForTrack, sameTrackByTitleAndArtist, shuffleQueueContext, type PlayerRequest, type RepeatMode } from './services/player-state';
import { terminalBehaviorForRepeatMode } from './interfaces/player';
import { reconcilePlayerSnapshot, seekSnapshotMatchesIntent, volumeSnapshotMatchesIntent } from './services/player-sync';
import { extractNativePaths, isTauriRuntime, localLibraryService, localPlaybackErrorMessage, localPlaylistToPlaylist, requireNativePaths, selectedDialogPaths, summarizeLocalImport } from './services/local-library';
import { normalizeBridgeError } from './interfaces/bridge';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
const Serpents3D = defineAsyncComponent(() => import('./visualizers/Serpents3D.vue'));

type Screen = 'home' | 'libraries' | 'playlist' | 'logs';
type UserSettings = { darkMode: boolean; developerMode: boolean; visualizer: VisualizerKind; serpentDensity: number; serpentDetail: number; serpentGlow: number; serpentResolution: number; volume: number; isMuted: boolean; visualizerSensitivity: number; visualizerSmoothing: number; visualizerIntensity: number; visualizerMotion: number; mascotBassSensitivity: number; mascotBassDistance: number; bassMinHz: number; bassMaxHz: number; energyMinHz: number; energyMaxHz: number; voiceMinHz: number; voiceMaxHz: number; motionMinHz: number; motionMaxHz: number };

const userSettingsKey = 'vibedeck:user-settings';
const savedSettings = loadUserSettings();

const screen = ref<Screen>('libraries');
const query = ref('');
const selectedPlaylistId = ref('local');
const localTracks = ref<LibraryItem[]>([]);
const localCatalog = ref<LocalLibraryCatalog>({ version: 1, playlists: [] });
const localLibraryLoading = ref(false);
const localLibraryError = ref<string | null>(null);
const localStaleTrackCount = ref(0);
const localImport = ref<{ operationId: string; processed: number; total: number; currentPath: string; files: LocalImportFileResult[]; cancelled: boolean } | null>(null);
const favoriteTracks = ref<LibraryItem[]>([]);
const onlinePlaylists = ref<Playlist[]>([]);
const searchPlaylist = ref<Playlist | null>(null);
const currentTrack = ref<LibraryItem | null>(null);
const queue = ref<LibraryItem[]>([]);
const queuePlaylistId = ref<string | null>(null);
const queueSource = ref<Playlist['source']>('local');
const queueGeneration = ref(0);
const queueIndex = ref(-1);
const queueShuffled = ref(false);
const queueActivationPending = ref(false);
const queuePanelOpen = ref(false);
const isPlaying = ref(false);
const repeatMode = ref<RepeatMode>('off');
const nativeTrackEnded = ref(false);
const googleConnected = ref(youtubeMusicSource.isConnected());
const darkMode = ref(savedSettings.darkMode);
const developerMode = ref(savedSettings.developerMode);
const selectedVisualizer = ref<VisualizerKind>(savedSettings.visualizer);
const serpentDensity = ref(savedSettings.serpentDensity);
const serpentDetail = ref(savedSettings.serpentDetail);
const serpentGlow = ref(savedSettings.serpentGlow);
const serpentResolution = ref(savedSettings.serpentResolution);
const settingsOpen = ref(false);
const settingsSection = ref<'general' | 'visualizer' | 'developer'>('general');
const visualizerOpen = ref(false);
const spectrogramOpen = ref(false);
const normalVisualizer = ref<Exclude<VisualizerKind, 'spectrogram'>>(selectedVisualizer.value === 'spectrogram' ? 'mascot' : selectedVisualizer.value);
const audio = ref<HTMLAudioElement>();
const fileInput = ref<HTMLInputElement>();
const folderInput = ref<HTMLInputElement>();
const currentTime = ref(0);
const duration = ref(0);
const visualizerLevels = ref<number[]>([]);
const volume = ref(savedSettings.volume);
const isMuted = ref(savedSettings.isMuted);
const visualizerSensitivity = ref(savedSettings.visualizerSensitivity);
const visualizerSmoothing = ref(savedSettings.visualizerSmoothing);
const visualizerIntensity = ref(savedSettings.visualizerIntensity);
const visualizerMotion = ref(savedSettings.visualizerMotion);
const mascotBassSensitivity = ref(savedSettings.mascotBassSensitivity);
const mascotBassDistance = ref(savedSettings.mascotBassDistance);
const bassMinHz = ref(savedSettings.bassMinHz);
const bassMaxHz = ref(savedSettings.bassMaxHz);
const energyMinHz = ref(savedSettings.energyMinHz);
const energyMaxHz = ref(savedSettings.energyMaxHz);
const voiceMinHz = ref(savedSettings.voiceMinHz);
const voiceMaxHz = ref(savedSettings.voiceMaxHz);
const motionMinHz = ref(savedSettings.motionMinHz);
const motionMaxHz = ref(savedSettings.motionMaxHz);
const usingNativeAudio = ref(false);
const dropTarget = ref<string | null>(null);
const playlistModes = ref<Record<string, PlaylistMode>>({ local: 'local', favorites: 'mixed' });
const playlistSettingsOpen = ref(false);
const playlistSettingsId = ref<string | null>(null);
const playlistSettingsMode = ref<PlaylistMode>('local');
const toasts = ref<{ id: number; message: string; tone: 'success' | 'error' }[]>([]);
const loadingPlaylistId = ref<string | null>(null);
const loadingPlaylistGeneration = ref<number | null>(null);
const fullyLoadedPlaylists = ref(new Set<string>());
const playlistPaginationOperations = new PlaylistOperationRegistry<void>();
const appLogs = ref<AppLogEntry[]>([]);
const logSearch = ref('');
const logLevelFilter = ref<'all' | 'info' | 'error'>('all');
const logSourceFilter = ref('all');
const appWebviews = ref<AppWebviewInfo[]>([]);
const changingWebviewLabel = ref<string | null>(null);
const VISUALIZER_POLL_INTERVAL_MS = 33;
let unlistenLogs: (() => void) | undefined;
let logsRefreshTimer: number | undefined;
let audioPositionTimer: number | undefined;
let audioLevelTimer: number | undefined;
let visualizerRequestPending = false;
let visualizerRequestGeneration = 0;
let nativeSeekTimer: number | undefined;
let nativeSeekSequence = 0;
let nativeSeekHoldUntil = 0;
let nativeVolumeSequence = 0;
const nativeVolumePending = ref<VolumeIntent | null>(null);
let nativeVolumeTimer: number | undefined;
let seekControlObserver: MutationObserver | undefined;
let trackListElement: HTMLElement | undefined;
let playlistOpenGeneration = 0;
let playbackGeneration = 0;
let playbackActivationSequence = 0;
let activePlaybackQueueIndex = -1;
let activePlaybackAbort: AbortController | undefined;
let activePlayerRequest: PlayerRequest | null = null;
let localLibraryGeneration = 0;
let localImportGeneration = 0;
let unlistenLocalImport: (() => void) | undefined;
let unlistenNativeDrop: UnlistenFn | undefined;
let unlistenPlayerState: UnlistenFn | undefined;
let playerReconnectTimer: number | undefined;
let playerHydrationPromise: Promise<void> | undefined;
let nativeRepeatSync: Promise<void> = Promise.resolve();
const nativePlayerRevision = ref(-1);
const nativePlayerSnapshot = ref<PlayerSnapshot | null>(null);
const nativePlayerConnection = ref<PlayerConnection>('connecting');
const nativeSeekPending = ref<NativeSeekIntent | null>(null);
const transientObjectUrls = new Set<string>();

function reconcilePlaylistCatalog(snapshot: PlaylistCatalogSnapshot) {
  const cached = new Map(snapshot.playlists.map((playlist) => [playlist.id, playlist]));
  onlinePlaylists.value = onlinePlaylists.value.map((playlist) => {
    const saved = cached.get(playlist.id);
    return saved ? { ...playlist, tracks: saved.tracks, allowLocal: playlist.allowLocal, mode: playlist.mode, continuation: saved.continuation, terminal: saved.terminal } : playlist;
  });
}

async function hydratePlaylistCatalog() {
  if (!isTauriRuntime()) return;
  try {
    const snapshot = await googleSyncService.loadPlaylistCatalog();
    playlistOpenGeneration = Math.max(playlistOpenGeneration, ...snapshot.playlists.map((playlist) => playlist.generation));
    logUi(`player queue catalog hydrated: playlists=${snapshot.playlists.length}, tracks=${snapshot.playlists.reduce((total, playlist) => total + playlist.tracks.length, 0)}, states=${snapshot.playlists.map((playlist) => `${playlist.id}:${playlist.tracks.length}:${playlist.terminal ? 'terminal' : playlist.continuation ? 'continuation' : 'incomplete'}`).join(',')}`);
     if (!onlinePlaylists.value.length) onlinePlaylists.value = snapshot.playlists.map(({ id, name, source, mode, tracks, allowLocal, continuation, terminal }) => ({ id, name, source: source as Playlist['source'], mode: mode as PlaylistMode, tracks, allowLocal, continuation, terminal }));
    else reconcilePlaylistCatalog(snapshot);
  } catch (error) {
    logUi(`playlist catalog hydration failed: ${error instanceof Error ? error.message : String(error)}`);
  }
}

const playlists = computed<Playlist[]>(() => [
  ...(searchPlaylist.value ? [searchPlaylist.value] : []),
  ...onlinePlaylists.value,
  ...localCatalog.value.playlists.map(localPlaylistToPlaylist),
  { id: 'local', name: 'Biblioteca local', source: 'local', mode: playlistModes.value.local, tracks: localTracks.value, allowLocal: playlistModes.value.local !== 'online' },
  { id: 'favorites', name: 'Favoritos', source: playlistModes.value.favorites, mode: playlistModes.value.favorites, tracks: favoriteTracks.value, allowLocal: playlistModes.value.favorites !== 'online' },
]);
const activePlaylist = computed(() => playlists.value.find((playlist) => playlist.id === selectedPlaylistId.value) ?? playlists.value[0]);
const queuePanelTracks = computed<PlayerQueueTrack[]>(() => queue.value.map((item) => ({
  id: item.id,
  title: item.title,
  artist: item.artist,
  cover: item.coverUrl ?? '',
  accent: item.accent,
  source: sourceLabel(item.source),
})));
const filteredTracks = computed(() => {
  const normalizedQuery = query.value.trim().toLowerCase();
  return activePlaylist.value.tracks.filter((item) => !normalizedQuery || `${item.title} ${item.artist} ${item.album}`.toLowerCase().includes(normalizedQuery));
});
const isActivePlaylistLoading = computed(() => loadingPlaylistId.value === activePlaylist.value.id);
const canRevealActivePlaylistTracks = computed(() => shouldRevealPlaylistTracks(isActivePlaylistLoading.value ? 'loading' : 'complete'));
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

function reportBridgeFailure(error: unknown, operation: string) {
  const normalized = normalizeBridgeError(error, operation);
  void writeAppLog({
    message: normalized.message,
    operation: normalized.operation,
    correlationId: normalized.correlationId,
    level: 'error',
    code: normalized.code,
    details: normalized.details,
  }).catch(() => undefined);
  return normalized;
}

function loadUserSettings(): UserSettings {
  try {
    const value = localStorage.getItem(userSettingsKey);
    if (!value) return defaultUserSettings();
    const settings = { ...defaultUserSettings(), ...JSON.parse(value) } as UserSettings;
    settings.serpentDensity = Math.min(1.5, Math.max(0.5, Number(settings.serpentDensity) || 1));
    settings.serpentDetail = Math.min(1.5, Math.max(0.5, Number(settings.serpentDetail) || 1));
    settings.serpentGlow = Math.min(2, Math.max(0, Number(settings.serpentGlow) || 1));
    settings.serpentResolution = Math.min(1.5, Math.max(0.75, Number(settings.serpentResolution) || DEFAULT_SERPENT_RESOLUTION));
    return settings;
  } catch {
    return defaultUserSettings();
  }
}

function defaultUserSettings(): UserSettings {
  return { darkMode: true, developerMode: false, visualizer: 'mascot', serpentDensity: 1, serpentDetail: 1, serpentGlow: 1, serpentResolution: DEFAULT_SERPENT_RESOLUTION, volume: 0.8, isMuted: false, visualizerSensitivity: 1, visualizerSmoothing: 0.5, visualizerIntensity: 1, visualizerMotion: 1, mascotBassSensitivity: 1, mascotBassDistance: 1, bassMinHz: 20, bassMaxHz: 250, energyMinHz: 20, energyMaxHz: 16000, voiceMinHz: 180, voiceMaxHz: 3500, motionMinHz: 20, motionMaxHz: 250 };
}

function saveUserSettings() {
  localStorage.setItem(userSettingsKey, JSON.stringify({ darkMode: darkMode.value, developerMode: developerMode.value, visualizer: selectedVisualizer.value, serpentDensity: serpentDensity.value, serpentDetail: serpentDetail.value, serpentGlow: serpentGlow.value, serpentResolution: serpentResolution.value, volume: volume.value, isMuted: isMuted.value, visualizerSensitivity: visualizerSensitivity.value, visualizerSmoothing: visualizerSmoothing.value, visualizerIntensity: visualizerIntensity.value, visualizerMotion: visualizerMotion.value, mascotBassSensitivity: mascotBassSensitivity.value, mascotBassDistance: mascotBassDistance.value, bassMinHz: bassMinHz.value, bassMaxHz: bassMaxHz.value, energyMinHz: energyMinHz.value, energyMaxHz: energyMaxHz.value, voiceMinHz: voiceMinHz.value, voiceMaxHz: voiceMaxHz.value, motionMinHz: motionMinHz.value, motionMaxHz: motionMaxHz.value }));
}

function applyLocalCatalog(catalog: LocalLibraryCatalog) {
  localCatalog.value = catalog;
  localTracks.value = catalog.playlists.flatMap((playlist) => playlist.tracks.map((track) => ({ ...localPlaylistToPlaylist({ ...playlist, tracks: [track] }).tracks[0] })));
  if (selectedPlaylistId.value !== 'local'
    && !playlists.value.some((playlist) => playlist.id === selectedPlaylistId.value)
    && !(usingNativeAudio.value && queuePlaylistId.value)) selectedPlaylistId.value = 'local';
}

async function loadLocalLibrary() {
  if (!isTauriRuntime()) return;
  const generation = ++localLibraryGeneration;
  localLibraryLoading.value = true;
  localLibraryError.value = null;
  try {
    const result = await localLibraryService.load();
    if (generation !== localLibraryGeneration) return;
    localStaleTrackCount.value = result.staleTrackCount;
    applyLocalCatalog(result.catalog);
     } catch (error) {
    if (generation !== localLibraryGeneration) return;
    localLibraryError.value = error instanceof Error ? error.message : 'Não foi possível carregar a biblioteca local.';
  } finally {
    if (generation === localLibraryGeneration) localLibraryLoading.value = false;
  }
}

function localPlaylistTarget(playlistId: string) {
  const playlist = playlists.value.find((item) => item.id === playlistId);
  if (!playlist || !playlist.allowLocal || playlist.source === 'online' || playlist.mode === 'online') return null;
  return playlist;
}

function summarizeImport(files: LocalImportFileResult[]) {
  return summarizeLocalImport(files);
}

async function importNativePaths(paths: string[], playlistId = selectedPlaylistId.value) {
  const target = localPlaylistTarget(playlistId);
  if (!target) {
    showToast('Arquivos locais só podem ser importados em playlists locais ou mistas.');
    return;
  }
  if (!paths.length) return;
  const operationId = `local-import-${Date.now()}-${Math.random().toString(36).slice(2)}`;
  const generation = ++localLibraryGeneration;
  localImportGeneration = generation;
  localImport.value = { operationId, processed: 0, total: paths.length, currentPath: '', files: [], cancelled: false };
  const onProgress = (progress: LocalImportProgress) => {
    if (progress.operationId !== operationId || generation !== localLibraryGeneration) return;
    localImport.value = { ...(localImport.value as NonNullable<typeof localImport.value>), ...progress };
  };
  try {
    const result = await localLibraryService.importPaths({ paths, playlistName: target.name, operationId });
    if (generation !== localLibraryGeneration || result.operationId !== operationId) return;
    applyLocalCatalog(result.catalog);
    localImport.value = { ...(localImport.value as NonNullable<typeof localImport.value>), files: result.files, cancelled: result.cancelled, processed: result.files.length, total: result.files.length, currentPath: '' };
    const summary = summarizeImport(result.files);
    showToast(`${summary.imported} importada(s), ${summary.duplicates} duplicata(s), ${summary.unsupported} não suportada(s), ${summary.failed} falha(s).`, summary.failed || summary.unsupported ? 'error' : 'success');
  } catch (error) {
    if (generation === localLibraryGeneration) showToast(error instanceof Error ? error.message : 'Não foi possível importar os arquivos locais.');
  } finally {
    if (generation === localLibraryGeneration) window.setTimeout(() => { if (localImport.value?.operationId === operationId) localImport.value = null; }, 5000);
  }
}

async function cancelLocalImport() {
  const operation = localImport.value;
  if (!operation) return;
  ++localLibraryGeneration;
  localImport.value = { ...operation, cancelled: true };
  try { await localLibraryService.cancel(operation.operationId); } catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível cancelar a importação.'); }
}

function applyNativePlayerSnapshot(snapshot: PlayerSnapshot) {
  const next = reconcilePlayerSnapshot({ snapshot: nativePlayerSnapshot.value, revision: nativePlayerRevision.value, connection: nativePlayerConnection.value }, snapshot, queueGeneration.value, queueActivationPending.value, playbackActivationSequence);
  if (next.snapshot === nativePlayerSnapshot.value && next.revision === nativePlayerRevision.value && next.connection === nativePlayerConnection.value) {
    return false;
  }
  const pendingSeek = nativeSeekPending.value;
  const sameSeekTrack = pendingSeek
    && snapshot.track?.id === pendingSeek.trackId
    && snapshot.queueGeneration === pendingSeek.queueGeneration;
  if (pendingSeek && !sameSeekTrack && snapshot.revision > pendingSeek.baseRevision) nativeSeekPending.value = null;
  const previousConnection = nativePlayerConnection.value;
  if (next.revision === nativePlayerRevision.value && next.connection === previousConnection) return false;
  nativePlayerRevision.value = next.revision;
  nativePlayerSnapshot.value = next.snapshot;
  nativePlayerConnection.value = next.connection;
  logUi(`player queue snapshot: playlist=${snapshot.playlistId ?? 'none'}, source=${snapshot.queueSource ?? snapshot.source ?? 'none'}, queue=${snapshot.queue.length}, index=${snapshot.queueIndex}, repeat=${snapshot.repeatMode}, terminal=${snapshot.terminalBehavior}, generation=${snapshot.queueGeneration}, revision=${snapshot.revision}, track=${snapshot.track?.id ?? 'none'}`);
  if (!snapshot.track) {
    nativeSeekPending.value = null;
    currentTrack.value = null;
    isPlaying.value = false;
    usingNativeAudio.value = false;
    currentTime.value = 0;
    duration.value = 0;
    if (!snapshot.queue.length) {
      queuePlaylistId.value = null;
      queue.value = [];
      queueIndex.value = -1;
    }
    usingNativeAudio.value = false;
    isPlaying.value = false;
    return true;
  }
  currentTrack.value = snapshot.track;
  queuePlaylistId.value = snapshot.playlistId;
  queueSource.value = snapshot.queueSource || snapshot.source || 'local';
  queueGeneration.value = snapshot.queueGeneration;
  queueIndex.value = snapshot.queueIndex;
  activePlaybackQueueIndex = snapshot.queueIndex;
  playbackActivationSequence = Math.max(playbackActivationSequence, snapshot.activationSequence ?? 0);
  queueShuffled.value = snapshot.shuffled;
  repeatMode.value = snapshot.repeatMode ?? repeatMode.value;
  queue.value = [...snapshot.queue];
  if (snapshot.queueGeneration === queueGeneration.value) queueActivationPending.value = false;
  usingNativeAudio.value = snapshot.nativeMode;
  currentTime.value = snapshot.position;
  if (pendingSeek && sameSeekTrack && seekSnapshotMatchesIntent(snapshot, pendingSeek)) {
    if (Math.abs(snapshot.position - pendingSeek.seconds) <= 0.35) {
      nativeSeekPending.value = null;
      nativeSeekHoldUntil = 0;
    } else {
      // Keep the newest slider intent visible while an older native response is caught up.
      currentTime.value = pendingSeek.seconds;
      nativeSeekHoldUntil = Date.now() + 1200;
      scheduleNativeSeekFlush();
    }
  }
  duration.value = snapshot.duration || snapshot.track.durationSeconds || 0;
  const pendingVolume = nativeVolumePending.value;
  if ((!pendingVolume || volumeSnapshotMatchesIntent(snapshot, pendingVolume)) && snapshot.volumeSequence >= nativeVolumeSequence) {
    nativeVolumeSequence = snapshot.volumeSequence;
    if (!snapshot.muted) volume.value = snapshot.volume;
    isMuted.value = snapshot.muted;
    if (pendingVolume && snapshot.volumeSequence >= pendingVolume.sequence) nativeVolumePending.value = null;
  }
  isPlaying.value = snapshot.isPlaying;
  return true;
}

function schedulePlayerRehydrate() {
  if (playerReconnectTimer !== undefined) return;
  playerReconnectTimer = window.setTimeout(() => {
    playerReconnectTimer = undefined;
    void rehydrateNativePlayer();
  }, 2000);
}

async function rehydrateNativePlayer() {
  if (!isTauriRuntime()) return;
  if (playerHydrationPromise) return playerHydrationPromise;
  playerHydrationPromise = (async () => {
    nativePlayerConnection.value = 'connecting';
    try {
      const snapshot = await googleSyncService.getPlayerSnapshot();
      applyNativePlayerSnapshot(snapshot);
    } catch (error) {
      nativePlayerConnection.value = 'disconnected';
      schedulePlayerRehydrate();
      logUi(`player rehydrate failed: ${error instanceof Error ? error.message : 'bridge unavailable'}`);
    } finally {
      playerHydrationPromise = undefined;
    }
  })();
  return playerHydrationPromise;
}

async function restoreNativePlayerReference() {
  await rehydrateNativePlayer();
}

function setQueueContext(playlist: Playlist | undefined) {
  queueGeneration.value += 1;
  queueActivationPending.value = true;
  queuePlaylistId.value = playlist?.id ?? null;
  queueSource.value = playlist?.source ?? 'local';
  queueShuffled.value = false;
  const currentId = currentTrack.value?.id;
  const context = playlist
    ? createQueueContext(playlist.id, playlist.tracks, queueGeneration.value, currentId ? playlist.tracks.findIndex((track) => track.id === currentId) : -1, playlist.source)
    : undefined;
  queue.value = context?.tracks ?? [];
  queueIndex.value = context?.index ?? -1;
  if (queueIndex.value < 0 && currentTrack.value && playlist) {
    // A queue replacement is only finalized by the explicit play activation.
  }
}

function queueOwnsTrack(item: LibraryItem, playlist: Playlist | undefined) {
  return Boolean(playlist
    && queuePlaylistId.value === playlist.id
    && queue.value.some((track) => track.id === item.id)
    && playlist.tracks.some((track) => track.id === item.id));
}

function logUi(message: string) {
  void writeAppLog({ message, operation: 'ui', correlationId: `ui-${Date.now()}` });
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

function openLogs() {
  visualizerOpen.value = false;
  screen.value = 'logs';
}

function setVisualizer(kind: VisualizerKind) {
  if (kind !== 'spectrogram') normalVisualizer.value = kind;
  selectedVisualizer.value = kind;
  spectrogramOpen.value = false;
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
  const generation = ++playlistOpenGeneration;
  const playlist = playlists.value.find((item) => item.id === id);
  logUi(`playlist open requested: id=${id}, found=${Boolean(playlist)}, source=${playlist?.source ?? 'unknown'}, tracks=${playlist?.tracks.length ?? 0}`);
  selectedPlaylistId.value = id;
  screen.value = 'playlist';
  query.value = '';
  if (playlist?.source === 'online' && playlist.id.startsWith('VLPL') && !fullyLoadedPlaylists.value.has(playlist.id)) {
    await loadAllPlaylistTracks(playlist.id, undefined, generation);
  }
}

async function loadAllPlaylistTracks(playlistId: string, initialPage?: PaginationPage<LibraryItem>, generation?: number) {
  const playlist = playlists.value.find((item) => item.id === playlistId);
  const requestGeneration = generation ?? playlistOpenGeneration;
  if (!playlist || fullyLoadedPlaylists.value.has(playlistId)) return;
  await playlistPaginationOperations.run(playlistId, requestGeneration, async () => {
    loadingPlaylistId.value = playlistId;
    loadingPlaylistGeneration.value = requestGeneration;
    try {
      let firstPage = initialPage;
      if (!firstPage && !playlist.tracks.length) {
        logUi(`playlist tracks load started: id=${playlistId}, generation=${requestGeneration}, mode=initial`);
        firstPage = await googleSyncService.loadYouTubePlaylistTracks(playlistId);
        if (!isCurrentPlaylistRequest(requestGeneration, playlistOpenGeneration, playlistId, activePlaylist.value.id)) return;
        playlist.continuation = firstPage.continuation;
        playlist.terminal = firstPage.hasMore === false && Number.isFinite(firstPage.expectedCount) ? firstPage.items.length >= Number(firstPage.expectedCount) : firstPage.hasMore === false;
        playlist.tracks.splice(0, playlist.tracks.length, ...firstPage.items);
        if (isTauriRuntime()) await googleSyncService.saveYouTubePlaylistPage({ playlistId, name: playlist.name, source: playlist.source, mode: playlist.mode, allowLocal: Boolean(playlist.allowLocal), generation: requestGeneration || 1, tracks: firstPage.items, continuation: firstPage.continuation, hasMore: firstPage.hasMore });
        logUi(`playlist tracks load finished: id=${playlistId}, received=${firstPage.items.length}, hasMore=${firstPage.hasMore}, applied=${playlist.tracks.length > 0}`);
      }
      firstPage ??= { items: playlist.tracks, hasMore: playlist.terminal !== true, continuation: playlist.continuation };
      const allTracks = await loadAllPages(firstPage, () => googleSyncService.loadYouTubePlaylistMoreTracks(playlistId), (track) => track.id, {
      onPage: async (items, page) => {
        if (!isCurrentPlaylistRequest(requestGeneration, playlistOpenGeneration, playlistId, activePlaylist.value.id)) return;
        playlist.tracks.push(...items);
        playlist.continuation = page.continuation;
        playlist.terminal = !page.hasMore;
        if (isTauriRuntime()) await googleSyncService.saveYouTubePlaylistPage({ playlistId, name: playlist.name, source: playlist.source, mode: playlist.mode, allowLocal: Boolean(playlist.allowLocal), generation: requestGeneration || 1, tracks: items, continuation: page.continuation, hasMore: page.hasMore });
        logUi(`playlist continuation page merged: id=${playlistId}, received=${items.length}, total=${playlist.tracks.length}, hasMore=${page.hasMore}`);
      },
      onStop: async (stop) => {
        const verifiedTerminal = stop.reason === 'provider-terminal';
        playlist.terminal = verifiedTerminal;
        if (verifiedTerminal) playlist.continuation = null;
        if (isTauriRuntime() && verifiedTerminal) await googleSyncService.saveYouTubePlaylistPage({ playlistId, name: playlist.name, source: playlist.source, mode: playlist.mode, allowLocal: Boolean(playlist.allowLocal), generation: requestGeneration || 1, tracks: [], hasMore: false });
        logUi(`playlist pagination terminal: id=${playlistId}, reason=${stop.reason}, pages=${stop.pagesLoaded}`);
        },
      });
      if (!isCurrentPlaylistRequest(requestGeneration, playlistOpenGeneration, playlistId, activePlaylist.value.id)) return;
      playlist.tracks.splice(0, playlist.tracks.length, ...allTracks);
      if (playlist.terminal) {
        fullyLoadedPlaylists.value = new Set([...fullyLoadedPlaylists.value, playlistId]);
        logUi(`playlist full load: id=${playlistId}, total=${playlist.tracks.length}, expected=${firstPage.expectedCount ?? 'unknown'}`);
      } else {
        logUi(`playlist incomplete: id=${playlistId}, total=${playlist.tracks.length}, cursor=${playlist.continuation ? 'available' : 'missing'}`);
        showToast(`A playlist carregou ${playlist.tracks.length} faixas, mas a paginação não confirmou o fim. Tente novamente.`);
      }
    } catch (error) {
      if (isCurrentPlaylistRequest(requestGeneration, playlistOpenGeneration, playlistId, activePlaylist.value.id)) {
        showToast(error instanceof Error ? error.message : 'Não foi possível carregar todas as faixas da playlist.');
      }
    } finally {
      if (loadingPlaylistId.value === playlistId && loadingPlaylistGeneration.value === requestGeneration) {
        loadingPlaylistId.value = null;
        loadingPlaylistGeneration.value = null;
      }
    }
  });
}

async function loadMoreTracks() {
  const playlist = activePlaylist.value;
  const generation = playlistOpenGeneration;
  if (!playlist || playlist.source !== 'online' || !playlist.id.startsWith('VLPL')) return;
  if (fullyLoadedPlaylists.value.has(playlist.id)) {
    logUi(`playlist more tracks skipped: id=${playlist.id}, initialLoading=${loadingPlaylistId.value === playlist.id}, fullyLoaded=${fullyLoadedPlaylists.value.has(playlist.id)}`);
    return;
  }
  const pendingPagination = playlistPaginationOperations.join(playlist.id, generation);
  if (pendingPagination) {
    logUi(`playlist more tracks queued: id=${playlist.id}, initialLoading=${loadingPlaylistId.value === playlist.id}`);
    await pendingPagination;
    return;
  }
  if (loadingPlaylistId.value) {
    logUi(`playlist more tracks skipped: id=${playlist.id}, initialLoading=${loadingPlaylistId.value === playlist.id}, fullyLoaded=${fullyLoadedPlaylists.value.has(playlist.id)}`);
    return;
  }
  logUi(`playlist more tracks starting pagination owner: id=${playlist.id}, current=${playlist.tracks.length}`);
  await loadAllPlaylistTracks(playlist.id, undefined, generation);
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

function shuffleActivePlaylist() {
  const playlist = activePlaylist.value;
  if (!playlist || playlist.tracks.length < 2) return;
  setQueueContext(playlist);
  const context = shuffleQueueContext({ playlistId: playlist.id, source: playlist.source, generation: queueGeneration.value, index: queueIndex.value, tracks: [...queue.value], shuffled: false });
  queue.value = context.tracks;
  queueIndex.value = context.index;
  queueShuffled.value = true;
  void play(queue.value[0], true);
}

function playSourceTrack(item: LibraryItem) {
  const playlist = activePlaylist.value;
  if (!playlist) return;
  const selectedIndex = playlist.tracks.findIndex((track) => track.id === item.id);
  if (selectedIndex < 0) return;
  const sequence = createSelectedTrackQueue(playlist.tracks, selectedIndex, repeatMode.value);
  queueGeneration.value += 1;
  queueActivationPending.value = true;
  queuePlaylistId.value = playlist.id;
  queueSource.value = playlist.source;
  queue.value = sequence.tracks;
  queueIndex.value = sequence.index;
  queueShuffled.value = false;
  logUi(`player queue selected: playlist=${playlist.id}, visible=${playlist.tracks.length}, queue=${sequence.tracks.length}, index=${sequence.index}, repeat=${repeatMode.value}, generation=${queueGeneration.value}, track=${item.id}`);
  void play(item, true);
}

function updateCurrentTrackHighlight() {
  const current = currentTrack.value;
  document.querySelectorAll<HTMLElement>('.track-row').forEach((row) => {
    const title = row.querySelector<HTMLElement>('.track-info strong')?.textContent ?? '';
    const artist = (row.querySelector<HTMLElement>('.track-info small')?.textContent ?? '').split(' · ')[0];
    const isCurrent = Boolean(current && sameTrackByTitleAndArtist({ title, artist }, current));
    row.classList.toggle('current-track-match', isCurrent);
    row.classList.toggle('current-track-playing', isCurrent && isPlaying.value);
    if (isCurrent) {
      row.setAttribute('aria-current', 'true');
      row.setAttribute('aria-label', `Faixa ${title}, ${isPlaying.value ? 'reproduzindo agora' : 'selecionada, pausada'}`);
    } else {
      row.removeAttribute('aria-current');
      row.removeAttribute('aria-label');
    }
  });
}

function savePlaylistSettings() {
  if (playlistSettingsId.value) playlistModes.value[playlistSettingsId.value] = playlistSettingsMode.value;
  playlistSettingsOpen.value = false;
}

async function syncGoogle() {
  try {
      await googleSyncService.connectYouTubeMusic();
      onlinePlaylists.value = await googleSyncService.loadYouTubePlaylists();
      await hydratePlaylistCatalog();
    googleConnected.value = true;
    showToast(`${onlinePlaylists.value.length} playlists carregadas.`, 'success');
  } catch (error) {
    try {
      await googleSyncService.connectYouTubeMusic(true);
        onlinePlaylists.value = await googleSyncService.loadYouTubePlaylists();
        await hydratePlaylistCatalog();
      googleConnected.value = true;
      showToast(`${onlinePlaylists.value.length} playlists carregadas.`, 'success');
    } catch (retryError) {
      showToast(retryError instanceof Error ? retryError.message : 'Não foi possível conectar ao YouTube Music.');
    }
  }
}

async function disconnectGoogle() {
  try {
    await googleSyncService.disconnectYouTubeMusic();
    googleConnected.value = false;
    onlinePlaylists.value = [];
    showToast('Conta Google desconectada.', 'success');
  } catch (error) {
    const normalized = reportBridgeFailure(error, 'disconnect_youtube_music');
    showToast(normalized.message || 'Não foi possível desconectar a conta Google.');
  }
}

async function restoreGoogleSession() {
  try {
    googleConnected.value = await googleSyncService.restoreYouTubeSession();
    if (googleConnected.value) {
      onlinePlaylists.value = await googleSyncService.loadYouTubePlaylists();
      await hydratePlaylistCatalog();
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

async function play(item: LibraryItem, fromQueue = false, forceRestart = false, selectedTrackIndex?: number) {
  const visiblePlaylist = activePlaylist.value;
  if (!fromQueue && visiblePlaylist?.id.startsWith('VLPL') && visiblePlaylist.source === 'online' && !fullyLoadedPlaylists.value.has(visiblePlaylist.id)) {
    const playlistId = visiblePlaylist.id;
    const generation = playlistOpenGeneration;
    logUi(`player queue activation waiting for terminal: playlist=${playlistId}, visible=${visiblePlaylist.tracks.length}, terminal=${visiblePlaylist.terminal === true}, cursor=${Boolean(visiblePlaylist.continuation)}, generation=${generation}`);
    await loadAllPlaylistTracks(playlistId, undefined, generation);
    if (activePlaylist.value.id !== playlistId || !fullyLoadedPlaylists.value.has(playlistId)) {
      logUi(`player queue activation blocked: playlist=${playlistId}, visible=${visiblePlaylist.tracks.length}, terminal=${visiblePlaylist.terminal === true}, active=${activePlaylist.value.id}, generation=${generation}`);
      showToast('A playlist ainda não confirmou o fim do carregamento; a reprodução não foi iniciada com uma fila parcial.');
      return;
    }
    logUi(`player queue activation resumed: playlist=${playlistId}, visible=${visiblePlaylist.tracks.length}, terminal=${visiblePlaylist.terminal === true}, generation=${generation}`);
  }
  const playlist = fromQueue && queuePlaylistId.value
    ? { id: queuePlaylistId.value, name: 'Fila do player', source: queueSource.value, mode: queueSource.value === 'youtube' || queueSource.value === 'online' ? 'online' as const : 'local' as const, tracks: [...queue.value] }
    : visiblePlaylist;
  if (!fromQueue && visiblePlaylist?.tracks.some((track) => track.id === item.id)) {
    const selected = selectedTrackIndex !== undefined
      && selectedTrackIndex >= 0
      && selectedTrackIndex < visiblePlaylist.tracks.length
      && visiblePlaylist.tracks[selectedTrackIndex].id === item.id
      ? selectedTrackIndex
      : visiblePlaylist.tracks.findIndex((track) => track.id === item.id);
    const sequence = createSelectedTrackQueue(visiblePlaylist.tracks, selected, repeatMode.value);
    queueGeneration.value += 1;
    queueActivationPending.value = true;
    queuePlaylistId.value = visiblePlaylist.id;
    queueSource.value = visiblePlaylist.source;
    queue.value = sequence.tracks;
    queueIndex.value = sequence.index;
    queueShuffled.value = false;
    logUi(`player queue built: playlist=${visiblePlaylist.id}, visible=${visiblePlaylist.tracks.length}, queue=${sequence.tracks.length}, index=${sequence.index}, repeat=${repeatMode.value}, generation=${queueGeneration.value}, selected=${item.id}, ids=${sequence.tracks.map((track) => track.id).join(',')}`);
  } else if (!queueOwnsTrack(item, playlist)) setQueueContext(playlist);
  if (!playlist || queuePlaylistId.value !== playlist.id || !playlist.tracks.some((track) => track.id === item.id)) return;
  queueIndex.value = queueIndexForTrack(queue.value, queueIndex.value, item.id);
  if (queueIndex.value < 0) return;
  if (currentTrack.value?.id === item.id
    && activePlaybackQueueIndex === queueIndex.value
    && !queueActivationPending.value
    && !forceRestart) {
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
  const requestGeneration = ++playbackGeneration;
  const activationSequence = ++playbackActivationSequence;
  activePlaybackQueueIndex = queueIndex.value;
  nativeSeekPending.value = null;
  if (nativeSeekTimer) {
    window.clearTimeout(nativeSeekTimer);
    nativeSeekTimer = undefined;
  }
  activePlaybackAbort?.abort();
  const abortController = new AbortController();
  activePlaybackAbort = abortController;
  const request: PlayerRequest = { generation: requestGeneration, playlistId: playlist.id, trackId: item.id };
  activePlayerRequest = request;
  logUi(`player queue activation start: intent=${activationSequence}, playlist=${playlist.id}, source=${queueSource.value}, queue=${queue.value.length}, index=${queueIndex.value}, repeat=${repeatMode.value}, generation=${queueGeneration.value}, request=${requestGeneration}, track=${item.id}, activationPending=${queueActivationPending.value}`);
  visualizerRequestGeneration += 1;
  nativeTrackEnded.value = false;
  currentTrack.value = item;
  isPlaying.value = false;
  usingNativeAudio.value = false;
  visualizerLevels.value = [];
  currentTime.value = 0;
  duration.value = item.durationSeconds ?? 0;
  if (!item.audioUrl && item.youtubeId) {
    try {
        const stream = await googleSyncService.playAudio(item.youtubeId, { track: item, queue: [...queue.value], playlistId: playlist.id, queueSource: queueSource.value, shuffled: queueShuffled.value, repeatMode: repeatMode.value, terminalBehavior: terminalBehaviorForRepeatMode(repeatMode.value), queueGeneration: queueGeneration.value, queueIndex: queueIndex.value, activationSequence }, abortController.signal);
        if (!isCurrentPlayerRequest(request, activePlayerRequest)) {
          logUi(`player queue activation stale completion: intent=${activationSequence}, request=${requestGeneration}, track=${item.id}`);
          return;
        }
        logUi(`player queue activation complete: intent=${activationSequence}, request=${requestGeneration}, queueGeneration=${queueGeneration.value}, index=${queueIndex.value}, track=${item.id}, native=true`);
        if (stream.durationSeconds) { duration.value = Number(stream.durationSeconds); item.durationSeconds = duration.value; item.duration = formatTime(duration.value); }
        nativeVolumePending.value = currentVolumeIntent();
        try { await syncNativeVolume(); } catch {
         // Playback should continue even if the native volume preference is temporarily unavailable.
       }
       usingNativeAudio.value = true;
      isPlaying.value = true;
    } catch (error) {
      if (!isCurrentPlayerRequest(request, activePlayerRequest)) return;
      queueActivationPending.value = false;
      activePlaybackQueueIndex = -1;
      isPlaying.value = false;
      logUi(`player queue activation failed: intent=${activationSequence}, request=${requestGeneration}, track=${item.id}, native=true`);
      showToast(error instanceof Error ? error.message : 'Não foi possível resolver o áudio online.');
    }
    return;
  }
  if (item.source === 'local' && !item.audioUrl) {
     try {
           const snapshot = await localLibraryService.playTrack(item.id, item.managedPath, { track: item, queue: [...queue.value], playlistId: playlist.id, queueSource: queueSource.value, shuffled: queueShuffled.value, repeatMode: repeatMode.value, terminalBehavior: terminalBehaviorForRepeatMode(repeatMode.value), queueGeneration: queueGeneration.value, queueIndex: queueIndex.value, activationSequence });
       if (!isCurrentPlayerRequest(request, activePlayerRequest)) {
         logUi(`player queue activation stale completion: intent=${activationSequence}, request=${requestGeneration}, track=${item.id}`);
         return;
       }
       applyNativePlayerSnapshot(snapshot);
       logUi(`player queue activation complete: intent=${activationSequence}, request=${requestGeneration}, queueGeneration=${snapshot.queueGeneration}, index=${snapshot.queueIndex}, revision=${snapshot.revision}, track=${item.id}, native=true`);
       nativeVolumePending.value = currentVolumeIntent();
       try { await syncNativeVolume(); } catch {
        // Playback should continue even if the native volume preference is temporarily unavailable.
      }
      usingNativeAudio.value = true;
      isPlaying.value = true;
     } catch (error) {
      if (!isCurrentPlayerRequest(request, activePlayerRequest)) return;
      queueActivationPending.value = false;
      activePlaybackQueueIndex = -1;
      isPlaying.value = false;
      logUi(`player queue activation failed: intent=${activationSequence}, request=${requestGeneration}, track=${item.id}, native=true`);
       const normalized = reportBridgeFailure(error, 'play_local_audio');
       showToast(localPlaybackErrorMessage(normalized));
    }
    return;
  }
  if (!audio.value || !item.audioUrl) {
    queueActivationPending.value = false;
    activePlaybackQueueIndex = -1;
    return;
  }
  nextTick(() => {
    if (!isCurrentPlayerRequest(request, activePlayerRequest)) return;
    if (!audio.value) return;
    audio.value.src = item.audioUrl as string;
    audio.value.volume = volume.value;
    audio.value.muted = isMuted.value;
    void audio.value.play();
    queueActivationPending.value = false;
  });
}

async function togglePlayback() {
  if (!currentTrack.value) return;
  if (usingNativeAudio.value) {
    if (isPlaying.value) {
       try { await googleSyncService.setAudioPaused(true); isPlaying.value = false; visualizerLevels.value = []; }
      catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível pausar o áudio.'); }
    } else {
      try { await googleSyncService.setAudioPaused(false); isPlaying.value = true; }
      catch (error) { showToast(error instanceof Error ? error.message : 'Não foi possível retomar o áudio.'); }
    }
    return;
  }
  if (!audio.value) return;
   if (isPlaying.value) {
     audio.value.pause();
     visualizerLevels.value = [];
   }
  else void audio.value.play();
  isPlaying.value = !isPlaying.value;
}

function moveInQueue(direction: 1 | -1) {
  if (!queue.value.length || !currentTrack.value) return;
  const index = queueIndex.value >= 0 ? queueIndex.value : queue.value.findIndex((item) => item.id === currentTrack.value?.id);
  if (index < 0) return;
  const intent = createQueueMoveIntent(index, queue.value.length, direction, repeatMode.value, playbackActivationSequence);
  logUi(`player queue manual intent=${intent?.activationSequence ?? playbackActivationSequence}: direction=${direction}, from=${index}:${queue.value[index]?.id ?? 'none'}, to=${intent ? `${intent.toIndex}:${queue.value[intent.toIndex]?.id ?? 'none'}` : 'terminal'}, queue=${queue.value.length}, repeat=${repeatMode.value}, generation=${queueGeneration.value}, activationPending=${queueActivationPending.value}`);
  if (!intent) return;
  queueIndex.value = intent.toIndex;
  queueActivationPending.value = true;
  void play(queue.value[intent.toIndex], true);
}

function playQueueTrack(selection: PlayerQueuePanelEvents['select-track']) {
  const item = queue.value[selection.queueIndex];
  if (!item || item.id !== selection.trackId) return;
  queueIndex.value = selection.queueIndex;
  queueActivationPending.value = true;
  void play(item, true);
}

function cycleRepeatMode() {
  const previousMode = repeatMode.value;
  repeatMode.value = nextRepeatMode(previousMode);
  if (currentTrack.value && queue.value.length) {
    const currentIndex = queueIndex.value >= 0 ? queueIndex.value : queue.value.findIndex((item) => item.id === currentTrack.value?.id);
    if (currentIndex >= 0 && repeatMode.value !== previousMode) {
      const sourcePlaylist = playlists.value.find((item) => item.id === queuePlaylistId.value);
      const sequence = queueForRepeatTransition(
        queue.value,
        currentIndex,
        previousMode,
        repeatMode.value,
        sourcePlaylist?.tracks ?? queue.value,
        currentTrack.value.id,
      );
      queue.value = sequence.tracks;
      queueIndex.value = sequence.index;
      queueGeneration.value += 1;
      queueActivationPending.value = usingNativeAudio.value;
    }
    if (usingNativeAudio.value) {
      const request = {
        repeatMode: repeatMode.value,
        terminalBehavior: terminalBehaviorForRepeatMode(repeatMode.value),
        playlistId: queuePlaylistId.value ?? '',
        trackId: currentTrack.value?.id ?? '',
        baseRevision: nativePlayerRevision.value,
        queueSource: queueSource.value,
        queueGeneration: queueGeneration.value,
        queueIndex: queueIndex.value,
        queue: [...queue.value],
        shuffled: queueShuffled.value,
      };
      nativeRepeatSync = nativeRepeatSync
        .then(async () => { applyNativePlayerSnapshot(await googleSyncService.setRepeatMode(request)); })
        .catch((error) => { reportBridgeFailure(error, 'set_repeat_mode'); });
    }
  }
}

function moveAfterTrackEnded(expected?: { trackId?: string; queueGeneration: number; queueIndex: number }) {
  if (queueActivationPending.value) return;
  if (!currentTrack.value || !queue.value.length) return;
  const index = queueIndex.value >= 0 ? queueIndex.value : queue.value.findIndex((item) => item.id === currentTrack.value?.id);
  if (expected && (expected.trackId !== currentTrack.value.id || expected.queueGeneration !== queueGeneration.value || expected.queueIndex !== index)) return;
  if (index < 0) return;
  logUi(`player queue track ended: track=${currentTrack.value.id}, index=${index}, queue=${queue.value.length}, repeat=${repeatMode.value}, generation=${queueGeneration.value}, expected=${expected?.trackId ?? 'browser'}`);
  const nextIndex = nextQueueIndex(index, queue.value.length, repeatMode.value);
  if (nextIndex === null) {
    logUi(`player queue terminal reached: track=${currentTrack.value.id}, index=${index}, queue=${queue.value.length}, repeat=${repeatMode.value}, generation=${queueGeneration.value}`);
    isPlaying.value = false;
    void googleSyncService.setAudioPaused(true);
    return;
  }
  queueIndex.value = nextIndex;
  if (nextIndex === index) isPlaying.value = false;
  void play(queue.value[queueIndex.value], true, nextIndex === index);
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
  const target = localPlaylistTarget(playlistId);
  if (!target || !supported.length) return;
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
  imported.forEach((item) => { if (item.audioUrl) transientObjectUrls.add(item.audioUrl); });
  if (!imported.length) return;
  localTracks.value.push(...imported);
  if (playlistId === 'favorites') favoriteTracks.value.push(...imported);
  if (playlistId === selectedPlaylistId.value) setQueueContext(playlists.value.find((playlist) => playlist.id === playlistId));
}

function importFromInput(event: Event) {
  const input = event.target as HTMLInputElement;
  const files = Array.from(input.files ?? []);
  const paths = extractNativePaths(files.map((file) => (file as File & { path?: string }).path).filter((path): path is string => Boolean(path)));
  if (isTauriRuntime()) {
    if (paths.length) void importNativePaths(paths);
    else showToast('O seletor nativo não forneceu caminhos. Use arrastar e soltar arquivos ou pastas.');
  } else addFilesToPlaylist(files);
  input.value = '';
}

function handleDrop(event: DragEvent, playlistId = selectedPlaylistId.value) {
  dropTarget.value = null;
  const paths = extractNativePaths(Array.from(event.dataTransfer?.files ?? []).map((file) => (file as File & { path?: string }).path).filter((path): path is string => Boolean(path)));
  if (isTauriRuntime() && paths.length) void importNativePaths(paths, playlistId);
  else addFilesToPlaylist(Array.from(event.dataTransfer?.files ?? []), playlistId);
}

function handleMetadata() {
  if (!audio.value || !currentTrack.value) return;
  duration.value = audio.value.duration;
  currentTrack.value.durationSeconds = audio.value.duration;
}

function handleTimeUpdate() { if (audio.value) currentTime.value = audio.value.currentTime; }
function scheduleNativeSeekFlush() {
  if (nativeSeekTimer) window.clearTimeout(nativeSeekTimer);
  nativeSeekTimer = window.setTimeout(() => {
    nativeSeekTimer = undefined;
    void flushNativeSeek();
  }, 180);
}
async function flushNativeSeek() {
  const intent = nativeSeekPending.value;
  if (!intent || !usingNativeAudio.value || currentTrack.value?.id !== intent.trackId || queueGeneration.value !== intent.queueGeneration) return;
  const request: NativeSeekRequest = { ...intent, revision: nativePlayerRevision.value };
  try {
    const snapshot = await googleSyncService.seekAudio(request);
    applyNativePlayerSnapshot(snapshot);
    if (nativeSeekPending.value?.sequence === intent.sequence && nativeSeekPending.value && Math.abs(snapshot.position - intent.seconds) > 0.35) scheduleNativeSeekFlush();
  } catch (error) {
    if (nativeSeekPending.value?.sequence !== intent.sequence) return;
    try {
      const snapshot = await googleSyncService.getPlayerSnapshot();
      applyNativePlayerSnapshot(snapshot);
      if (nativeSeekPending.value?.sequence === intent.sequence) scheduleNativeSeekFlush();
    } catch {
      nativeSeekPending.value = null;
      nativeSeekHoldUntil = 0;
      showToast(error instanceof Error ? error.message : 'Não foi possível buscar a posição da música.');
    }
  }
}
function seek(event: Event) {
  if (!duration.value) return;
  const seconds = (Number((event.target as HTMLInputElement).value) / 100) * duration.value;
  if (usingNativeAudio.value) {
    currentTime.value = seconds;
    visualizerRequestGeneration += 1;
    visualizerLevels.value = [];
    nativeSeekHoldUntil = Date.now() + 1200;
    nativeSeekPending.value = {
      trackId: currentTrack.value?.id ?? '',
      queueGeneration: queueGeneration.value,
      seconds,
      baseRevision: nativePlayerRevision.value,
      sequence: ++nativeSeekSequence,
    };
    scheduleNativeSeekFlush();
    return;
  }
  if (!audio.value) return;
  audio.value.currentTime = seconds;
  currentTime.value = audio.value.currentTime;
}
function currentVolumeIntent(): VolumeIntent {
  return { volume: volume.value, muted: isMuted.value, sequence: ++nativeVolumeSequence };
}
async function syncNativeVolume() {
  const intent = nativeVolumePending.value;
  if (!intent || !usingNativeAudio.value) return;
  try {
    const snapshot = await googleSyncService.setAudioVolume(intent);
    if (nativeVolumePending.value?.sequence === intent.sequence) applyNativePlayerSnapshot(snapshot);
  } catch (error) {
    if (nativeVolumePending.value?.sequence === intent.sequence) nativeVolumePending.value = null;
    throw error;
  }
}
function scheduleNativeVolumeSync() {
  if (nativeVolumeTimer !== undefined) window.clearTimeout(nativeVolumeTimer);
  nativeVolumeTimer = window.setTimeout(() => {
    nativeVolumeTimer = undefined;
    void syncNativeVolume().catch((error) => showToast(error instanceof Error ? error.message : 'Não foi possível alterar o volume.'));
  }, 16);
}
function updateVolume() {
  isMuted.value = volume.value === 0;
  nativeVolumePending.value = currentVolumeIntent();
  if (usingNativeAudio.value) {
    scheduleNativeVolumeSync();
  }
  if (audio.value) {
    audio.value.volume = volume.value;
    audio.value.muted = isMuted.value;
  }
}

async function refreshNativeAudioPosition() {
  if (!usingNativeAudio.value || queueActivationPending.value) return;
  const endedContext = { trackId: currentTrack.value?.id, queueGeneration: queueGeneration.value, queueIndex: queueIndex.value };
  if (nativeSeekPending.value) return;
  if (Date.now() < nativeSeekHoldUntil) return;
  try {
    currentTime.value = await googleSyncService.getAudioPosition();
    const nativeDuration = await googleSyncService.getAudioDuration();
    if (endedContext.trackId !== currentTrack.value?.id
      || endedContext.queueGeneration !== queueGeneration.value
      || endedContext.queueIndex !== queueIndex.value) return;
    if (nativeDuration > 0) duration.value = nativeDuration;
    if (duration.value > 0 && currentTime.value >= duration.value - 0.05 && !nativeTrackEnded.value) {
      nativeTrackEnded.value = true;
      moveAfterTrackEnded(endedContext);
    } else if (currentTime.value < duration.value - 1) {
      nativeTrackEnded.value = false;
    }
  } catch {
    // Native position is best-effort while the stream is being replaced.
  }
}
async function refreshVisualizerLevels() {
  if (!usingNativeAudio.value || visualizerRequestPending) return;
  visualizerRequestPending = true;
  const requestGeneration = visualizerRequestGeneration;
  try {
     const levels = await googleSyncService.getAudioLevels();
     if (!usingNativeAudio.value || requestGeneration !== visualizerRequestGeneration) return;
     const compensation = isMuted.value || volume.value <= 0
       ? 0
       : Math.min(6, Math.max(1, 0.8 / Math.max(volume.value, 0.08)));
     visualizerLevels.value = levels.map((level) => Math.min(1, level * compensation));
  } catch {
    // Visualizer data is best-effort and must not interrupt playback.
  } finally {
    visualizerRequestPending = false;
  }
}
function toggleMute() {
  isMuted.value = !isMuted.value;
  nativeVolumePending.value = currentVolumeIntent();
  if (usingNativeAudio.value) {
    scheduleNativeVolumeSync();
  }
  if (audio.value) audio.value.muted = isMuted.value;
}
function handleEnded() { moveAfterTrackEnded(); }
function handleAudioError() { isPlaying.value = false; showToast('Não foi possível reproduzir esta faixa online. Consulte o log do VibeDeck.'); }
function formatTime(value: number) { return !Number.isFinite(value) || value <= 0 ? '--:--' : `${Math.floor(value / 60)}:${String(Math.floor(value % 60)).padStart(2, '0')}`; }
function handlePlayerBackgroundClick(event: MouseEvent) {
  const target = event.target as HTMLElement;
  if (target.closest('.player-bar') && !target.closest('button, input')) visualizerOpen.value = !visualizerOpen.value;
}
async function openNativePicker(directory: boolean) {
  if (!isTauriRuntime()) {
    (directory ? folderInput : fileInput).value?.click();
    return;
  }
  try {
    const paths = selectedDialogPaths(await localLibraryService.pickPaths(directory));
    if (paths.length) await importNativePaths(paths);
  } catch (error) {
    showToast(error instanceof Error ? error.message : 'Não foi possível abrir o seletor nativo.');
  }
}
function handlePickerSelection(directory: boolean) { void openNativePicker(directory); }
function sourceLabel(source: LibrarySource | PlaylistMode) { return source === 'local' ? 'Local' : source === 'youtube' ? 'YouTube Music' : source === 'online' ? 'Online' : 'Misto'; }
function handleVisualizerSelection(event: Event) {
  const detail = (event as CustomEvent<string>).detail;
  if (detail === 'spectrogram-toggle') {
    if (spectrogramOpen.value) {
      selectedVisualizer.value = normalVisualizer.value;
      spectrogramOpen.value = false;
    } else {
      if (selectedVisualizer.value !== 'spectrogram') normalVisualizer.value = selectedVisualizer.value;
      selectedVisualizer.value = 'spectrogram';
      spectrogramOpen.value = true;
    }
    return;
  }
  if (detail) setVisualizer(detail as VisualizerKind);
}

onMounted(async () => {
  document.addEventListener('click', handlePlayerBackgroundClick);
  window.addEventListener('vibedeck:select-visualizer', handleVisualizerSelection);
  window.addEventListener('vibedeck:shuffle-playlist', shuffleActivePlaylist);
  document.addEventListener('scroll', handleCapturedScroll, true);
  void refreshAppLogs();
  void refreshAppWebviews();
  audioPositionTimer = window.setInterval(refreshNativeAudioPosition, 250);
  // Poll below the native FFT hop rate without stacking bridge requests.
  audioLevelTimer = window.setInterval(refreshVisualizerLevels, VISUALIZER_POLL_INTERVAL_MS);
  unlistenLogs = await listenAppLogs(pushAppLog);
  unlistenLocalImport = await localLibraryService.listenProgress((progress) => {
    if (localImport.value?.operationId !== progress.operationId || localImportGeneration !== localLibraryGeneration) return;
    localImport.value = { ...localImport.value, ...progress };
  });
  if (isTauriRuntime()) {
    try {
      unlistenPlayerState = await googleSyncService.listenPlayerState((event) => {
        applyNativePlayerSnapshot(event.snapshot);
        if (event.end && nativePlayerSnapshot.value && isCurrentPlayerEndEvent(event.end, nativePlayerSnapshot.value)) {
          logUi(`player queue native ended event: track=${event.end.trackId}, index=${event.end.queueIndex}, generation=${event.end.queueGeneration}, revision=${event.end.revision}, terminal=${event.end.terminalBehavior}`);
          moveAfterTrackEnded(event.end);
        }
        if (event.kind === 'error' || event.snapshot.error) {
          nativePlayerConnection.value = 'error';
          schedulePlayerRehydrate();
        }
      });
    } catch (error) {
      nativePlayerConnection.value = 'disconnected';
      schedulePlayerRehydrate();
      logUi(`player event subscription failed: ${error instanceof Error ? error.message : 'bridge unavailable'}`);
    }
    unlistenNativeDrop = await listen<unknown>('tauri://drag-drop', (event) => {
      try {
        void importNativePaths(requireNativePaths(event.payload));
      } catch (error) {
        showToast(error instanceof Error ? error.message : 'O drop nativo não forneceu um caminho utilizável.');
      }
    });
  }
  void loadLocalLibrary();
   void restoreNativePlayerReference();
    void hydratePlaylistCatalog().then(() => restoreGoogleSession()).then(restoreNativePlayerReference);
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
  void nextTick(() => {
    bindTrackListScroll();
    updateCurrentTrackHighlight();
  });
});
watch([currentTrack, isPlaying, filteredTracks], () => void nextTick(updateCurrentTrackHighlight));
watch([darkMode, developerMode, selectedVisualizer, serpentDensity, serpentDetail, serpentGlow, serpentResolution, volume, isMuted, visualizerSensitivity, visualizerSmoothing, visualizerIntensity, visualizerMotion, mascotBassSensitivity, mascotBassDistance, bassMinHz, bassMaxHz, energyMinHz, energyMaxHz, voiceMinHz, voiceMaxHz, motionMinHz, motionMaxHz], saveUserSettings);
watch(canRevealActivePlaylistTracks, (revealed) => document.body.classList.toggle('playlist-is-loading', !revealed), { immediate: true });
watch(developerMode, (enabled) => document.body.classList.toggle('developer-mode', enabled), { immediate: true });
watch([screen, developerMode], updateLogsPolling);
onBeforeUnmount(() => {
  document.removeEventListener('click', handlePlayerBackgroundClick);
  window.removeEventListener('vibedeck:select-visualizer', handleVisualizerSelection);
  window.removeEventListener('vibedeck:shuffle-playlist', shuffleActivePlaylist);
  document.removeEventListener('scroll', handleCapturedScroll, true);
  unlistenLogs?.();
  unlistenLocalImport?.();
  unlistenNativeDrop?.();
  unlistenPlayerState?.();
  transientObjectUrls.forEach((url) => URL.revokeObjectURL(url));
  transientObjectUrls.clear();
  if (logsRefreshTimer) window.clearInterval(logsRefreshTimer);
  if (audioPositionTimer) window.clearInterval(audioPositionTimer);
  if (audioLevelTimer) window.clearInterval(audioLevelTimer);
  if (nativeVolumeTimer !== undefined) window.clearTimeout(nativeVolumeTimer);
  if (playerReconnectTimer !== undefined) window.clearTimeout(playerReconnectTimer);
  seekControlObserver?.disconnect();
});
</script>

<template>
  <div :class="['app-shell', { 'light-theme': !darkMode, 'logs-open': screen === 'logs' }]">
    <aside class="sidebar">
       <div class="brand"><div><span>Vibe</span><span class="brand-accent">Deck</span></div></div>
      <nav class="main-nav" aria-label="Navegacao principal"><button :class="['nav-item', { active: screen === 'home' }]" @click="screen = 'home'"><span>⌂</span> Inicio</button><button :class="['nav-item', { active: screen === 'libraries' }]" @click="screen = 'libraries'"><span>▱</span> Fontes</button></nav>
      <div class="source-section"><div class="section-label">FONTES</div><button v-for="library in libraryCards" :key="library.id" :class="['source-item', { active: screen === 'playlist' && selectedPlaylistId === library.id }]" @click="library.id === 'local' ? openPlaylist('local') : (screen = 'libraries')" @dragenter.prevent="dropTarget = library.id" @dragover.prevent="dropTarget = library.id" @dragleave.prevent="dropTarget = null" @drop.prevent="handleDrop($event, library.id)"><span class="source-dot" :style="{ background: library.accent }"></span>{{ library.name }}</button></div>
      <div class="playlist-section"><div class="section-label">PLAYLISTS</div><button v-for="playlist in playlists" :key="playlist.id" :class="['playlist-item', { active: screen === 'playlist' && selectedPlaylistId === playlist.id, 'drop-target-item': dropTarget === playlist.id }]" @click="openPlaylist(playlist.id)" @dragenter.prevent="dropTarget = playlist.id" @dragover.prevent="dropTarget = playlist.id" @dragleave.prevent="dropTarget = null" @drop.prevent="handleDrop($event, playlist.id)"><span>♫</span>{{ playlist.name }}<span class="playlist-gear" aria-label="Configurar playlist" @click.stop="openPlaylistSettings(playlist)">⚙</span></button></div>
      <div class="sidebar-footer"></div>
    </aside>

      <main class="content"><header class="topbar"><label class="search-box"><span>⌕</span><input v-model="query" :placeholder="screen === 'playlist' ? 'Buscar nesta playlist' : 'Buscar em suas bibliotecas'" @keydown.enter="searchLibrary" /></label><button v-if="developerMode" class="settings header-settings header-logs" @click="openLogs">≣ Logs</button><button class="settings header-settings" @click="settingsOpen = true">⚙ Configurações</button><AccountMenu :connected="googleConnected" @connect="void syncGoogle()" @disconnect="void disconnectGoogle()" /></header>
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
          <div v-if="localLibraryLoading && activePlaylist.source === 'local'" class="empty-state playlist-loading" role="status" aria-live="polite"><span class="loading-spinner" aria-hidden="true"></span><strong>Carregando biblioteca local</strong><span>Consultando o catálogo nativo...</span></div>
          <div v-else-if="localLibraryError && activePlaylist.source === 'local'" class="empty-state local-library-error" role="alert"><strong>Biblioteca local indisponível</strong><span>{{ localLibraryError }}</span><button @click="loadLocalLibrary">Tentar novamente</button></div>
          <div v-if="isActivePlaylistLoading" class="empty-state playlist-loading" role="status" aria-live="polite"><span class="loading-spinner" aria-hidden="true"></span><strong>Carregando {{ activePlaylist.name }}</strong><span>Buscando as faixas no YouTube Music...</span></div>
          <div v-if="localImport" class="local-import-status" role="status" aria-live="polite"><strong>{{ localImport.cancelled ? 'Cancelando importação...' : 'Importando arquivos locais' }}</strong><span>{{ localImport.processed }} de {{ localImport.total }} processados{{ localImport.currentPath ? ` · ${localImport.currentPath}` : '' }}</span><progress :max="Math.max(localImport.total, 1)" :value="localImport.processed"></progress><button :disabled="localImport.cancelled" @click="cancelLocalImport">Cancelar</button><small v-if="localImport.files.length">{{ summarizeImport(localImport.files).imported }} importada(s) · {{ summarizeImport(localImport.files).duplicates }} duplicata(s) · {{ summarizeImport(localImport.files).unsupported }} não suportada(s) · {{ summarizeImport(localImport.files).failed }} falha(s)</small></div>
           <Serpents3D v-if="selectedVisualizer === 'serpents'" class="playlist-visualizer" :levels="visualizerLevels" :active="isPlaying" :has-track="Boolean(currentTrack)" :density="serpentDensity" :detail="serpentDetail" :glow="serpentGlow" :resolution="serpentResolution" :sensitivity="visualizerSensitivity" :smoothing="visualizerSmoothing" :intensity="visualizerIntensity" :motion="visualizerMotion" />
           <VisualizerCanvas v-else class="playlist-visualizer" :kind="selectedVisualizer" :levels="visualizerLevels" :active="isPlaying" :has-track="Boolean(currentTrack)" :serpent-density="serpentDensity" :serpent-detail="serpentDetail" :serpent-glow="serpentGlow" :sensitivity="visualizerSensitivity" :smoothing="visualizerSmoothing" :intensity="visualizerIntensity" :motion="visualizerMotion" :bass-sensitivity="mascotBassSensitivity" :bass-distance="mascotBassDistance" :bass-min-hz="bassMinHz" :bass-max-hz="bassMaxHz" :energy-min-hz="energyMinHz" :energy-max-hz="energyMaxHz" :voice-min-hz="voiceMinHz" :voice-max-hz="voiceMaxHz" :motion-min-hz="motionMinHz" :motion-max-hz="motionMaxHz" compact />
          <section class="library-section"><div class="section-heading"><div><p class="eyebrow">FAIXAS · {{ sourceLabel(activePlaylist.mode) }}</p><h2>{{ query ? 'Resultados da busca' : activePlaylist.name }}</h2></div><button v-if="activePlaylist.id !== 'favorites'" class="heading-gear" aria-label="Configurar playlist" @click="openPlaylistSettings(activePlaylist)">⚙</button></div><div class="track-list" @dragenter.prevent="dropTarget = activePlaylist.id" @dragover.prevent="dropTarget = activePlaylist.id" @dragleave.prevent="dropTarget = null" @drop.prevent="handleDrop($event)"><div v-for="(item, index) in filteredTracks" :key="`${item.id}-${index}`" class="track-row" role="button" tabindex="0" @click="play(item, false, false, activePlaylist.tracks.indexOf(item))" @keydown.enter="play(item, false, false, activePlaylist.tracks.indexOf(item))" @keydown.space.prevent="play(item, false, false, activePlaylist.tracks.indexOf(item))"><span class="track-index">{{ currentTrack?.id === item.id && isPlaying ? '▶' : String(index + 1).padStart(2, '0') }}</span><span class="cover" :style="{ background: item.accent }"><img v-if="item.coverUrl" :src="item.coverUrl" alt="" /><template v-else>{{ item.title.slice(0, 1) }}</template></span><span class="track-info"><strong>{{ item.title }}</strong><small>{{ item.artist }} · {{ item.album }}</small></span><span :class="['origin-badge', item.source]"><i></i>{{ sourceLabel(item.source) }}</span><button class="favorite-button" :aria-label="isFavorite(item) ? 'Remover dos favoritos' : 'Adicionar aos favoritos'" @click.stop="toggleFavorite(item)">{{ isFavorite(item) ? '♥' : '♡' }}</button><span class="duration">{{ item.durationSeconds ? formatTime(item.durationSeconds) : item.duration }}</span></div><div v-if="!filteredTracks.length" class="empty-state"><strong>Playlist vazia</strong><span>Use a área abaixo para adicionar músicas locais.</span></div><MusicPickerMenu v-if="activePlaylist.allowLocal" :class="{ 'drop-target': dropTarget === activePlaylist.id }" @select="handlePickerSelection" /></div></section>
      </template>
      <section v-else class="hero home-hero"><div><p class="eyebrow">VIBEDECK</p><h1>Seu som, do seu jeito.</h1><p class="hero-copy">Abra uma playlist na lateral para começar.</p></div></section>
       <button v-if="currentTrack" class="repeat-control" :class="repeatMode" :title="repeatMode === 'off' ? 'Sem repetição' : repeatMode === 'playlist' ? 'Repetir playlist' : 'Repetir faixa'" @click="cycleRepeatMode"><Repeat :size="16" :stroke-width="1.8" /><span v-if="repeatMode === 'track'" class="repeat-badge">1</span></button></main>

     <footer class="player-bar"><div class="now-playing"><span class="mini-cover" :style="{ background: currentTrack?.accent || '#2a2f3a' }">{{ currentTrack?.title.slice(0, 1) || '♪' }}</span><div><strong>{{ currentTrack?.title || 'Nenhuma música selecionada' }}</strong><small>{{ currentTrack?.artist || 'Abra uma playlist para começar' }}</small></div><button :disabled="!currentTrack" :aria-label="currentTrack && isFavorite(currentTrack) ? 'Remover dos favoritos' : 'Adicionar aos favoritos'" @click="currentTrack && toggleFavorite(currentTrack)"><Heart :size="17" :stroke-width="1.8" :fill="currentTrack && isFavorite(currentTrack) ? 'currentColor' : 'none'" /></button></div><div class="player-controls"><div class="control-buttons"><button :disabled="!currentTrack" aria-label="Faixa anterior" @click="moveInQueue(-1)"><SkipBack :size="19" :stroke-width="1.8" /></button><button class="play-button" :disabled="!currentTrack" :aria-label="isPlaying ? 'Pausar' : 'Reproduzir'" @click="togglePlayback"><Pause v-if="isPlaying" :size="18" :stroke-width="2.2" /><Play v-else :size="18" :stroke-width="0" fill="currentColor" /></button><button :disabled="!currentTrack" aria-label="Próxima faixa" @click="moveInQueue(1)"><SkipForward :size="19" :stroke-width="1.8" /></button></div><div class="progress"><input aria-label="Posição da música" type="range" min="0" max="100" :value="progressPercent" :disabled="!currentTrack || usingNativeAudio" :title="usingNativeAudio ? 'Seek nativo ainda não está disponível' : undefined" @input="seek" /></div><div class="time-label"><span>{{ formatTime(currentTime) }}</span><span>{{ formatTime(duration) }}</span></div></div><div class="player-tools"><span class="player-origin"><i :class="currentTrack?.source || 'local'"></i>{{ currentTrack ? sourceLabel(currentTrack.source) : 'Local' }}</span><button class="mute-button" :disabled="!currentTrack" :aria-label="isMuted ? 'Ativar som' : 'Silenciar'" @click="toggleMute"><VolumeX v-if="isMuted" :size="18" :stroke-width="1.8" /><Volume2 v-else :size="18" :stroke-width="1.8" /></button><input v-model.number="volume" aria-label="Volume" class="volume-control" type="range" min="0" max="1" step="0.01" @input="updateVolume" /></div></footer>
     <img v-if="currentTrack?.coverUrl" class="player-thumbnail-overlay" :src="currentTrack.coverUrl" alt="" aria-hidden="true" /><div class="toast-stack" aria-live="polite"><div v-for="toast in toasts" :key="toast.id" :class="['toast', toast.tone]">{{ toast.message }}</div></div>
       <input ref="fileInput" class="hidden-input" type="file" accept="audio/*" multiple @change="importFromInput" />
       <input ref="folderInput" class="hidden-input" type="file" accept="audio/*" webkitdirectory multiple @change="importFromInput" />
<audio ref="audio" @loadedmetadata="handleMetadata" @ended="handleEnded" @timeupdate="handleTimeUpdate" @error="handleAudioError" />
       <Transition name="visualizer-drawer"><div v-if="visualizerOpen" class="visualizer-drawer"><section class="visualizer-drawer-content"><div class="visualizer-drawer-heading"><div><p class="eyebrow">VISUALIZADOR</p><h2>{{ currentTrack?.title || 'VibeDeck' }}</h2><small>{{ currentTrack?.artist || 'Selecione uma faixa para começar' }}</small></div></div><VisualizerControls v-model:sensitivity="visualizerSensitivity" v-model:smoothing="visualizerSmoothing" v-model:intensity="visualizerIntensity" v-model:motion="visualizerMotion" v-model:bass-sensitivity="mascotBassSensitivity" v-model:bass-distance="mascotBassDistance" v-model:bass-min-hz="bassMinHz" v-model:bass-max-hz="bassMaxHz" v-model:energy-min-hz="energyMinHz" v-model:energy-max-hz="energyMaxHz" v-model:voice-min-hz="voiceMinHz" v-model:voice-max-hz="voiceMaxHz" v-model:motion-min-hz="motionMinHz" v-model:motion-max-hz="motionMaxHz" :kind="selectedVisualizer" :visible="developerMode" /><Serpents3D v-if="selectedVisualizer === 'serpents'" :levels="visualizerLevels" :active="isPlaying" :density="serpentDensity" :detail="serpentDetail" :glow="serpentGlow" :resolution="serpentResolution" :sensitivity="visualizerSensitivity" :smoothing="visualizerSmoothing" :intensity="visualizerIntensity" :motion="visualizerMotion" /><VisualizerCanvas v-else :kind="selectedVisualizer" :levels="visualizerLevels" :active="isPlaying" :serpent-density="serpentDensity" :serpent-detail="serpentDetail" :serpent-glow="serpentGlow" :sensitivity="visualizerSensitivity" :smoothing="visualizerSmoothing" :intensity="visualizerIntensity" :motion="visualizerMotion" :bass-sensitivity="mascotBassSensitivity" :bass-distance="mascotBassDistance" :bass-min-hz="bassMinHz" :bass-max-hz="bassMaxHz" :energy-min-hz="energyMinHz" :energy-max-hz="energyMaxHz" :voice-min-hz="voiceMinHz" :voice-max-hz="voiceMaxHz" :motion-min-hz="motionMinHz" :motion-max-hz="motionMaxHz" /><div class="visualizer-drawer-status">{{ isPlaying ? 'Reagindo ao áudio em reprodução' : 'Dê play para ativar o movimento' }}</div></section></div></Transition>
     <div v-if="settingsOpen" class="settings-overlay" @click.self="settingsOpen = false"><section class="settings-panel settings-shell"><button class="close-settings" @click="settingsOpen = false">×</button><aside class="settings-nav"><p class="eyebrow">PREFERÊNCIAS</p><h2>Configurações</h2><button :class="{ selected: settingsSection === 'general' }" @click="settingsSection = 'general'">Geral<small>Tema e comportamento</small></button><button :class="{ selected: settingsSection === 'visualizer' }" @click="settingsSection = 'visualizer'">Visualizador<small>Estilo e qualidade</small></button><button :class="{ selected: settingsSection === 'developer' }" @click="settingsSection = 'developer'">Desenvolvedor<small>Ferramentas internas</small></button></aside><div class="settings-content"><template v-if="settingsSection === 'general'"><p class="eyebrow">GERAL</p><h2>Aparência e comportamento</h2><label class="setting-row"><span><strong>Tema</strong><small>Escolha a aparência do VibeDeck.</small></span><span class="theme-switch"><button :class="{ selected: darkMode }" @click="darkMode = true">Escuro</button><button :class="{ selected: !darkMode }" @click="darkMode = false">Claro</button></span></label></template><template v-else-if="settingsSection === 'developer'"><p class="eyebrow">DESENVOLVEDOR</p><h2>Ferramentas internas</h2><label class="setting-row"><span><strong>Modo desenvolvedor</strong><small>Exibe ferramentas internas, como o painel de logs em tempo real.</small></span><span class="switch-control"><input type="checkbox" :checked="developerMode" @change="setDeveloperMode(($event.target as HTMLInputElement).checked)" /><i></i></span></label></template><template v-else><p class="eyebrow">VISUALIZADOR</p><h2>Escolha como o som aparece</h2><div class="visualizer-options"><button :class="{ selected: selectedVisualizer === 'mascot' }" @click="setVisualizer('mascot')"><span class="visualizer-preview mascot-preview"><VisualizerCanvas kind="mascot" compact /></span><span class="visualizer-card-title"><b>Mascote</b><em>2D</em></span><small>Órbitas e partículas</small></button><button :class="{ selected: selectedVisualizer === 'bars' }" @click="setVisualizer('bars')"><span class="visualizer-preview"><VisualizerCanvas kind="bars" compact /></span><span class="visualizer-card-title"><b>Barras</b><em>2D</em></span><small>Espectro minimalista</small></button><button :class="{ selected: selectedVisualizer === 'serpents' }" @click="setVisualizer('serpents')"><span class="visualizer-preview serpents-preview"><Serpents3D :density="serpentDensity" :detail="serpentDetail" :glow="serpentGlow" :resolution="serpentResolution" compact /></span><span class="visualizer-card-title"><b>Serpentes</b><em>3D</em></span><small>Escamas, volume e bloom</small></button></div><div v-if="selectedVisualizer === 'serpents'" class="serpent-controls"><label><span>Densidade <b>{{ serpentDensity.toFixed(1) }}</b></span><input v-model.number="serpentDensity" type="range" min="0.6" max="1.6" step="0.1" /></label><label><span>Detalhe das escamas <b>{{ serpentDetail.toFixed(1) }}</b></span><input v-model.number="serpentDetail" type="range" min="0.6" max="1.8" step="0.1" /></label><label><span>Glow vermelho <b>{{ serpentGlow.toFixed(1) }}</b></span><input v-model.number="serpentGlow" type="range" min="0" max="1.8" step="0.1" /></label><label><span>Resolução 3D <b>{{ serpentResolution.toFixed(1) }}x</b></span><input v-model.number="serpentResolution" type="range" min="0.5" max="2" step="0.1" /></label></div></template></div></section></div><div v-if="playlistSettingsOpen" class="settings-overlay" @click.self="playlistSettingsOpen = false"><section class="settings-panel"><button class="close-settings" @click="playlistSettingsOpen = false">×</button><p class="eyebrow">PLAYLIST</p><h2>Configurar playlist</h2><p class="settings-description">Defina quais tipos de música podem entrar nesta playlist.</p><div class="playlist-mode-options"><label v-for="mode in (['local', 'online', 'mixed'] as PlaylistMode[])" :key="mode" :class="['mode-option', { selected: playlistSettingsMode === mode }]" @click="playlistSettingsMode = mode"><input v-model="playlistSettingsMode" type="radio" :value="mode" /><span><strong>{{ mode === 'local' ? 'Somente local' : mode === 'online' ? 'Somente online' : 'Online e local' }}</strong><small>{{ mode === 'local' ? 'Aceita arquivos deste computador.' : mode === 'online' ? 'Aceita apenas faixas sincronizadas.' : 'Mistura as duas bibliotecas.' }}</small></span></label></div><button class="save-settings" @click="savePlaylistSettings">Salvar configuração</button></section></div>
     <PlaylistShuffleButton v-if="screen === 'playlist' && activePlaylist?.tracks.length" />
      <QueuePanel v-model:open="queuePanelOpen" :tracks="queuePanelTracks" :current-track-id="currentTrack?.id ?? null" :current-track-index="queueIndex" :is-playing="isPlaying" @select-track="playQueueTrack" />
     <PlaylistLoadingOverlay v-if="isActivePlaylistLoading" />
    <VisualizerTrackArtwork v-if="visualizerOpen" :track="currentTrack" />
    <VisualizerPicker v-if="settingsOpen && settingsSection === 'visualizer'" v-model="selectedVisualizer" />
   </div>
 </template>
