import type { LibraryItem, LibrarySource, Playlist } from './library';

export type PlayerCommand =
  | { kind: 'play'; activation: NativePlayerActivation }
  | { kind: 'pause'; paused: boolean }
  | { kind: 'stop' }
  | { kind: 'seek'; request: NativeSeekRequest }
  | { kind: 'volume'; intent: VolumeIntent };

export type RepeatMode = 'off' | 'playlist' | 'track';

export type QueueTerminalBehavior = 'stop' | 'wrapToStart' | 'restartCurrent';

export interface QueueEndPolicy {
  repeatMode: RepeatMode;
  terminalBehavior: QueueTerminalBehavior;
  queueGeneration: number;
  queueIndex: number;
}

export function terminalBehaviorForRepeatMode(repeatMode: RepeatMode): QueueTerminalBehavior {
  return repeatMode === 'playlist' ? 'wrapToStart' : repeatMode === 'track' ? 'restartCurrent' : 'stop';
}

export interface RepeatModeRequest extends QueueContext {
  repeatMode: RepeatMode;
  terminalBehavior: QueueTerminalBehavior;
  trackId: string;
  baseRevision: number;
}

export interface QueueContext {
  playlistId: string;
  queueSource: Playlist['source'];
  queueGeneration: number;
  queueIndex: number;
  queue: LibraryItem[];
  shuffled: boolean;
  repeatMode?: RepeatMode;
  terminalBehavior?: QueueTerminalBehavior;
}

export interface NativePlayerActivation extends QueueContext {
  track: LibraryItem;
  activationSequence: number;
}

export interface PlayerQueueTrack {
  id: string;
  title: string;
  artist: string;
  cover: string;
  accent: string;
  source: string;
}

export interface PlayerQueuePanelProps {
  open: boolean;
  tracks: PlayerQueueTrack[];
  currentTrackId: string | null;
  currentTrackIndex: number;
  isPlaying: boolean;
}

export interface PlayerQueuePanelEvents {
  'update:open': boolean;
  'select-track': { queueIndex: number; trackId: string };
}

export interface PlayerSnapshot extends Omit<QueueContext, 'playlistId' | 'queueIndex'> {
  track: LibraryItem | null;
  playlistId: string | null;
  queueIndex: number;
  isPlaying: boolean;
  position: number;
  duration: number;
  volume: number;
  muted: boolean;
  volumeSequence: number;
  source: LibrarySource | null;
  nativeMode: boolean;
  revision: number;
  activationSequence: number;
  error: string | null;
  repeatMode: RepeatMode;
  terminalBehavior: QueueTerminalBehavior;
}

export type PlayerEventKind = 'stateChanged' | 'trackReplaced' | 'ended' | 'error';

export interface PlayerStateEvent {
  kind: PlayerEventKind;
  snapshot: PlayerSnapshot;
  end?: PlayerEndEvent;
}

export interface PlayerEndEvent {
  trackId: string;
  queueGeneration: number;
  queueIndex: number;
  revision: number;
  terminalBehavior: QueueTerminalBehavior;
}

export function isCurrentPlayerEndEvent(event: PlayerEndEvent, active: Pick<PlayerSnapshot, 'track' | 'queueGeneration' | 'queueIndex' | 'revision' | 'terminalBehavior'>): boolean {
  return event.trackId === active.track?.id
    && event.queueGeneration === active.queueGeneration
    && event.queueIndex === active.queueIndex
    && event.revision >= active.revision
    && event.terminalBehavior === active.terminalBehavior;
}

export interface SeekIntent {
  trackId: string;
  queueGeneration: number;
  seconds: number;
  baseRevision: number;
  sequence: number;
}

export type NativeSeekIntent = SeekIntent;

export interface NativeSeekRequest extends SeekIntent {
  revision: number;
}

export interface VolumeIntent {
  volume: number;
  muted: boolean;
  sequence: number;
}

export type PlayerConnection = 'connecting' | 'connected' | 'disconnected' | 'error';

export interface RehydrationState {
  snapshot: PlayerSnapshot | null;
  revision: number;
  connection: PlayerConnection;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return Boolean(value) && typeof value === 'object' && !Array.isArray(value);
}

export function parsePlayerSnapshot(value: unknown): PlayerSnapshot {
  if (!isRecord(value) || !Number.isFinite(value.revision)) {
    throw new Error('O bridge retornou um snapshot de player inválido.');
  }
  return value as unknown as PlayerSnapshot;
}

export function isPlayerStateEvent(value: unknown): value is PlayerStateEvent {
  return isRecord(value)
    && (value.kind === 'stateChanged' || value.kind === 'trackReplaced' || value.kind === 'ended' || value.kind === 'error')
    && isRecord(value.snapshot)
    && Number.isFinite(value.snapshot.revision);
}
