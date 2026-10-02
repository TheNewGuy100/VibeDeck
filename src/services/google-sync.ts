import type { LibraryItem, Playlist } from '../models/library';
import type { PlaylistCatalogSnapshot, SavePlaylistPageRequest } from '../interfaces/library';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { decryptSecureRecord, encryptSecureRecord } from './secure-storage';
import type { PaginationPage } from './playlist-loader';
import type { NativeSeekRequest, NativePlayerActivation, PlayerSnapshot, PlayerStateEvent, RepeatModeRequest, VolumeIntent } from '../interfaces/player';
import { bridgeErrorMessage } from '../interfaces/bridge';
import { isPlayerStateEvent, parsePlayerSnapshot } from '../interfaces/player';
import { parseAnalyzerSnapshot } from '../interfaces/visualizer';

export interface GoogleSyncResult {
  playlists: Playlist[];
  tracks: LibraryItem[];
}

export type YouTubePlaylistPage = PaginationPage<LibraryItem>;

export interface YouTubeAudioStream {
  url: string;
  mimeType: string;
  bitrate?: number;
  expiresInSeconds?: number | string;
  durationSeconds?: number | string;
}

export type { NativePlayerActivation, NativeSeekRequest, PlayerSnapshot, PlayerStateEvent } from '../interfaces/player';

interface GoogleTokens {
  access_token: string;
  refresh_token?: string;
  expires_in?: number;
  expires_at?: number;
}

export class GoogleSyncService {
  private readonly clientId = import.meta.env.VITE_GOOGLE_CLIENT_ID as string | undefined;
  private accessToken?: string;
  private youtubeSession = false;
  private playlistsPromise?: Promise<Playlist[]>;

  isConfigured() {
    return Boolean(this.clientId);
  }

  async connect(): Promise<GoogleSyncResult> {
    if (!this.clientId) {
      throw new Error('Configure VITE_GOOGLE_CLIENT_ID para iniciar o OAuth do Google.');
    }

    const port = await invoke<number>('start_oauth_listener');
    const verifier = this.randomString(64);
    const state = this.randomString(32);
    const challenge = await this.pkceChallenge(verifier);
    const redirectUri = `http://127.0.0.1:${port}/oauth2callback`;
    const params = new URLSearchParams({ client_id: this.clientId, redirect_uri: redirectUri, response_type: 'code', scope: 'https://www.googleapis.com/auth/youtube', access_type: 'offline', prompt: 'consent', state, code_challenge: challenge, code_challenge_method: 'S256' });
    await invoke('open_external_url', { url: `https://accounts.google.com/o/oauth2/v2/auth?${params}` });

    const callback = await this.waitForCallback();
    const callbackUrl = new URL(`http://127.0.0.1${callback}`);
    if (callbackUrl.searchParams.get('state') !== state) throw new Error('Resposta OAuth inválida.');
    const code = callbackUrl.searchParams.get('code');
    if (!code) throw new Error(callbackUrl.searchParams.get('error') ?? 'Autorização Google cancelada.');
    let tokens: GoogleTokens;
    try {
      const tokenBody = await invoke<string>('exchange_google_code', { code, clientId: this.clientId, redirectUri, verifier });
      tokens = JSON.parse(tokenBody) as { access_token: string; refresh_token?: string; expires_in: number };
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      let googleError: { error?: string; error_description?: string };
      try { googleError = JSON.parse(message) as { error?: string; error_description?: string }; }
      catch { throw new Error(message); }
      throw new Error(`Google recusou o token (${googleError.error ?? 'invalid_request'}): ${googleError.error_description ?? 'resposta inválida'}`);
    }
    await this.storeTokens(tokens);
    return this.fetchLibrary(this.accessToken as string);
  }

  async restoreSession(): Promise<GoogleSyncResult | null> {
    if (!this.clientId) return null;
    const record = await invoke<string | null>('load_secure_record');
    if (!record) return null;
    const tokens = await decryptSecureRecord<GoogleTokens>(JSON.parse(record));
    if (!tokens.access_token && !tokens.refresh_token) return null;
    if (tokens.expires_at && tokens.expires_at <= Date.now() + 60_000) {
      if (!tokens.refresh_token) return null;
      await this.refresh(tokens.refresh_token);
    } else {
      this.accessToken = tokens.access_token;
    }
    return this.fetchLibrary(this.accessToken as string);
  }

  async resolveAudio(videoId: string, signal?: AbortSignal) {
    if (!this.youtubeSession) throw new Error('Faça login no YouTube Music antes de reproduzir uma faixa online.');
    this.throwIfAborted(signal);
    const stream = await this.invokeBridge<YouTubeAudioStream>('resolve_youtube_audio_bridge', { videoId }, (payload) => this.validateAudioStream(payload));
    this.throwIfAborted(signal);
    return stream;
  }

