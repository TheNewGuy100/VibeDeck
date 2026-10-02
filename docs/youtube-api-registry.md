# YouTube Music API Registry

Last reviewed: 2026-10-01

These are internal, unstable interfaces. They are not public API contracts. Do not store cookies, tokens, signatures, personal identifiers, or complete media URLs in this document.

## `youtubei/v1/search`

- Method: POST.
- Client: YouTube Music `WEB_REMIX` context.
- Input: search query and Innertube context extracted from the authenticated WebView.
- Output: recursive music/search renderers normalized into `LibraryItem`.
- Current limit: 25 results.
- Risks: client version, renderer paths, auth headers, and response shape can change.
- Tests: bridge command smoke coverage only; sanitized fixtures missing.

## `youtubei/v1/player`

- Method: POST.
- Input: video ID and client context.
- Output: `playabilityStatus`, `streamingData`, and adaptive audio formats.
- Observed audio formats: AAC/MP4 and Opus/WebM.
- Known fields: `url`, `signatureCipher`, `s`, `sp`, `n`, `pot`, `expiresInSeconds`.
- Current implementation limitation: direct resolver uses formats with `url`; signature decipher, `n`, `pot`, expiry renewal, and complete `PlayerResponse` models are not implemented.
- Tests: no sanitized PlayerResponse fixtures.

## Playlist Browse and Continuation

- Internal browse payloads contain `musicPlaylistShelfRenderer`, continuation renderers, or `appendContinuationItemsAction`.
- Initial and continuation data are captured through authenticated WebViews and bridge side channels.
- Playlist commands normalize responses to `{ tracks, hasMore, continuation }`; an embedded response without a continuation is terminal and does not enter the bridge continuation path.
- Frontend deduplicates pages by track ID, awaits each catalog save, and stops on an empty, repeated, duplicate, or provider-terminal page.
- Rust persists the latest continuation with the merged catalog and reloads it when the WebView process has been recreated; a repeated/invalid cursor is terminal rather than retried forever.
- The catalog is the durable source of truth for hydration and queue creation; no 100-track application cap is applied.
- Frontend pagination owns one in-flight operation per playlist generation; scroll/overflow requests during the initial page await that operation instead of being discarded by `initialLoading`. Each returned page is merged, persisted before advancing, and the terminal response clears loading and the cursor.
- Required hardening: continuation request ID, playlist identity, timeout, cancellation, stale-response rejection, and terminal error state. Do not impose a fixed page-count cap; repeated/empty/duplicate cursor safeguards are the terminal protections.
- Tests: generic paginator tests cover terminal single-page, duplicate-page, partial-error, continuation during the initial 100-track load, 250-track multi-page responses, and more than 1000 continuation pages; Rust catalog tests cover 250-track save/load and generation isolation. WebView continuation fixtures are still missing.
- 2026-10-01 code audit confirmed the TypeScript bridge sends `{ request }` with camelCase fields matching Rust `#[serde(rename_all = "camelCase")]`; Rust catalog merges by playlist generation and track ID. Tauri CDP runtime interaction was attempted via `npm run tauri:debug`, but the debug endpoint did not remain available after the command timeout, so current authenticated WebView fetch/save/reload/DOM counts remain unverified.
- 2026-10-01 native reproduction on authenticated `Just The Best`: the bridge logged `YouTube direct continuation returned 100 tracks`, but the Tauri invoke never resolved. Root cause was a non-reentrant mutex deadlock: the continuation command held `YOUTUBE_PLAYLIST_CONTINUATIONS` while calling `playlist_continuation_state`, which attempted to lock the same mutex. The lock is now dropped before cursor snapshot. App initial fetch + continuation now share one per-playlist-generation pagination owner, and persistence errors reject the operation rather than being silently ignored.
- 2026-10-01 Tauri CDP validation after restart: `Just The Best` displayed 529 visible rows with loading off, including after WebView reload from the persisted catalog. A live paginated playlist (`Safeplace`) returned 12 continuation pages, merged to 1257 rows, logged provider terminal, displayed 1257 visible rows with loading off, and reloaded with 1257 rows. Initial catalog hydration now seeds the frontend generation counter from persisted Rust generations so a WebView restart does not send stale save generations.

## WebView Bridge

- WebViews: auth, library, and stream bridge.
- Current transport: global WebView state and `document.title` chunk transport.
- Risks: stale title chunks, concurrent requests, navigation races, and weak correlation.
- Required contract: request ID, operation, stage, payload schema, timeout, cancellation, cleanup, and redaction policy.

## Validation Gaps

- No sanitized fixture catalog.
- No dated parser contract report.
- No real runtime smoke test for login, playlist pagination, stream resolution, or reload while playing.
- Official YouTube Data API pagination also needs `nextPageToken` coverage.
