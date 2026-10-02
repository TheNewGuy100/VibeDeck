import type { LibraryItem } from '../models/library';
import type { RepeatMode } from '../interfaces/player';
export type { RepeatMode } from '../interfaces/player';

export interface PersistedPlayerState {
  track: LibraryItem;
  playlistId: string;
  isPlaying: boolean;
  usingNativeAudio: boolean;
}

export interface QueueContext<T = LibraryItem> {
  playlistId: string;
  source: string;
  generation: number;
  index: number;
  tracks: T[];
  shuffled: boolean;
}

export interface PlayerRequest {
  generation: number;
  playlistId: string;
  trackId: string;
}

export interface QueueMoveIntent {
  fromIndex: number;
  toIndex: number;
  activationSequence: number;
}

export function createQueueMoveIntent(index: number, length: number, direction: 1 | -1, repeatMode: RepeatMode, currentActivationSequence: number): QueueMoveIntent | null {
  const toIndex = manualQueueIndex(index, length, direction, repeatMode);
  return toIndex === null ? null : { fromIndex: index, toIndex, activationSequence: currentActivationSequence + 1 };
}

export function createSelectedTrackQueue<T>(tracks: T[], selectedIndex: number, repeatMode: RepeatMode): { tracks: T[]; index: number } {
  if (selectedIndex < 0 || selectedIndex >= tracks.length) return { tracks: [...tracks], index: -1 };
  // Without playlist repeat, retain only the selected track and its source-order tail.
  // Repeat-one is handled at each track boundary, not by wrapping this queue.
  if (repeatMode !== 'playlist') return { tracks: tracks.slice(selectedIndex), index: 0 };
  return {
    tracks: [...tracks.slice(selectedIndex), ...tracks.slice(0, selectedIndex)],
    index: 0,
  };
}

export function queueForRepeatTransition<T extends { id: string }>(queue: T[], currentIndex: number, previousMode: RepeatMode, nextMode: RepeatMode, sourceTracks: T[], currentTrackId: string): { tracks: T[]; index: number } {
  if (previousMode === nextMode) return { tracks: [...queue], index: currentIndex };
  if (nextMode === 'playlist' || previousMode === 'playlist') {
    const sourceIndex = sourceTracks.findIndex((track) => track.id === currentTrackId);
    if (sourceIndex >= 0) return createSelectedTrackQueue(sourceTracks, sourceIndex, nextMode);
  }
  return { tracks: [...queue], index: currentIndex };
}

export function nextQueueIndex(index: number, length: number, repeatMode: RepeatMode): number | null {
  if (length <= 0 || index < 0 || index >= length) return null;
  if (repeatMode === 'track') return index;
  if (index + 1 < length) return index + 1;
  return repeatMode === 'playlist' ? 0 : null;
}

export function queueIndexForTrack<T extends { id: string }>(queue: T[], preferredIndex: number, trackId: string): number {
  if (preferredIndex >= 0 && preferredIndex < queue.length && queue[preferredIndex].id === trackId) return preferredIndex;
  return queue.findIndex((track) => track.id === trackId);
}

export function manualQueueIndex(index: number, length: number, direction: 1 | -1, repeatMode: RepeatMode): number | null {
  if (length <= 0 || index < 0 || index >= length) return null;
  const candidate = index + direction;
  if (candidate >= 0 && candidate < length) return candidate;
  if (repeatMode !== 'playlist') return null;
  return direction > 0 ? 0 : length - 1;
}

export function nextRepeatMode(repeatMode: RepeatMode): RepeatMode {
  return repeatMode === 'off' ? 'playlist' : repeatMode === 'playlist' ? 'track' : 'off';
}

export function createQueueContext<T>(playlistId: string, tracks: T[], generation: number, index = -1, source = 'local', shuffled = false): QueueContext<T> {
  return { playlistId, source, generation, index, tracks: [...tracks], shuffled };
}

export function shuffleQueueContext<T>(context: QueueContext<T>, random = Math.random): QueueContext<T> {
  const current = context.index >= 0 ? context.tracks[context.index] : undefined;
  const remaining = context.tracks.filter((_, index) => index !== context.index);
  const tracks = current ? [current, ...shuffleTracks(remaining, random)] : shuffleTracks(remaining, random);
  return { ...context, tracks, index: current ? 0 : -1, shuffled: true };
}

export function advanceQueueContext<T>(context: QueueContext<T>, direction: 1 | -1): QueueContext<T> {
  if (!context.tracks.length || context.index < 0) return context;
  return { ...context, index: (context.index + direction + context.tracks.length) % context.tracks.length };
}

export function isCurrentPlayerRequest(request: PlayerRequest, current: PlayerRequest | null) {
  return Boolean(current)
    && request.generation === current?.generation
    && request.playlistId === current.playlistId
    && request.trackId === current.trackId;
}

export function sameTrackByTitleAndArtist(left: Pick<LibraryItem, 'title' | 'artist'>, right: Pick<LibraryItem, 'title' | 'artist'>) {
  return left.title.trim().toLocaleLowerCase() === right.title.trim().toLocaleLowerCase()
    && left.artist.trim().toLocaleLowerCase() === right.artist.trim().toLocaleLowerCase();
}

export function shuffleTracks<T>(tracks: T[], random = Math.random) {
  const shuffled = [...tracks];
  for (let index = shuffled.length - 1; index > 0; index -= 1) {
    const randomIndex = Math.floor(random() * (index + 1));
    [shuffled[index], shuffled[randomIndex]] = [shuffled[randomIndex], shuffled[index]];
  }
  return shuffled;
}

export function parsePersistedPlayerState(value: string | null): PersistedPlayerState | null {
  if (!value) return null;
  try {
    const parsed = JSON.parse(value) as PersistedPlayerState;
    if (!parsed.track?.title || !parsed.track?.artist || !parsed.playlistId) return null;
    return parsed;
  } catch {
    return null;
  }
}