  async connectYouTubeMusic(forceLogin = false) {
    if (!forceLogin && await invoke<boolean>('youtube_session_available')) {
      this.youtubeSession = true;
      return;
    }
    await invoke('open_youtube_login');
    for (let attempt = 0; attempt < 300; attempt += 1) {
      if (await invoke<boolean>('capture_youtube_session')) {
        this.youtubeSession = true;
        return;
      }
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
    }
    throw new Error('Tempo limite aguardando o login no YouTube Music.');
  }

  async restoreYouTubeSession() {
    this.youtubeSession = await invoke<boolean>('youtube_session_available');
    return this.youtubeSession;
  }

  async disconnectYouTubeMusic() {
    await invoke('logout_youtube_music');
    this.youtubeSession = false;
    this.accessToken = undefined;
    this.playlistsPromise = undefined;
  }

  searchYouTube(query: string) {
    return invoke<LibraryItem[]>('search_youtube_music', { query });
  }

  loadYouTubePlaylists() {
    if (this.playlistsPromise) return this.playlistsPromise;
    this.playlistsPromise = this.invokeBridge<Playlist[]>('load_youtube_playlists', undefined, (payload) => this.validateArrayPayload(payload, 'playlists')).finally(() => {
      this.playlistsPromise = undefined;
    });
    return this.playlistsPromise;
  }

  loadYouTubePlaylistTracks(playlistId: string) {
    return this.invokeBridge<YouTubePlaylistPage>('load_youtube_playlist_tracks', { playlistId }, (payload) => this.validatePlaylistPage(payload, 'playlist tracks'));
  }

  loadYouTubePlaylistMoreTracks(playlistId: string) {
    return this.invokeBridge<YouTubePlaylistPage>('load_youtube_playlist_more_tracks', { playlistId }, (payload) => this.validatePlaylistPage(payload, 'playlist continuation tracks'));
  }

  loadPlaylistCatalog() {
    return invoke<PlaylistCatalogSnapshot>('load_playlist_catalog');
  }

  saveYouTubePlaylistPage(request: SavePlaylistPageRequest) {
    return invoke<PlaylistCatalogSnapshot>('save_youtube_playlist_page', { request });
  }

  async playAudio(videoId: string, activation: NativePlayerActivation, signal?: AbortSignal) {
    const stream = await this.resolveAudio(videoId, signal);
    this.throwIfAborted(signal);
    await invoke('play_audio_url', { url: stream.url, activation });
    this.throwIfAborted(signal);
    return stream;
  }

  stopAudio() {
    return invoke('stop_audio');
  }

  setAudioPaused(paused: boolean) {
    return invoke('set_audio_paused', { paused });
  }

  setRepeatMode(request: RepeatModeRequest) {
    return invoke<PlayerSnapshot>('set_audio_repeat_mode', { request });
  }

  setAudioVolume(intent: VolumeIntent) {
    return invoke<unknown>('set_audio_volume', { request: intent }).then(parsePlayerSnapshot);
  }

  getPlayerSnapshot() {
    return invoke<unknown>('get_player_snapshot').then(parsePlayerSnapshot);
  }

  listenPlayerState(callback: (event: PlayerStateEvent) => void): Promise<UnlistenFn> {
    return listen<unknown>('player-state', (event) => {
      if (isPlayerStateEvent(event.payload)) callback(event.payload);
    });
  }

  getAudioPosition() {
    return invoke<number>('get_audio_position');
  }

  getAudioDuration() {
    return invoke<number>('get_audio_duration');
  }

  getAudioLevels() {
    return invoke<unknown>('get_audio_levels').then((payload) => parseAnalyzerSnapshot(payload).levels);
  }

  isAudioActive() {
    return invoke<boolean>('is_audio_active');
  }

  isAudioPaused() {
    return invoke<boolean>('is_audio_paused');
  }

  seekAudio(request: NativeSeekRequest) {
    return invoke<unknown>('seek_audio', { request }).then(parsePlayerSnapshot);
  }

  private async refresh(refreshToken: string) {
    const body = await invoke<string>('refresh_google_token', { refreshToken, clientId: this.clientId });
    const refreshed = JSON.parse(body) as GoogleTokens;
    await this.storeTokens({ ...refreshed, refresh_token: refreshed.refresh_token ?? refreshToken });
  }

  private async storeTokens(tokens: GoogleTokens) {
    this.accessToken = tokens.access_token;
    const normalized = { ...tokens, expires_at: Date.now() + (tokens.expires_in ?? 3600) * 1000 };
    const encrypted = await encryptSecureRecord(normalized);
    await invoke('save_secure_record', { record: JSON.stringify(encrypted) });
  }

