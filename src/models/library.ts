export type LibrarySource = 'youtube' | 'local';
export type PlaylistMode = 'local' | 'online' | 'mixed';

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
}

export interface Playlist {
  id: string;
  name: string;
  source: LibrarySource | 'mixed' | 'online';
  tracks: LibraryItem[];
  mode: PlaylistMode;
  allowLocal?: boolean;
}
