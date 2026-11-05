export type ArtworkDirection = 'center' | 'up' | 'up-right' | 'right' | 'down-right' | 'down' | 'down-left' | 'left' | 'up-left';

export interface NormalizedPointer {
  x: number;
  y: number;
}

export function normalizeArtworkPointer(x: number, y: number): NormalizedPointer {
  return { x: Math.max(-1, Math.min(1, Number.isFinite(x) ? x : 0)), y: Math.max(-1, Math.min(1, Number.isFinite(y) ? y : 0)) };
}

export function artworkPointerFromClient(clientX: number, clientY: number, rect: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>): NormalizedPointer {
  if (rect.width <= 0 || rect.height <= 0) return { x: 0, y: 0 };
  return normalizeArtworkPointer(((clientX - rect.left) / rect.width - 0.5) * 2, ((clientY - rect.top) / rect.height - 0.5) * 2);
}

export function artworkDirection(x: number, y: number): ArtworkDirection {
  const point = normalizeArtworkPointer(x, y);
  if (Math.hypot(point.x, point.y) < 0.16) return 'center';
  const angle = Math.atan2(-point.y, point.x);
  const octant = Math.round(angle / (Math.PI / 4) + 8) % 8;
  return (['right', 'up-right', 'up', 'up-left', 'left', 'down-left', 'down', 'down-right'] as const)[octant];
}

export function artworkTilt(point: NormalizedPointer, reducedMotion: boolean): { rotateX: number; rotateY: number } {
  return reducedMotion ? { rotateX: 0, rotateY: 0 } : { rotateX: -point.y * 8, rotateY: point.x * 8 };
}
