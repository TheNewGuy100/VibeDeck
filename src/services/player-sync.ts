import type { NativeSeekIntent, NativeSeekRequest, PlayerConnection, PlayerSnapshot, RehydrationState, VolumeIntent } from '../interfaces/player';

export type { NativeSeekIntent, NativeSeekRequest, PlayerConnection, PlayerSnapshot, RehydrationState, VolumeIntent } from '../interfaces/player';

export function acceptsPlayerSnapshot(snapshot: PlayerSnapshot, currentRevision: number) {
  return Number.isFinite(snapshot.revision) && snapshot.revision > currentRevision;
}

export function applyPlayerSnapshot(state: RehydrationState, snapshot: PlayerSnapshot): RehydrationState {
  if (state.snapshot && snapshot.queueGeneration < state.snapshot.queueGeneration) return state;
  if (state.snapshot
    && snapshot.queueGeneration === state.snapshot.queueGeneration
    && snapshot.playlistId === state.snapshot.playlistId
    && snapshot.queue.length < state.snapshot.queue.length) return state;
  const reconnectingAtSameRevision = state.connection !== 'connected' && snapshot.revision === state.revision;
  if (!reconnectingAtSameRevision && !acceptsPlayerSnapshot(snapshot, state.revision)) return state;
  return { snapshot, revision: snapshot.revision, connection: snapshot.error ? 'error' : 'connected' };
}

export function isSnapshotOlderThanQueueIntent(snapshot: PlayerSnapshot, queueGeneration: number, activationPending: boolean, activationSequence = 0) {
  return activationPending && (snapshot.queueGeneration < queueGeneration || (snapshot.activationSequence ?? 0) < activationSequence);
}

export function reconcilePlayerSnapshot(state: RehydrationState, snapshot: PlayerSnapshot, queueGeneration: number, activationPending: boolean, activationSequence = 0): RehydrationState {
  if (isSnapshotOlderThanQueueIntent(snapshot, queueGeneration, activationPending, activationSequence)) return state;
  return applyPlayerSnapshot(state, snapshot);
}

export function snapshotHasQueueContext(snapshot: PlayerSnapshot) {
  return Boolean(snapshot.playlistId && snapshot.queueGeneration > 0 && snapshot.queue.length);
}

export function seekSnapshotMatchesIntent(snapshot: PlayerSnapshot, intent: NativeSeekIntent) {
  return snapshot.nativeMode
    && snapshot.track?.id === intent.trackId
    && snapshot.queueGeneration === intent.queueGeneration
    && snapshot.revision > intent.baseRevision;
}

export function volumeSnapshotMatchesIntent(snapshot: PlayerSnapshot, intent: VolumeIntent) {
  return Number.isFinite(snapshot.volumeSequence) && snapshot.volumeSequence >= intent.sequence;
}

export function isCurrentSeekIntent(intent: NativeSeekIntent, latest: NativeSeekIntent | null) {
  return latest?.sequence === intent.sequence
    && latest.trackId === intent.trackId
    && latest.queueGeneration === intent.queueGeneration;
}

export function disconnectedPlayerState(error: string, revision = 0): RehydrationState {
  return { snapshot: null, revision, connection: 'disconnected' };
}