  private randomString(length: number) { const bytes = crypto.getRandomValues(new Uint8Array(length)); return btoa(String.fromCharCode(...bytes)).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''); }

  private async pkceChallenge(verifier: string) { const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier)); return btoa(String.fromCharCode(...new Uint8Array(digest))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, ''); }

  private async waitForCallback(): Promise<string> {
    for (let attempt = 0; attempt < 120; attempt += 1) {
      const callback = await invoke<string | null>('take_oauth_callback');
      if (callback) return callback;
      await new Promise((resolve) => window.setTimeout(resolve, 1000));
    }
    throw new Error('Tempo limite aguardando autorização Google.');
  }

  private async fetchLibrary(accessToken: string): Promise<GoogleSyncResult> {
    const playlistsResponse = await fetch('https://www.googleapis.com/youtube/v3/playlists?part=snippet,contentDetails&mine=true&maxResults=50', { headers: { Authorization: `Bearer ${accessToken}` } });
    if (!playlistsResponse.ok) throw new Error('A conta foi autorizada, mas a biblioteca não pôde ser carregada.');
    const playlistsData = await playlistsResponse.json() as { items?: Array<{ id: string; snippet: { title: string }; contentDetails: { itemCount: number } }> };
    const playlists: Playlist[] = [];
    const tracks: LibraryItem[] = [];
    for (const playlist of playlistsData.items ?? []) {
      const itemsResponse = await fetch(`https://www.googleapis.com/youtube/v3/playlistItems?part=snippet,contentDetails&playlistId=${playlist.id}&maxResults=50`, { headers: { Authorization: `Bearer ${accessToken}` } });
      const itemsData = await itemsResponse.json() as { items?: Array<{ contentDetails: { videoId: string }; snippet: { title: string; videoOwnerChannelTitle?: string } }> };
      const playlistTracks = (itemsData.items ?? []).map((item, index): LibraryItem => ({ id: `youtube-${item.contentDetails.videoId}`, youtubeId: item.contentDetails.videoId, title: item.snippet.title, artist: item.snippet.videoOwnerChannelTitle ?? 'YouTube', album: playlist.snippet.title, duration: '--:--', source: 'youtube', accent: ['#ff5f55', '#7267f0', '#e6a85c'][index % 3] }));
      tracks.push(...playlistTracks);
      playlists.push({ id: `youtube-playlist-${playlist.id}`, name: playlist.snippet.title, source: 'online', mode: 'online', tracks: playlistTracks, allowLocal: false });
    }
    return { playlists, tracks };
  }

  private async invokeBridge<T>(command: string, args: Record<string, unknown> | undefined, validate: (payload: unknown) => T): Promise<T> {
    try {
      const payload = args === undefined ? await invoke<unknown>(command) : await invoke<unknown>(command, args);
      return validate(payload);
    } catch (error) {
      throw new Error(bridgeErrorMessage(error, command), { cause: error });
    }
  }

  private validateArrayPayload<T>(payload: unknown, resultName: string): T[] {
    if (!Array.isArray(payload) || payload.some((item) => !item || typeof item !== 'object')) throw new Error(`O bridge retornou ${resultName} inválidos.`);
    return payload as T[];
  }

  private validatePlaylistPage(payload: unknown, resultName: string): YouTubePlaylistPage {
    if (!payload || typeof payload !== 'object') throw new Error(`O bridge retornou ${resultName} inválidos.`);
    const page = payload as { tracks?: unknown; hasMore?: unknown; continuation?: unknown };
    return {
      items: this.validateArrayPayload<LibraryItem>(page.tracks, resultName),
      hasMore: typeof page.hasMore === 'boolean' ? page.hasMore : true,
      ...(typeof page.continuation === 'string' ? { continuation: page.continuation } : {}),
    };
  }

  private validateAudioStream(payload: unknown): YouTubeAudioStream {
    if (!payload || typeof payload !== 'object') throw new Error('O bridge retornou um stream inválido.');
    const stream = payload as Partial<YouTubeAudioStream>;
    if (typeof stream.url !== 'string' || !/^https?:\/\/\S+$/i.test(stream.url)) throw new Error('O bridge retornou uma URL de stream inválida.');
    if (typeof stream.mimeType !== 'string' || !stream.mimeType.trim()) throw new Error('O bridge retornou um tipo de mídia inválido.');
    if (stream.bitrate !== undefined && (typeof stream.bitrate !== 'number' || !Number.isFinite(stream.bitrate))) throw new Error('O bridge retornou um bitrate inválido.');
    for (const value of [stream.expiresInSeconds, stream.durationSeconds]) {
      if (value !== undefined && typeof value !== 'string' && (typeof value !== 'number' || !Number.isFinite(value))) throw new Error('O bridge retornou metadados de duração inválidos.');
    }
    return stream as YouTubeAudioStream;
  }

  private throwIfAborted(signal?: AbortSignal) {
    if (signal?.aborted) throw new DOMException('A operação do player foi cancelada.', 'AbortError');
  }
}

export const googleSyncService = new GoogleSyncService();
