import type { LibraryItem } from '../models/library';

export interface PersistedPlayerState {
  track: LibraryItem;
  playlistId: string;
  isPlaying: boolean;
  usingNativeAudio: boolean;
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
