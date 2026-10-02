# Visualizer Contract

Last reviewed: 2026-09-25

## Analyzer

- Source: native Rust CPAL output callback.
- FFT: `rustfft`, 2048 samples, 1024-sample hop.
- Published bands: 32 normalized values.
- Native analyzer update: one spectrum is published per 1024-sample hop, about 21.3 ms at 48 kHz or 23.2 ms at 44.1 kHz.
- UI bridge polling: 33 ms, with one pending `get_audio_levels` request maximum; rendering remains on `requestAnimationFrame`.
- Current UI spectrogram cadence: configurable, default 20 ms.
- Current UI history target: 10 seconds.
- Render target: requestAnimationFrame at 60 FPS, with a 16.67 ms frame budget. Audio acquisition remains on the independent 33 ms bridge cadence.
- Canvas DPR is capped at 1.5; Serpents3D defaults to resolution 1.25 and caps its renderer DPR at 1.25.
- Current tooltip frequency model: estimated 20 Hz to 24 kHz logarithmic bands.

## Visualizer Rules

- Audio acquisition must remain independent from Canvas rendering.
- Playback modes are explicit: `active` renders audio-reactive motion, while `paused` and `idle` (no track) share the same subtle ambient motion without changing analyzer levels.
- Ambient motion is disabled by `prefers-reduced-motion: reduce`; paused and no-track visualizers then render once and stop.
- Paused and no-track audio must clear levels, clear prior spectrogram columns, and never append synthetic history.
- History must be bounded and must not be copied on every render frame.
- Capture cadence changes must not rewrite timestamps of existing columns.
- Idle spectrogram motion is a scan-only background effect; it never appends synthetic history columns or intensity samples.
- Actual sample rate and band boundaries should be passed from the analyzer instead of inferred in the UI.
- Background visualizers must not affect playlist layout.
- Bars use one gradient per bar geometry: the body stays in the green app palette through 50% height, then the top transitions smoothly toward the red peak palette from 51% to 100%; a full bar reaches red only at its top, never as a solid overlay.
- Tooltip coordinates must be clamped to the viewport.
- Interactive Canvas output needs an accessible wrapper or keyboard equivalent; `aria-hidden` is not sufficient for inspected spectrogram data.

## Current Risks

- FFT work and allocations happen in the CPAL callback path.
- The decoder waits for the complete download before decoding, delaying analyzer output.
- Analyzer snapshots cross the Tauri bridge by polling, so bridge scheduling and WebView load can still add latency.
- `smoothing` is exposed but is not applied consistently.
- Analyzer globals are not reset per track.
- Spectrogram rendering still performs one draw call per visible cell; history is bounded to 10 seconds and capture is paused when inactive.

## Performance Validation

- Canvas render loops use `requestAnimationFrame`, cache the 2D context and quantized per-bar gradients, reuse serpent point buffers, and stop scheduling frames when inactive.
- Ambient loops are scheduled only while the visualizer is mounted and visible; hidden Canvas/Three.js instances render once and stop scheduling frames.
- Serpents3D records last, average, maximum frame time and the 16.67 ms budget in `getStats()` for runtime inspection.
- No representative Tauri audio runtime was available here, so sustained FPS on a real GPU was not measured. The expected budget is bounded by the browser compositor and may degrade gracefully on integrated GPUs or large high-DPR windows.

## Required Tests

- Active and paused capture cadence.
- Ten-second bounded history.
- Intensity min/max mapping.
- Tooltip time/frequency mapping and viewport clamping.
- Switching visualizers and clearing transient state.
- Background mode layout isolation.
