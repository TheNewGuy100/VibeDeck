# VibeDeck

VibeDeck is a personal desktop music client built with Tauri, Vue and Rust.
It unifies local music libraries with synchronized YouTube Music metadata in a
fast, keyboard-friendly interface.

## Current Features

- Local audio playback with queue controls.
- Local folders and files added through drag and drop.
- Playlists for local, online and mixed sources.
- Favorites playlist.
- Google OAuth desktop login with PKCE.
- YouTube playlist synchronization.
- Light and dark themes.
- Two-layer protection for sensitive records: AES-GCM and Windows DPAPI.

## Playback Status (2026-10-21)

The current online path relays successive SABR/UMP responses from a muted,
authenticated YouTube Music WebView to the Rust Symphonia/CPAL audio pipeline.
Audible multi-response playback, automatic next-track advancement and pause
were validated on 2026-10-20. Seek is not supported by this relay.

The goal is a native Rust SABR client with its own scheduling and seek, removing
playback brokers/WebViews. Its bootstrap and protocol helpers are experimental;
native SABR playback has not been validated. The relay remains a fallback.

These unofficial internal interfaces may change and require maintenance.
Google OAuth/YouTube Data API synchronization is separate from the internal
YouTube Music cookie-session integration.

## Project Knowledge

- [Documentation index](docs/README.md): architecture, playback, UI, decisions,
  active investigation and validation, with maintenance rules.
- [AGENTS.md](AGENTS.md): mandatory safety and development rules.
- [CLAUDE.md](CLAUDE.md): entry point for Claude and GitNexus workflow.

## Technology

- Tauri 2
- Vue 3
- TypeScript
- Rust
- Vite
- Symphonia decoding and CPAL native audio output

## Development

Requirements:

- Node.js 20 or newer
- Rust stable with the MSVC toolchain
- Visual Studio C++ Build Tools
- WebView2

Install dependencies and run the web interface:

```powershell
npm install
npm run dev
```

Run the desktop application:

```powershell
npm run tauri -- dev
```

Run the desktop application with the native YouTube playback path (no WebView broker for the
audio stream; falls back to the relay if it cannot start):

```powershell
npm run tauri -- dev --features native-range-playback
```

Create Windows installers:

```powershell
npm run tauri -- build
```

## Google Configuration

Create a local `.env` file based on `.env.example`:

```env
VITE_GOOGLE_CLIENT_ID=your-desktop-client-id
GOOGLE_CLIENT_SECRET=your-client-secret
```

The client secret must not use the `VITE_` prefix. It is consumed by the Rust
build layer and must never be exposed to the frontend bundle.

Enable YouTube Data API v3 in the Google Cloud project and add development
accounts as OAuth test users.

## Attribution

Protocol research includes the open-source project `ytmusicapi` by sigma67:

https://github.com/sigma67/ytmusicapi

`ytmusicapi` is distributed under the MIT License. If code from that project is
reused directly, its copyright and license notice must be preserved.

The `n` throttling-challenge work builds on the solver of
[`yt-dlp/ejs`](https://github.com/yt-dlp/ejs) (`yt-dlp-ejs`, Unlicense; it bundles `meriyah`, ISC,
and `astring`, MIT) and on the PO token research of [yt-dlp](https://github.com/yt-dlp/yt-dlp)
and [`LuanRT/BgUtils`](https://github.com/LuanRT/BgUtils). Thanks to their authors. Today the
solver runs only in debug builds, from a local copy outside the repository; if its bundles or
ported code are ever distributed, their notices must ship with the app.

SABR/UMP research also references `LuanRT/googlevideo` and
`gsuberland/UMP_Format`; pinned references and evidence limits are recorded in
[Playback reference](docs/playback.md). No Python runtime is bundled.

## Disclaimer

VibeDeck is a personal project and is not affiliated with Google or YouTube.
YouTube Music synchronization and playback depend on external services and
their applicable terms and policies.
