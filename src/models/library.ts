export type LibrarySource = 'youtube' | 'local';
export type PlaylistMode = 'local' | 'online' | 'mixed';
/** Why a track is listed but cannot be played right now. Absent means playable. */
export type UnavailableReason = 'signed-out' | 'file-missing';

export interface LibraryItem {
  id: string;
  title: string;
  artist: string;
  album: string;
  duration: string;
  source: LibrarySource;
  accent: string;
  coverUrl?: string;
  audioUrl?: string;
  fileName?: string;
  durationSeconds?: number;
  youtubeId?: string;
  managedPath?: string;
  /** Legacy local-file flag: `false` means the managed file is gone. Prefer `unavailableReason`. */
  available?: boolean;
  unavailableReason?: UnavailableReason;
}

export interface Playlist {
  id: string;
  name: string;
  source: LibrarySource | 'mixed' | 'online';
  tracks: LibraryItem[];
  mode: PlaylistMode;
  allowLocal?: boolean;
  continuation?: string | null;
  terminal?: boolean;
}
