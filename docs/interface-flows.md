# Interface Flow Registry

Last reviewed: 2026-09-25

## Playlist Full Load

```text
open playlist
  -> set selected playlist
  -> loading state
   -> initial WebView/API page (held in loading state)
  -> continuation pages
  -> deduplicate by track id
  -> terminal empty/repeated page
   -> publish complete/partial track list only after terminal or error decision
```

Source of truth: the native/WebView pagination operation owns continuation state; Vue owns presentation state only.

Required terminal states: complete, empty, retryable failure, cancelled, stale request.

## Native Player Reload

```text
Rust audio engine continues
  -> frontend reloads
  -> subscribe to `player-state`
  -> invoke `get_player_snapshot` (idempotent, no playback intent)
  -> reject snapshots with revision <= last applied revision
  -> restore track/queue context/position/controls
  -> publish connected or explicit disconnected state and retry
```

The Rust `AudioEngine` owns `PlayerSnapshot`. Playback commands receive the complete track and queue activation context, while frontend commands remain intent-based. The snapshot contains the complete queue copy plus playlist id, generation, index, source, and active track metadata, so refresh does not require a frontend refetch before controls work. Native playback is never started by hydration. `revision` changes on replacement, pause, volume, seek, stop, and end; duplicate hydration and late bridge responses are ignored.

## Known Risks

- Startup can receive a snapshot before playlist metadata is loaded; the queue context is retained and reconciled when the playlist arrives.
- Native audio failures are logged but are not consistently delivered as typed UI events.
- Login and callback polling do not support cancellation on WebView close/unmount.
- Pagination needs continuation identity, page limits, cancellation, and stale-response protection.

## Required Tests

- Startup and reload while native audio is playing.
- Reload while paused.
- Playlist initial page, continuation pages, duplicate page, empty terminal page, and failure.
- Two playlist opens in quick succession with out-of-order responses.
- Two play requests in quick succession with the first resolving last.
- WebView close, reconnect, timeout, and retry.
- Native audio ended and failed events.

## Player Bridge Contract

`get_player_snapshot` returns `PlayerSnapshot` with `track`, `queue`, `playlistId`, `queueGeneration`, `queueIndex`, `isPlaying`, `position`, `duration`, `volume`, `muted`, `source`, `nativeMode`, `revision`, and `error`. The `player-state` event carries `{ kind, snapshot }`, where `kind` is `stateChanged`, `trackReplaced`, `ended`, or `error`.

The frontend owns one in-flight hydration promise, one event listener, and one reconnect timer. A bridge failure is represented as `disconnected`; it does not restore stale localStorage playback or issue a duplicate `play` command.
# Biblioteca local

O catálogo nativo em `appLocalDataDir` é a fonte de verdade. No startup, o frontend chama `load_local_library`; respostas antigas são descartadas por geração. O drag-and-drop Tauri escuta `tauri://drag-drop` e envia apenas caminhos nativos para `import_local_paths`, enquanto o navegador mantém o fallback transitório de `File`/object URL sem persistência. Faixas persistidas nunca usam `File` ou object URL no player nativo.

Cada importação possui `operationId`. O evento `local-import-progress` e o retorno final só alteram a UI quando pertencem à operação e geração atuais. Cancelamento invalida a geração antes de solicitar `cancel_local_import`, impedindo que um retorno tardio reanime o estado.

O comando `play_local_audio` recebe `trackId` e `managedPath` como referência, consulta o catálogo em Rust, exige que ambos correspondam, canonicaliza o arquivo sob `appLocalDataDir/library` e rejeita caminhos fora da raiz, arquivos ausentes, catálogo corrompido e containers não suportados com erros tipados.

## Native Local Import Boundary

```text
Tauri drag/drop payload
  -> normalize string/string[]/{ paths }/{ payload: { paths } }
  -> import_local_paths

  -> string|string[]|null
  -> empty selection = cancelled/no-op
  -> import_local_paths
```

The Tauri dialog plugin is enabled only for the desktop `main` window. Folder selection uses the native directory picker with recursive enumeration owned by Rust. Browser mode keeps the existing `File` fallback transient in memory; object URLs are revoked when the app unmounts and are never sent to or persisted by the native catalog.

Runtime limitation: Tauri drag/drop and native dialog behavior still require a packaged/live Tauri desktop smoke test. WebView versions may expose different drag payload shapes, so an event with no usable path is surfaced as a typed UI error instead of silently falling back to browser `File` objects.
# Playlist catalog hydration [Workload: IF-20250925-LIBRARY-03]

The native playlist catalog is the source of truth for durable YouTube playlist metadata and loaded track pages. It is stored at the Tauri `app_data_dir` as `playlist-catalog.json`; writes use `playlist-catalog.json.tmp` and `playlist-catalog.json.bak` before replacing the primary file.

Startup flow:

1. The WebView invokes `load_playlist_catalog` after mounting.
2. Rust returns the versioned snapshot, or the last valid backup when the primary file is corrupt.
3. The WebView reconciles cached tracks with fresh YouTube playlist metadata after session restoration.
4. A playlist request carries the current UI generation. Rust ignores older generations and clears the track set for newer generations.
5. `save_youtube_playlist_page` merges pages by track id, records terminal state and continuation context, and never stores `audioUrl`, browser object URLs, stream URLs, tokens, or cookies.

The native catalog does not own the current playback queue or live stream. Queue activation still uses the existing playlist generation checks in the WebView and AudioEngine bridge. If a refresh occurs while a continuation request is active, the old response can be persisted only when its generation is not stale; the refreshed WebView rehydrates the latest valid pages on demand.

Runtime limitation: no Tauri/WebView restart smoke test was available in this environment. A manual test should load a multi-page playlist, refresh while the next page is pending, corrupt the primary catalog file, and verify backup recovery and queue isolation.
