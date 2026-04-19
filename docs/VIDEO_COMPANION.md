# Video companion (design concept)

**Status:** **Scenario 1 (headless + LAN HLS)** is implemented in **volumio-evo** (single `ffmpeg` process: ALSA + HLS under `GET /hls/...`, `pushState.videoStreamUrl`, stock layer `index.html` + `evo-video-overlay.js` + vendored `hls.js`). **Local HDMI** and “audio+video in browser” remain future work. **Scope:** Rust **volumio-evo** only; no Node.

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
- **Runtime user** for groups / permissions: §11 — **never hardcode** a login name in scripts or docs.

---

## 9. Branch

Work for this concept lives on **`video-companion`** until reviewed for merge to `main`.

---

## 10. Implementation (Scenario 1 — in-tree)

- **`playback_router`** — same entry points as above; when the resolved library path for the command is video, **`video_companion`** runs **`ffmpeg`** (ALSA + **HLS** into **`/run/volumio-evo/hls/live/`**, overridable via **`VOLUMIO_EVO_HLS_DIR`**) and exposes **`GET /hls/...`** via Axum **`ServeDir`**. The shipped unit sets **`RuntimeDirectory=volumio-evo`** so a **non-root** service user can create **`/run/volumio-evo/...`** (otherwise **`mkdir`** returns **EACCES**). For **`playItemsList`**, only the URI at the **played index** is considered (browse often sends many rows).
- **`video_companion`** — **`stop_clear_queue_connected`** MPD first; transport uses **SIGSTOP/SIGCONT** (pause) and **seek** restarts **`ffmpeg`** with **`-ss`**. Encoding uses **`-re`**, **4 s** HLS segments, mux queues; **ALSA** gets **`-af aresample=async=1`** so brief encode stalls do not sound like a variable-speed “tap”. **`EVO_VIDEO_ENCODER`**: default **`auto`** → **`libx264`** (capped threads). Opt in to **`h264_v4l2m2m`** (**`hw`**) only after a manual FFmpeg test — **`/dev/video*`** alone is **not** a reliable signal on headless Pi (encoder may fail with **Could not find a valid device**). Hardware encode needs **`video`** / **`render`** (**§11**).
- **`VolumioState.videoStreamUrl`** — **`/hls/live/index.m3u8`** while a session is active; **`pushState`** / **`getState`** / **`getQueue`** use the video snapshot when **`video_playback_active`**.
- **UI** — **`layer/web/*/index.html`** loads **`/evo-hls.min.js`** (vendored **hls.js**) and **`/evo-video-overlay.js`** (Angular **`socket:pushState`** hook, `<video muted>`). The overlay **retries** the manifest URL briefly so a transient **404** (playlist not written yet right after **`ffmpeg`** start) does not strand the spinner.

If **`GET /hls/live/index.m3u8`** stays **404**: nginx is proxying (**`Server: nginx`** proves port **80** hit nginx) — the backend **`ServeDir`** then has **no file** (`Content-Length: 0` on 404). On the device: **`ls /run/volumio-evo/hls/live/`** while “playing”; empty means no **ffmpeg** session (**`journalctl -u volumio-evo`**). **`curl -sS http://127.0.0.1:3000/hls/live/index.m3u8`** bypasses nginx; **`RuntimeDirectory=volumio-evo`** must be installed so HLS dirs are writable (**§10** systemd unit).

Build: plain **`cargo build -p volumio-evo-core`** (video path is always compiled). Runtime needs **`ffmpeg`** and **`ffprobe`** (Debian: package **`ffmpeg`**, which provides both; bootstrap installs it). If `systemd`’s **`PATH`** is too small, Evo also tries **`/usr/bin/ffmpeg`** and **`/usr/bin/ffprobe`**, or set **`EVO_FFMPEG_PATH`** / **`EVO_FFPROBE_PATH`**.

---

## 11. Runtime user — **never hardcode a login**

Canonical policy is **[RUNTIME_USER.md](RUNTIME_USER.md)**: Evo does **not** assume **`volumio`**, uid **1000**, or any fixed account. The **`volumio-evo`** service runs as **`EVO_SERVICE_USER`** after bootstrap resolution (or **root** when explicitly configured).

**Companion OS packages** (**`mpv`**, **`ffmpeg`**, …) are installed **as root** (`apt`); package installs do **not** encode a login.

