# Windows Process Identity

VibeDeck's supported Windows identity is configured through Tauri and Cargo:

- `productName`, bundle descriptions, and the `com.vibedeck.desktop` identifier are defined in `src-tauri/tauri.conf.json`.
- The main window has the explicit Tauri label `main` and the title `VibeDeck`.
- Auxiliary WebViews use stable labels and VibeDeck-prefixed native titles: `youtube-auth`, `youtube-library-bridge`, and `youtube-stream-bridge`.
- No sidecar or external binary is bundled. The Cargo package and application binary remain `vibedeck` so Tauri's generated entry point and existing build integration are not changed.

These settings improve the app/window and packaged executable metadata shown by Windows where those fields are supported. They do not rename the child processes created by the WebView2 runtime. Windows/WebView2 controls those process names and may continue to show entries such as `msedgewebview2.exe`; the Tauri labels and window titles identify the owning WebViews in application diagnostics, but cannot safely override the runtime's process executable name.

Validation should include a packaged Windows run and a Task Manager/process-tree check. A frontend-only Vite run cannot validate native window titles or WebView2 process grouping.
