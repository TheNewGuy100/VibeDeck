import type { LibraryItem } from '../models/library';

export interface MusicSource {
  readonly id: 'youtube' | 'local';
  readonly label: string;
  isConnected(): boolean;
  search(query: string): Promise<LibraryItem[]>;
}

export class YouTubeMusicSource implements MusicSource {
  readonly id = 'youtube' as const;
  readonly label = 'YouTube Music';

  isConnected() {
    return false;
  }

  async search(_query: string): Promise<LibraryItem[]> {
    // OAuth and the remote catalog will be wired here without coupling the UI to the provider.
    return [];
  }
}

export const youtubeMusicSource = new YouTubeMusicSource();