**DRM / V4L2 device access** (`/dev/dri`, `/dev/video*`) requires the **same UID as the Evo service** to be in supplementary groups **`audio`**, **`video`**, and **`render`**. Bootstrap should set **`SupplementaryGroups=`** and **`usermod`** for **whatever user owns the service** — the variable **`${u}`** in **`bootstrap-volumio-evo-player.sh`**, **not** a literal string.

For **one-off** fixes on a live system, resolve the service user from systemd first, then fall back to the invoking login:

```bash
EVO_USER="$(grep -h '^User=' /etc/systemd/system/volumio-evo.service.d/*.conf 2>/dev/null | head -1)"
EVO_USER="${EVO_USER#User=}"
EVO_USER="$(echo "$EVO_USER" | tr -d '[:space:]')"
if [[ -z "$EVO_USER" ]]; then
  EVO_USER="${SUDO_USER:-$(id -un)}"
fi
sudo usermod -aG audio,video,render "$EVO_USER"
```

After changing groups, **restart** **`volumio-evo`** (or re-login for SSH-only checks). Future bootstrap changes should extend **`SupplementaryGroups=`** beyond **`audio`** when video companion packages are enabled.

---

## 12. Bootstrap **apt** capture (sample dry-run)

**Purpose:** lock the **exact** Debian/Raspberry Pi closure for **companion** tooling before adding to **`bootstrap-volumio-evo-player.sh`** (or a companion phase). **Re-run** `apt-get install -sy …` before each release — versions move.

### 12.1 Command (no install)

```bash
sudo apt-get update -qq
sudo apt-get install -sy mpv ffmpeg gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-libav gstreamer1.0-tools vainfo 2>&1 | tee /tmp/volumio-video-companion-apt-dryrun.txt
```

Optional **smaller** footprint for spikes (mpv + ffmpeg only):

```bash
sudo apt-get install -sy mpv ffmpeg 2>&1 | tee /tmp/volumio-video-companion-minimal-dryrun.txt
```

### 12.2 Sample outcome (device snapshot — **do not treat as frozen**)

| Field | Value |
|-------|--------|
| **Recorded** | 2026-04, **Debian 13 (trixie)** **`DEBIAN_VERSION_FULL=13.4`**, **arm64**, Pi kernel **`6.12.*+rpt`** |
| **Requested top-level** | **`mpv`**, **`ffmpeg`**, **`gstreamer1.0-plugins-{base,good,bad}`**, **`gstreamer1.0-libav`**, **`gstreamer1.0-tools`**, **`vainfo`** |
| **Result** | **120** new packages, **0** upgraded, **0** removed |
| **Repos** | Mix of **`Debian:13.4/stable`**, **`Debian-Security:13/stable-security`**, and **`Raspberry Pi Foundation:stable`** (**`+rpt`**, **`+rpt3`**) for **`ffmpeg`**, **`libavdevice61`**, several **`gstreamer1.0-*`** and **`libgstreamer-*`** |

**Direct `Inst` lines for the requested names (versions from capture):**

- **`ffmpeg`** `8:7.1.3-0+deb13u1+rpt1` [arm64] — Raspberry Pi Foundation  
- **`mpv`** `0.40.0-3+deb13u1` [arm64] — Debian stable  
- **`gstreamer1.0-tools`** `1.26.2-2` [arm64]  
- **`gstreamer1.0-plugins-base`** `1.26.2-1+rpt3+deb13u1` [arm64] — Pi Foundation  
- **`gstreamer1.0-plugins-good`** `1.26.2-1` [arm64]  
- **`gstreamer1.0-plugins-bad`** `1.26.2-3+rpt3+deb13u1` [arm64] — Pi Foundation  
- **`gstreamer1.0-libav`** `1.26.2-1` [arm64]  
- **`vainfo`** `2.22.0+ds1-2` [arm64]  

**Also pulled** (among others): **`libavdevice61`**, **`gstreamer1.0-gl`**, **`gstreamer1.0-x`**, **`ghostscript`** / font stacks (**`mpv`** dependency chain), **`yt-dlp`**, Python bits — see full **`tee`** log.

### 12.3 Groups vs packages (same sample host)

On that host, **`volumio-evo`** **MainPID** already had **`video`** and **`render`** in effective supplementary groups (§11 probes). **Package install** does not replace group setup; it only supplies **`mpv`/`ffmpeg`/GStreamer** binaries.

### 12.4 Install for real (operator)

After reviewing **`/tmp/volumio-video-companion-apt-dryrun.txt`**, run the same package list **without** **`y`**:

```bash
sudo apt-get install -y mpv ffmpeg gstreamer1.0-plugins-base gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-bad gstreamer1.0-libav gstreamer1.0-tools vainfo
```
