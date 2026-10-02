# Visual Components Registry

Last reviewed: 2026-09-24

## Components

| Component | Responsibility | Main states | Tests |
|---|---|---|---|
| `VisualizerCanvas` | Canvas visualizers, spectrogram history, hover data | active, paused, compact, tooltip, background | pending runtime Canvas tests |
| `VisualizerControls` | Shared general, Mascot and Serpents controls | collapsed, expanded, visualizer-specific groups, bounded inputs, overflow | `src/components/visualizer-controls.test.ts` |
| `FrequencyRangeControl` | Min/max frequency range | valid range, crossed handles, keyboard input | pending dedicated tests |
| `VisualizerPicker` | Visualizer selection in settings | selected, preview, responsive | pending settings interaction test |
| `PlaylistLoadingOverlay` | Blocks playlist rows during full loading | loading, completion, failure | `src/services/playlist-loader.test.ts` |
| `PlaylistShuffleButton` | Requests playlist shuffle | enabled, empty, single item | pending interaction test |

### Track Row Current State

- Responsibility: identifies the selected track without replacing its stable playlist index.
- Visual states: normal, current paused/selected, current playing, hover, focus-visible, and reduced-motion.
- Layout constraints: preserves the existing source badge, favorite action, duration, and responsive grid; the marker stays inside the row edge.
- Accessibility: current rows expose `aria-current="true"` and announce whether they are playing or paused; focus remains on the row action and favorite button.
- Verification: `npm test -- --run`, `npm run build`, and desktop/mobile browser snapshots.

## Rules

- Visualizer controls use explicit bounded numeric inputs and ranges.
- Serpents controls are grouped under Cobras and Iluminação in `VisualizerControls`; the canvas has no interactive overlay. Shared revisioned Rust preferences are the source of truth for every Serpents3D instance.
- Serpents control groups are rendered only while `kind="serpents"`; keyboard users can navigate native fieldsets, selects, ranges and buttons, and the general panel owns scrolling without obscuring the canvas itself.
- Background visualizers must be fixed and must not participate in playlist layout.
- Loading overlays must remain inside the owned content region and remain visible until the terminal response.
- Tooltips must not intercept pointer input and must escape lower stacking contexts when rendered over the player.
- Track rows must not use nested interactive semantics; the row action and child actions need separate keyboard behavior.
- Every modal/settings surface needs focus-visible styling, Escape handling, and dialog semantics.

## Current Gaps

- `App.vue` still owns most navigation, player, settings, playlist, and log markup.
- There is no dedicated visual component catalog with screenshots.
- Mobile is constrained by `body { min-width: 960px; }`.
- Settings dialog accessibility and focus management are incomplete.
- Playlist loading visibility is controlled by the load terminal state; Teleport owns only the transparent status overlay.

## Validation

- Automated: `npm test`.
- Static: `npm run build`.
- Runtime visual validation: required in Tauri for desktop, mobile-width layout, theme, drawer, loading, and overflow.
# Biblioteca local

O alvo de drop da playlist é acessível por teclado como botão e mantém estados visíveis de vazio, carregamento, erro e importação. Durante a importação, progresso, cancelamento e resumo por arquivo são anunciados com `role="status"`/`aria-live`; playlists online não exibem alvo de importação.

The local import surface also exposes a separate `Escolher pasta` action. It uses the native folder dialog in Tauri and the browser `webkitdirectory` input only outside Tauri. The action has visible focus styling and cancellation is silent; missing native paths are announced through the existing toast error surface.
