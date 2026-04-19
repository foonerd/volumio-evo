# Video companion (design concept)

**Status:** concept / git branch **`video-companion`** — not part of stock parity until explicitly merged. **Scope:** Rust **volumio-evo** only; no Node.

**Related:** [PLAYBACK_STATE_REQUIREMENTS.md](PLAYBACK_STATE_REQUIREMENTS.md) (`pushState` / timer contract), [CONCEPT.md](CONCEPT.md) (Evo architecture).

---

## 1. Goals

| Priority | Scenario |
|----------|-----------|
| **Must** | **Headless player + LAN browser:** audio from the device’s normal path (ALSA / Evo playback options); **video picture** in the browser (Chrome, Safari, Firefox, Opera). Browser video is **muted**; sound comes only from the player. |
| **Must** | **Local display** (HDMI, DSI, framebuffer-class output): **video on the attached screen**, audio from the **same** player audio path as today. |
| **Later** | **Remote client:** audio **and** video both in the browser — different product mode (duplicate sink or browser-only audio); defer. |

---

## 2. Non-goals (for initial delivery)

- Replacing or extending **MPD** to decode video.
- Implementing codecs in **Rust** (no per-format decoder crates in Evo).
- DRM / Widevine / subscription streaming providers (unless explicitly scoped later).

---

## 3. Core idea: one router, exclusive backends

Playback today flows through **MPD** for library audio (`replaceAndPlay`, `addPlay`, `playItemsList`, etc.). Video **cannot** be routed through MPD as the decoder.

Introduce a **playback router** at the **same command entry points** (after URI → path resolution, before MPD):

1. **`is_video_track(path | uri)`** — primarily extension allowlist (e.g. `.mp4`, `.mkv`, `.webm`, `.mov`) + optional **`ffprobe`** (or equivalent) for ambiguous cases.
2. If **audio-only** → existing **MPD** path (unchanged).
3. If **video** → **stop MPD** (no competing audio), start a **`VideoCompanionSession`** (Rust-owned):

   - Single decode pipeline (see §5): demux, sync, audio to ALSA (or configured sink), video to **either** a LAN-visible stream **or** a local display sink — never two independent decoders for the same file.

**Naming:** The router is the single policy gate; avoid scattering “is video?” checks in the UI only. UI reacts to **state** (§6), not to guessing formats locally.

---

## 4. Codec / format handling

- **Containers** (MKV, MP4, WebM) wrap **codecs** (H.264, HEVC, VP9, AV1, AAC, …).
- Evo does **not** maintain a matrix of hand-wired decoders in Rust.
- Ship **one** capable stack on the OS image:

  - **mpv** (FFmpeg) **or** **GStreamer** with FFmpeg + good plugins.

- “Extra decoders” in practice means: **complete FFmpeg/GStreamer build**, optional **hardware** offload for SBCs (V4L2, etc.), and **legal/build** choices for patent-encumbered codecs (e.g. HEVC) — not new Rust modules per extension.

---

## 5. Implementation options (orchestration only in Evo)

| Approach | Evo’s job |
|----------|-----------|
| **mpv + JSON IPC** | Spawn/control mpv; map play/pause/seek/stop; read time/position events. |
| **GStreamer (e.g. `gstreamer-rs`)** | Build pipeline: `filesink`/network src → decode → `alsasink` + **`kmssink` / compositor** (HDMI) or encode branch for LAN. |

**LAN browser:** mux/encode video to something the browser can play (`<video>`, **MSE** / **HLS** / etc.); serve a **URL** from Evo (same host as UI). **Audio** stays off the browser for the must-have scenario.

**Sync (headless + muted browser video):** device is time authority; UI shows a muted stream — accept bounded drift or occasional alignment using session-reported position (see [PLAYBACK_STATE_REQUIREMENTS.md](PLAYBACK_STATE_REQUIREMENTS.md) when extending state).

---

## 6. State and UI contract

- While **MPD** is authoritative, `getState` / `pushState` follow existing MPD mapping (see `mpd::VolumioState`).

- While **`VideoCompanionSession`** is active, timing and track metadata must reflect the **video session**, not MPD.

- Extend the published state in a **backward-compatible** way, for example:

  - `videoActive`, and/or a small `video: { … }` object with `streamUrl` (LAN), `sink: "hdmi" | "lan"`, optional `width` / `height`.
  - Keep `trackType` / URI useful for icons and debugging.

Exact field names should be aligned with [PLAYBACK_STATE_REQUIREMENTS.md](PLAYBACK_STATE_REQUIREMENTS.md) and any stock UI fork notes in [UI_GAP.md](UI_GAP.md) when implementing.

---

## 7. Phasing

1. **Spike:** **Volatile** video play — router stops MPD, plays one file via `VideoCompanionSession`, stop returns to idle (or documented behaviour). Minimal queue coupling.
2. **Next:** stable **pause/seek/stop** and display policy (HDMI vs LAN detection).
3. **Later:** **Queue** parity (skip/next, mixed audio/video lists) requires a **logical queue** in Evo or clear rules; MPD alone cannot drive mpv’s queue without a custom layer.

---

## 8. OS / image

- Add **mpv** or **GStreamer** (+ plugins) to the Volumio layer / package set; document in the same place other optional binaries are described (e.g. tester/bootstrap docs when packaging is fixed).
- Optional: hardware decode packages per board profile.

---

## 9. Branch

Work for this concept lives on **`video-companion`** until reviewed for merge to `main`.

---

## 10. Implementation stub (in-tree)

- **`playback_router::replace_and_play_uri`** — used by Socket.IO **`replaceAndPlay`** (non-playlist) and **`POST /api/v1/replaceAndPlay`**. Calls **`video_companion::try_take_over_replace_and_play`** when built with **`--features video-companion`**, then **`mpd::replace_and_play_resolved`**.
- **`video_companion::is_video_volumio_uri`** — extension-based classification (always built; unit-tested).
- Stub takeover currently logs and returns **fall through** to MPD until a real **`VideoCompanionSession`** exists.

Build with hooks: **`cargo build -p volumio-evo-core --features video-companion`** (default remains unchanged).
