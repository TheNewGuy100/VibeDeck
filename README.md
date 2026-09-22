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

## Planned Playback Layer

The online playback layer will be implemented as a small Rust client inspired
by the behavior documented by `ytmusicapi`. The goal is to avoid shipping a
Python runtime or depending on the library directly while keeping the online
catalog and local player behind the same source interface.

The online implementation is unofficial and depends on internal YouTube Music
behavior. Stream URLs may expire, and upstream changes can require maintenance.
The YouTube Data API remains responsible for official account synchronization.

## Technology

- Tauri 2
- Vue 3
- TypeScript
- Rust
- Vite
- Web Audio and HTML audio playback for local files

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
npm run tauri dev
```

Create Windows installers:

```powershell
npm run tauri build
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

The planned unofficial YouTube Music protocol client is technically inspired
by the open-source project `ytmusicapi` by sigma67:

https://github.com/sigma67/ytmusicapi

`ytmusicapi` is distributed under the MIT License. If code from that project is
reused directly, its copyright and license notice must be preserved.

## Disclaimer

VibeDeck is a personal project and is not affiliated with Google or YouTube.
YouTube Music synchronization and playback depend on external services and
their applicable terms and policies.
