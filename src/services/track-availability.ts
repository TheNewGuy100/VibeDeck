import type { LibraryItem, UnavailableReason } from '../models/library';

type MaybeUnavailable = Pick<LibraryItem, 'available' | 'unavailableReason'>;

/**
 * One rule for "listed but not playable" (signed out, file missing, ...). Unavailable tracks stay
 * visible, are left out of every queue, and cannot be started directly.
 */
export function isTrackUnavailable(track: MaybeUnavailable | null | undefined): boolean {
  return Boolean(track) && (track?.unavailableReason !== undefined || track?.available === false);
}

export function unavailableReasonOf(track: MaybeUnavailable): UnavailableReason | undefined {
  if (track.unavailableReason) return track.unavailableReason;
  return track.available === false ? 'file-missing' : undefined;
}

export function unavailableLabel(reason: UnavailableReason | undefined): string {
  return reason === 'signed-out' ? 'Conecte a conta' : reason === 'file-missing' ? 'Arquivo ausente' : 'Indisponível';
}

export function unavailableMessage(track: MaybeUnavailable & Pick<LibraryItem, 'title'>): string {
  const reason = unavailableReasonOf(track);
  if (reason === 'signed-out') return `“${track.title}” é do YouTube Music. Conecte sua conta para ouvir.`;
  if (reason === 'file-missing') return `O arquivo de “${track.title}” não está mais no app. Importe-o de novo.`;
  return `“${track.title}” está indisponível.`;
}

/** The tracks a queue may contain: unavailable ones are skipped, keeping their order. */
export function playableTracks<T extends MaybeUnavailable>(tracks: readonly T[]): T[] {
  return tracks.filter((track) => !isTrackUnavailable(track));
}

export function withUnavailable<T extends LibraryItem>(track: T, reason: UnavailableReason | undefined): T {
  if (!reason) {
    if (track.unavailableReason === undefined) return track;
    const { unavailableReason: _removed, ...rest } = track;
    return rest as T;
  }
  return { ...track, unavailableReason: reason };
}
