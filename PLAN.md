# Endel audio reproduction — one-page plan (tracer bullet)

**Goal:** reproduce the music surface of `app.endel.io` headless-first (study → build → check → refine), without ever crashing or locking up. This slice proves the observation loop only: **one scenario (Focus), one capture → fingerprint → offline-render → compare.** Full rebuild / all scenarios / generation are later items.

## What must be identical (user-visible)

Play/pause, scenario switch (Focus only here), mix-layer enable + gain/pan sliders, timer read-out, and a **live, generated continuous stream** — never a loop file. The stream is what `OfflineAudioContext` must render for 30–60 s from the same generative recipe. **Dynamic mixing is required behaviour, not out of scope:** the mix is driven by UI interaction (slider → layer gain/pan param → audible envelope change), so any rebuild must reproduce the causal mapping (control → param → curve), not just one static snapshot. The tracer bullet covers its static-snapshot proof; the envelope-mapping proof is **phase 2** (still required, next pending change). Exact note equality is **out of scope** (Endel is stochastic); perceptual likeness is the bar. All scenarios, account/login flows beyond attached-session capture, and `ogg/wav/mp3/flac/opus` asset extraction are **out of scope** (Endel has no such files; game tools' extractors do not apply to a live WebAudio page).

## Reuse (read-only study already proved the gap)

* `web_site_capture` — screenshots / DOM / layout / design tokens only. No audio.
* `web_pixels_compare` — per-pixel PNG diff. Template for the pure-compute audio compare.
* `web_page_show` — passive scripts/network/websocket/storage-key-name outlines (value-free). Supplies the session/network facet and the data-minimisation rules.
* `web_walkthrough_record` + `browser_tools::StartingStorage` — seeded cookies/storage as secrets, redacted in every encoding. Same handling covers the Endel session token: **never read cookies/storage values, only names + redacted outlines** (Deep-only `browser="attached"` on the ego browser, e.g. `http://127.0.0.1:9223`).
* `assets_image_generate` / `assets_model_generate` — pictures / 3D only. No music path exists today.
* `WebCommon::Browser::resolve` (`web_common.rs`) — attach-or-launch over loopback CDP via `platform::process` (deadline, cancel, capped output). Offline render reuses it headless.

## New tools (all `Toolset::Browser`, MCP id = CLI kebab)

### 1. `web_audio_capture`
Deep-only study. Enables `Runtime` + `Media` CDP domains; `Runtime.evaluate` walks the live `AudioContext` graph (constructor wrap is foreign code) and reads an `AnalyserNode` spectrum over `duration_ms`, plus the page's mixer-state object (scenario id, layer gains, tempo/scale shape). Returns `Evidence` whose facets are: `graph` (node types/counts + edges), `spectrum` (averaged FFT bins), `mixer_state`, `network_outline` (from `web_page_show`-style Network enable), and `secret_redactions` count. Pure `web_site_capture` behaviour stays Clean-room; this tool **is refused in Clean room** with the standard Deep-mode remediation. **Phase 2 adds optional interaction-tracing** (same tool, no new module): scripted slider steps reuse the walkthrough executor while `Runtime.evaluate` polls `(layer_id → gain, pan)` at ~10 Hz, yielding a `mixer_envelope: [{t_ms, layer_id, gain, pan}]` time-series — existing walkthroughs alone cannot prove this, only that the UI changed.
*Input:* `cdp_endpoint?`, `target_id?`, `duration_ms` (5_000..=120_000, default 30_000), `include_graph=true`, `interaction_trace=false` (phase 2: `steps:[{selector, to}]`), `timeout_secs`.
*Effects:* `EVIDENCE ∪ EXECUTES_FOREIGN_CODE ∪ ACCESSES_NETWORK ∪ LAUNCHES_PROCESS` (consent-gated, mirrors `web_site_open` / `web_walkthrough_record`).
*Requires:* `Nothing` (engine resolved via `Browser::resolve`; `EngineMissing` → install Chromium or give a loopback endpoint).

### 2. `web_audio_fingerprint`
Pure compute, no browser. Turns a capture's `spectrum` + `rms` envelope into a typed fingerprint: log-frequency bins (quantised), RMS envelope (1 Hz), `graph_signature{node_counts, edges}`, and the mixer seed shape. Phase 2 also fingerprints `mixer_envelope` as `(layer_id, gain/pan trajectory, step→param delta)` so a slider move is comparable across captures. Returns `ToolOutput::Evidence` with `Confidence::Derived` and explicit limitations (analyser resolution, stochastic variance).
*Input:* `capture_evidence_id` (or inline `{spectrum, rms, graph, mixer_state, mixer_envelope?}`), `bins` (default 48).
*Effects:* `EVIDENCE`. *Requires:* `Nothing`. **List in `clean_room.rs::TOOLS`** (pure evidence like `web_pixels_compare`).

### 3. `web_audio_compare`
Pure compute. Compares two fingerprint blobs (or two `web_audio_capture` records): spectral distance (cosine on log-bins), RMS-envelope correlation, graph-signature diff. Phase 2 adds envelope comparison (trajectory L1 distance on gain/pan per layer). Returns per-facet distances + one `likeness_pass` verdict against a caller-supplied threshold; no pixel/waveform data, threshold typed not text-parsed.
*Input:* `before`, `after` (evidence ids or fingerprint JSON `{path|base64}`), `log_bin_tolerance`, `rms_correlation` (defaults in the tool summary).
*Effects:* `EVIDENCE`. *Requires:* `Nothing`. **Clean-room allowed.**

### 4. `web_audio_offline_render`
Headless check harness. Launches a private Chromium via `platform::process`, injects the rebuild's generative recipe (`OfflineAudioContext` + same layer/filter/oscillator shape as the captured graph), `startRendering()`, feeds the output through the same fingerprint path. Deep-only (it injects foreign code), deadline-bounded, cancellable. Phase 2 recipes may carry `envelope` automation (`setValueCurveAtTime`) so an interpolated mix, not only a fixed mix, is renderable and comparable.
*Input:* `recipe` (inline `{version, sample_rate, duration_ms, layers:[{kind, gain, filter}], …}` bounded to ≤ 256 layers), `duration_ms` (30_000 default), `cdp_endpoint?`, `timeout_secs`.
*Effects:* `EVIDENCE ∪ EXECUTES_FOREIGN_CODE ∪ ACCESSES_NETWORK ∪ LAUNCHES_PROCESS`.
*Requires:* `Nothing` (Chromium via `Browser::resolve`).

`web_audio_state_apply` (mixer-state restore) is **deferred** past the tracer bullet; tracer captures mixer state only as evidence. Restoring a captured envelope (the inverse of phase 2 tracing) is the follow-up that closes study → build → refine for dynamic mixing.

## Storage (data table, not text)

`audio-fingerprint.json` beside the capture: `{version:1, scenario_id, sample_rate, window_ms, log_bins[], rms_envelope[], graph_signature, mixer_state, mixer_envelope[], captured_at_ms, engine_version, deep_mode}` — `mixer_envelope` is `[]` in the tracer (absent phase-2 trace), populated in phase 2 without a version bump for the empty case. Constants (`FINGERPRINT_VERSION`, `MIN_VERSION`) and a `migrate()` live in `crates/analysis` (L2) so L0–L2 stay WASM-safe and the engine (L4) only serialises them. Unknown `version` → typed `ToolError`, never a panic; corrupt `log_bins` → graph-signature diff only, limitation recorded.

## Clean-room vs Deep, consent, safety

`clean_room.rs::TOOLS` gains `web_audio_fingerprint` + `web_audio_compare` only. Capture/render stay Deep-only. `EXECUTES_FOREIGN_CODE` on capture/render drives consent (`needs_consent()`); programmatic callers never open dialogs. Every child runs through `platform::process` only (stdin closed, wall + idle deadline, output capped, group killed). All caps checked; all errors are typed `ToolError`s with plain-English remediation. `unsafe_code = "forbid"`; no new dependency beyond the workspace.

## Layers

* `crates/analysis/src/audio.rs` (new, L2): fingerprint math, distance, migration. No I/O, no browser.
* `crates/engines/src/cdp.rs` already owns CDP transport (L3) — only a bounded `Runtime`/`Media` helper if needed.
* `crates/engine/src/tools/browser_tools.rs` (L4): four `ToolSpec`s, `spec()`-style, registered in `tools/mod.rs`. No CDP code in `ui-egui` (thin UI only).

## Tracer-bullet acceptance (all must hold)

1. Attached to the signed-in ego browser (`http://127.0.0.1:9223`, Explore / Focus): `web_audio_capture` over 30 s returns a non-empty graph, spectrum and mixer snapshot, with zero storage/cookie values in evidence (existing `Secrets` redaction tests + new assert).
2. `web_audio_fingerprint` on that capture writes `audio-fingerprint.json` v1.
3. `web_audio_offline_render` of a minimal hand-built Focus recipe (two layers, one low-pass, one pad oscillator) finishes inside the deadline and yields a fingerprint.
4. `web_audio_compare(reference, rendered)` reports `log_bin_cosine ≥ 0.85` and `rms_correlation ≥ 0.80` for a like-for-like recipe, and fails when the recipe's filter type is flipped — proving the check discriminates.
5. `cargo xtask layers`, `locks`, `secrets`, `tools --check`, `wasm` (analysis stays wasm-safe), `fmt`, `clippy -D warnings`, `cargo test -p craft-anything-engine` green; regression test per new failure mode (no `AudioContext`, no `AnalyserNode`, render timeout, unknown fingerprint version).

Follow-ups (already on R3.13, not this slice): ~~phase 2 interaction-trace~~ **landed** (`web_audio_capture` `interaction_trace` + `mixer_envelope` fingerprint/compare + `envelope` automation in `web_audio_offline_render`); remaining are `web_audio_state_apply`, remaining scenarios, a music-generation asset provider akin to `assets_image_generate`, hidden-graph capture via `Page.addScriptToEvaluateOnNewDocument`, FFT-size-robust binning, and `audio-fingerprint.json` persistence. Live Endel capture (acceptance 1) is blocked on that hidden-graph item; acceptance 2 (file persistence) is the persistence item above.
