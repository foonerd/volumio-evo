//! LAN browser video (**HLS** via **ffmpeg**). **Default:** device audio through **MPD** (same path as normal playback); optional legacy second **ffmpeg** → ALSA (`EVO_VIDEO_DEVICE_AUDIO=ffmpeg`). Scenario 1 — see [VIDEO_COMPANION.md](../../../docs/VIDEO_COMPANION.md).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use serde_json::Value;
use tokio::sync::Mutex;

/// Lowercase extensions treated as video containers for routing (doc §4).
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];

/// **HLS** segment duration (**seconds**) — shorter segments reduce browser latency vs room audio (`-hls_time`).
const HLS_SEGMENT_SECS: &str = "2";

/// **`EVO_VIDEO_DEVICE_AUDIO`**: unset / **`mpd`** (default) → device sound through **MPD** (same path as playback); **`ffmpeg`** → legacy second **ffmpeg** straight to ALSA (often sounds bad under load).
fn companion_device_audio_via_mpd() -> bool {
    match std::env::var("EVO_VIDEO_DEVICE_AUDIO") {
        Ok(v) => !matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "ffmpeg" | "ffmpeg_alsa" | "legacy"
        ),
        Err(_) => true,
    }
}

/// Prefer explicit env, then common absolute paths so **`systemd`**’s minimal **`PATH`** still finds tools.
fn resolved_external_tool(env_key: &'static str, fallback_name: &'static str, typical_paths: &[&str]) -> String {
    if let Ok(v) = std::env::var(env_key) {
        let v = v.trim();
        if !v.is_empty() {
            return v.to_string();
        }
    }
    for p in typical_paths {
        if Path::new(p).is_file() {
            return (*p).to_string();
        }
    }
    fallback_name.to_string()
}

pub fn ffmpeg_binary() -> String {
    resolved_external_tool(
        "EVO_FFMPEG_PATH",
        "ffmpeg",
        &["/usr/bin/ffmpeg", "/usr/local/bin/ffmpeg"],
    )
}

pub fn ffprobe_binary() -> String {
    resolved_external_tool(
        "EVO_FFPROBE_PATH",
        "ffprobe",
        &["/usr/bin/ffprobe", "/usr/local/bin/ffprobe"],
    )
}

/// True if the URI’s path leaf looks like a video file (Volumio `music-library/...` or plain path).
pub fn is_video_volumio_uri(uri: &str) -> bool {
    let leaf = uri
        .rsplit(['/', '\\'])
        .next()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(uri);
    Path::new(leaf)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let el = e.to_ascii_lowercase();
            VIDEO_EXTENSIONS.iter().any(|ext| *ext == el.as_str())
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum VideoRouteIntent {
    ReplaceAndPlay,
    AddPlayAppend,
    PlayItemsList,
}

/// Shared session controller (one library video at a time).
pub struct VideoSessionCtl {
    gen_counter: AtomicU64,
    pub inner: Mutex<Option<VideoSessionInner>>,
}

impl Default for VideoSessionCtl {
    fn default() -> Self {
        Self {
            gen_counter: AtomicU64::new(0),
            inner: Mutex::new(None),
        }
    }
}

impl VideoSessionCtl {
    pub fn next_generation(&self) -> u64 {
        self.gen_counter.fetch_add(1, Ordering::AcqRel) + 1
    }
}

pub struct VideoSessionInner {
    pub generation: u64,
    /// Video → **HLS** (heavy encode). Separate from ALSA so encoding load does not stall local playback.
    pub hls_ffmpeg_pid: u32,
    /// Audio → **ALSA** only (lightweight `ffmpeg`). `None` when source has no audio.
    pub alsa_ffmpeg_pid: Option<u32>,
    /// **`true`**: jack audio from **MPD** (recommended). **`false`**: **`alsa_ffmpeg_pid`** decodes to ALSA.
    pub audio_via_mpd: bool,
    pub volumio_uri: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_secs: Option<u64>,
    pub track_type: Option<String>,
    pub albumart: Option<String>,
    local_path: PathBuf,
    alsa_device: String,
    has_audio: bool,
    /// Logical transport (UI seek / pause).
    anchor_elapsed_ms: u64,
    anchor_wall: Instant,
    playing: bool,
    paused: bool,
}

impl VideoSessionInner {
    fn elapsed_ms_now(&self) -> u64 {
        let base = self.anchor_elapsed_ms;
        if self.playing && !self.paused {
            base.saturating_add(self.anchor_wall.elapsed().as_millis() as u64)
        } else {
            base
        }
    }

    fn cap_elapsed(&self, ms: u64) -> u64 {
        if let Some(d) = self.duration_secs {
            ms.min(d.saturating_mul(1000))
        } else {
            ms
        }
    }

    pub fn snapshot_elapsed_ms(&self) -> u64 {
        self.cap_elapsed(self.elapsed_ms_now())
    }

    fn sync_anchor_from_now(&mut self, elapsed_ms: u64) {
        let e = self.cap_elapsed(elapsed_ms);
        self.anchor_elapsed_ms = e;
        self.anchor_wall = Instant::now();
    }

    fn pause_timeline(&mut self) {
        if self.paused {
            return;
        }
        let now_ms = self.elapsed_ms_now();
        self.anchor_elapsed_ms = self.cap_elapsed(now_ms);
        self.paused = true;
        self.playing = false;
    }

    fn resume_timeline(&mut self) {
        if !self.paused {
            return;
        }
        self.paused = false;
        self.playing = true;
        self.anchor_wall = Instant::now();
    }

    pub fn toggle_timeline(&mut self) {
        if self.paused {
            self.resume_timeline();
        } else {
            self.pause_timeline();
        }
    }
}

#[derive(Debug)]
struct ProbeResult {
    duration_secs: Option<u64>,
    has_audio: bool,
    has_video: bool,
}

async fn ffprobe_json(path: &Path) -> anyhow::Result<Value> {
    let bin = ffprobe_binary();
    let out = tokio::process::Command::new(&bin)
        .arg("-v")
        .arg("error")
        .arg("-print_format")
        .arg("json")
        .arg("-show_format")
        .arg("-show_streams")
        .arg(path)
        .output()
        .await
        .with_context(|| {
            format!(
                "failed to spawn `{bin}` (install `ffmpeg` for `ffprobe`, or set EVO_FFPROBE_PATH; PATH={})",
                std::env::var("PATH").unwrap_or_default()
            )
        })?;
    if !out.status.success() {
        anyhow::bail!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let v: Value = serde_json::from_slice(&out.stdout)?;
    Ok(v)
}

fn probe_from_json(j: &Value) -> ProbeResult {
    let duration_secs = j
        .get("format")
        .and_then(|f| f.get("duration"))
        .and_then(|d| d.as_str())
        .and_then(|s| s.parse::<f64>().ok())
        .map(|x| x as u64);

    let mut has_audio = false;
    let mut has_video = false;
    if let Some(arr) = j.get("streams").and_then(|s| s.as_array()) {
        for st in arr {
            let ct = st.get("codec_type").and_then(|x| x.as_str());
            match ct {
                Some("audio") => has_audio = true,
                Some("video") => has_video = true,
                _ => {}
            }
        }
    }

    ProbeResult {
        duration_secs,
        has_audio,
        has_video,
    }
}

fn tags_from_probe(j: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let fmt_tags = j.get("format").and_then(|f| f.get("tags"));
    let stream0_tags = j
        .get("streams")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|st| st.get("tags"));

    fn tag(obj: Option<&Value>, key: &str) -> Option<String> {
        obj?.get(key).and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
    }

    let title = tag(fmt_tags, "title")
        .or_else(|| tag(stream0_tags, "title"))
        .or_else(|| tag(stream0_tags, "TITLE"));
    let artist = tag(fmt_tags, "artist")
        .or_else(|| tag(stream0_tags, "artist"))
        .or_else(|| tag(stream0_tags, "ARTIST"));
    let album = tag(fmt_tags, "album")
        .or_else(|| tag(stream0_tags, "album"))
        .or_else(|| tag(stream0_tags, "ALBUM"));

    (title, artist, album)
}

fn leaf_title(path: &Path) -> Option<String> {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
}

fn track_type_from_uri(uri: &str) -> Option<String> {
    let leaf = uri.rsplit('/').next()?.trim();
    let (_, ext) = leaf.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

async fn prepare_hls_directory() -> anyhow::Result<PathBuf> {
    let live = crate::paths::video_hls_live_dir();
    tokio::fs::create_dir_all(&live).await.with_context(|| {
        format!(
            "create HLS directory {live:?} (for a non-root service user, install \
             `RuntimeDirectory=volumio-evo` in the systemd unit — see layer/systemd/volumio-evo.service; \
             or set VOLUMIO_EVO_HLS_DIR to a writable path under /var/lib/volumio-evo)"
        )
    })?;
    Ok(live)
}

async fn scrub_hls_dir(dir: &Path) {
    let Ok(mut rd) = tokio::fs::read_dir(dir).await else {
        return;
    };
    while let Ok(Some(e)) = rd.next_entry().await {
        let _ = tokio::fs::remove_file(e.path()).await;
    }
}

#[cfg(unix)]
fn pid_kill(pid: u32, sig: i32) -> std::io::Result<()> {
    if pid == 0 {
        return Ok(());
    }
    let r = unsafe { libc::kill(pid as libc::pid_t, sig) };
    if r != 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(not(unix))]
fn pid_kill(_pid: u32, _sig: i32) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "video companion requires Unix",
    ))
}

/// **`EVO_VIDEO_ENCODER`**: **`auto`** (default) → **`libx264`**; **`h264_v4l2m2m`** / **`hw`** opt-in after you verify FFmpeg can open the V4L2 encoder on **this** host (presence of **`/dev/video*`** is not enough — headless Pi often fails **Could not find a valid device**).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VideoEncoderChoice {
    Libx264,
    V4l2m2m,
}

fn resolve_video_encoder() -> VideoEncoderChoice {
    match std::env::var("EVO_VIDEO_ENCODER") {
        Ok(s) => match s.trim().to_ascii_lowercase().as_str() {
            "" | "auto" | "libx264" | "sw" | "software" => VideoEncoderChoice::Libx264,
            "h264_v4l2m2m" | "hw" | "v4l2" | "pi" => VideoEncoderChoice::V4l2m2m,
            _ => {
                tracing::warn!(
                    "{} unknown EVO_VIDEO_ENCODER — using libx264",
                    crate::log_tags::EVO_PLAY
                );
                VideoEncoderChoice::Libx264
            }
        },
        Err(_) => VideoEncoderChoice::Libx264,
    }
}

fn append_video_encoder_args(cmd: &mut tokio::process::Command, enc: VideoEncoderChoice) {
    match enc {
        VideoEncoderChoice::Libx264 => {
            cmd.args([
                "-c:v",
                "libx264",
                "-preset",
                "superfast",
                "-pix_fmt",
                "yuv420p",
                "-x264-params",
                "threads=2:lookahead_threads=1:sync-lookahead=0",
            ]);
            cmd.args([
                "-g",
                "100",
                "-keyint_min",
                "25",
                "-sc_threshold",
                "0",
            ]);
        }
        VideoEncoderChoice::V4l2m2m => {
            tracing::info!(
                "{} using h264_v4l2m2m (unset EVO_VIDEO_ENCODER or set libx264 for CPU encode)",
                crate::log_tags::EVO_PLAY
            );
            cmd.args([
                "-c:v",
                "h264_v4l2m2m",
                "-pix_fmt",
                "yuv420p",
                "-b:v",
                "6M",
                "-maxrate",
                "8M",
                "-bufsize",
                "16M",
            ]);
            cmd.args(["-g", "100", "-keyint_min", "25"]);
        }
    }
}

fn pump_ffmpeg_stderr(mut stderr: tokio::process::ChildStderr) {
    tokio::spawn(async move {
        use tokio::io::AsyncBufReadExt;
        let reader = tokio::io::BufReader::new(&mut stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if !line.is_empty() {
                tracing::debug!(target: "volumio_evo::ffmpeg", "{}", line);
            }
        }
    });
}

/// Wait until **`ffmpeg`** has written a playlist listing at least one **`.ts`** segment (same clock family as **`-re`** input).
async fn wait_for_hls_manifest_ready(playlist_path: &Path, max_wait: Duration) -> anyhow::Result<()> {
    let poll = Duration::from_millis(120);
    let started = tokio::time::Instant::now();
    loop {
        if started.elapsed() > max_wait {
            anyhow::bail!(
                "timed out ({max_wait:?}) waiting for HLS playlist {:?}",
                playlist_path
            );
        }
        match tokio::fs::read_to_string(playlist_path).await {
            Ok(text) => {
                if text.contains("#EXTINF") && text.contains(".ts") {
                    return Ok(());
                }
            }
            Err(_) => {}
        }
        tokio::time::sleep(poll).await;
    }
}

/// Decode **audio only** → ALSA (second process). Same `-re`/`-ss` as HLS leg so clocks stay roughly aligned.
/// Industry pattern when one `ffmpeg` doing heavy video encode + ALSA starves realtime audio (common on ARM).
async fn spawn_ffmpeg_alsa_only(
    source: &Path,
    seek_secs: f64,
    alsa_dev: &str,
) -> anyhow::Result<tokio::process::Child> {
    let ffmpeg = ffmpeg_binary();
    let mut cmd = tokio::process::Command::new(&ffmpeg);
    cmd.kill_on_drop(false);
    cmd.stdin(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());
    cmd.arg("-y").arg("-nostdin");
    cmd.args(["-hide_banner", "-loglevel", "warning"]);
    if seek_secs > 0.05 {
        cmd.arg("-ss").arg(format!("{seek_secs:.3}"));
    }
    cmd.arg("-re");
    cmd.arg("-thread_queue_size").arg("2048");
    cmd.arg("-i").arg(source);
    cmd.arg("-vn");
    cmd.args(["-map", "0:a:0", "-ac", "2", "-sample_fmt", "s16", "-f", "alsa"]);
    cmd.arg(alsa_dev);
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "spawn `{ffmpeg}` (ALSA leg; set EVO_FFMPEG_PATH if needed; PATH={})",
            std::env::var("PATH").unwrap_or_default()
        )
    })?;
    if let Some(err) = child.stderr.take() {
        pump_ffmpeg_stderr(err);
    }
    Ok(child)
}

/// **Video only** → HLS (browser). No ALSA in this process — avoids encoder/RTP competing with sound card.
async fn spawn_ffmpeg_hls_only(
    source: &Path,
    seek_secs: f64,
    playlist_path: &Path,
    segment_pattern: &str,
) -> anyhow::Result<tokio::process::Child> {
    let ffmpeg = ffmpeg_binary();
    let mut cmd = tokio::process::Command::new(&ffmpeg);
    cmd.kill_on_drop(false);
    cmd.stdin(std::process::Stdio::null());
    cmd.stderr(std::process::Stdio::piped());
    cmd.arg("-y").arg("-nostdin");
    cmd.args(["-hide_banner", "-loglevel", "warning"]);
    if seek_secs > 0.05 {
        cmd.arg("-ss").arg(format!("{seek_secs:.3}"));
    }
    cmd.arg("-re");
    cmd.arg("-thread_queue_size").arg("1024");
    cmd.arg("-i").arg(source);
    cmd.arg("-sn");
    cmd.arg("-max_muxing_queue_size").arg("4096");
    cmd.arg("-map").arg("0:v:0");
    append_video_encoder_args(&mut cmd, resolve_video_encoder());
    cmd.args(["-f", "hls", "-hls_time", HLS_SEGMENT_SECS, "-hls_list_size", "12"]);
    cmd.arg("-hls_flags").arg(
        "delete_segments+append_list+omit_endlist+program_date_time",
    );
    cmd.args(["-hls_segment_filename", segment_pattern]);
    cmd.arg(playlist_path);
    let mut child = cmd.spawn().with_context(|| {
        format!(
            "spawn `{ffmpeg}` (HLS leg; set EVO_FFMPEG_PATH if needed; PATH={})",
            std::env::var("PATH").unwrap_or_default()
        )
    })?;
    if let Some(err) = child.stderr.take() {
        pump_ffmpeg_stderr(err);
    }
    Ok(child)
}

/// Kill the sibling **ffmpeg** when one leg exits so we do not leave audio or encoding running alone.
async fn cleanup_when_ffmpeg_leg_exits(state: crate::api::AppState, generation: u64, exited_pid: u32) {
    let mut lock = state.video_session.inner.lock().await;
    let Some(inner) = lock.as_ref() else {
        return;
    };
    if inner.generation != generation {
        return;
    }
    let ours = inner.hls_ffmpeg_pid == exited_pid || inner.alsa_ffmpeg_pid == Some(exited_pid);
    if !ours {
        return;
    }
    let audio_mpd = inner.audio_via_mpd;
    if inner.hls_ffmpeg_pid != exited_pid {
        let _ = pid_kill(inner.hls_ffmpeg_pid, libc::SIGKILL);
    }
    if let Some(a) = inner.alsa_ffmpeg_pid {
        if a != exited_pid {
            let _ = pid_kill(a, libc::SIGKILL);
        }
    }
    *lock = None;
    drop(lock);
    if audio_mpd {
        let cfg = crate::mpd::MpdConfig {
            host: state.config.mpd_host.clone(),
            port: state.config.mpd_port,
        };
        let _ = crate::mpd::stop_clear_queue_connected(&cfg).await;
    }
    state.clear_video_playback_active();
    state.notify_push_state();
    state.notify_push_queue();
}

async fn try_take_over_inner(
    state: &crate::api::AppState,
    volumio_uri: &str,
    intent: VideoRouteIntent,
) -> anyhow::Result<Option<()>> {
    let config = crate::mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    let music_root = &state.config.music_sources.music_root;
    let paths = crate::mpd::resolve_uri_for_queue(&config, music_root, volumio_uri).await?;
    if paths.len() != 1 {
        return Ok(None);
    }
    let only = &paths[0];
    if !is_video_volumio_uri(only) {
        return Ok(None);
    }

    let path = crate::mpd::resolved_queue_uri_to_path(music_root, only);
    start_video_session(state, path, volumio_uri.to_string(), intent).await?;
    Ok(Some(()))
}

pub(crate) async fn try_take_over_replace_and_play(
    state: &crate::api::AppState,
    uri: &str,
) -> anyhow::Result<Option<()>> {
    try_take_over_inner(state, uri, VideoRouteIntent::ReplaceAndPlay).await
}

pub(crate) async fn try_take_over_add_play_append(
    state: &crate::api::AppState,
    uri: &str,
) -> anyhow::Result<Option<()>> {
    try_take_over_inner(state, uri, VideoRouteIntent::AddPlayAppend).await
}

pub(crate) async fn try_take_over_play_items_list(
    state: &crate::api::AppState,
    uris: &[String],
    index: usize,
) -> anyhow::Result<Option<()>> {
    if index >= uris.len() {
        return Ok(None);
    }
    // Take over for **the row being played**, not the whole list — browse often sends many URIs
    // (e.g. folder contents) with a play index; MPD would get the full queue otherwise.
    try_take_over_inner(state, &uris[index], VideoRouteIntent::PlayItemsList).await
}

pub(crate) async fn start_video_session(
    state: &crate::api::AppState,
    source: PathBuf,
    volumio_uri: String,
    intent: VideoRouteIntent,
) -> anyhow::Result<()> {
    if !source.is_file() {
        anyhow::bail!("video source not a file: {:?}", source);
    }

    let j = ffprobe_json(&source).await?;
    let probe = probe_from_json(&j);
    if !probe.has_video {
        anyhow::bail!("no video stream in {:?}", source);
    }
    let (mut title, artist, album) = tags_from_probe(&j);
    if title.is_none() {
        title = leaf_title(&source);
    }

    let alsa = state.alsa.read().await.clone();
    let alsa_device = crate::alsa::mpd_playback_device(&alsa);
    let track_type = track_type_from_uri(&volumio_uri);
    let albumart = Some(crate::mpd::browse_song_albumart_path_only(&volumio_uri));

    let live_dir = prepare_hls_directory().await?;
    scrub_hls_dir(&live_dir).await;

    let playlist_path = live_dir.join("index.m3u8");
    let seg_pat = live_dir.join("seg_%03d.ts");
    let segment_pattern = seg_pat
        .to_str()
        .ok_or_else(|| anyhow!("invalid utf-8 in HLS path"))?
        .to_string();

    let mpd_cfg = crate::mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    let audio_mpd = companion_device_audio_via_mpd() && probe.has_audio;

    {
        let mut g = state.video_session.inner.lock().await;
        if let Some(prev) = g.as_ref() {
            let _ = pid_kill(prev.hls_ffmpeg_pid, libc::SIGKILL);
            if let Some(a) = prev.alsa_ffmpeg_pid {
                let _ = pid_kill(a, libc::SIGKILL);
            }
        }
        *g = None;
    }

    // Idle **MPD** before **`ffmpeg`** so device audio cannot run ahead of the sliding-window **HLS** encode.
    // When **`audio_via_mpd`**, we **`stop_clear`** here, spawn **HLS**, wait until the manifest lists a segment,
    // **then** **`add_play`** so clocks stay aligned (see **wait_for_hls_manifest_ready**).
    crate::mpd::stop_clear_queue_connected(&mpd_cfg)
        .await
        .with_context(|| "MPD stop/clear before video companion encode session")?;

    let generation = state.video_session.next_generation();
    let seek0 = 0_f64;

    let use_ffmpeg_alsa = probe.has_audio && !audio_mpd;
    let mut alsa_child = if use_ffmpeg_alsa {
        Some(
            spawn_ffmpeg_alsa_only(&source, seek0, &alsa_device)
                .await
                .with_context(|| "spawn ffmpeg ALSA leg")?,
        )
    } else {
        None
    };
    let mut hls_child = match spawn_ffmpeg_hls_only(&source, seek0, &playlist_path, &segment_pattern).await
    {
        Ok(c) => c,
        Err(e) => {
            if let Some(mut ac) = alsa_child.take() {
                let _ = ac.kill().await;
            }
            return Err(e.context("spawn ffmpeg HLS leg"));
        }
    };

    let hls_pid = match hls_child.id() {
        Some(p) => p,
        None => {
            if let Some(mut ac) = alsa_child.take() {
                let _ = ac.kill().await;
            }
            anyhow::bail!("ffmpeg HLS leg had no pid");
        }
    };
    let alsa_pid = alsa_child.as_mut().and_then(|c| c.id());

    if audio_mpd {
        if let Err(e) =
            wait_for_hls_manifest_ready(&playlist_path, Duration::from_secs(30)).await
        {
            let _ = pid_kill(hls_pid, libc::SIGKILL);
            if let Some(mut ac) = alsa_child.take() {
                let _ = ac.kill().await;
            }
            return Err(e).context("HLS playlist not ready before MPD play (audio_via_mpd)");
        }
        if let Err(e) =
            crate::mpd::add_play_connected(&mpd_cfg, volumio_uri.trim()).await
        {
            let _ = pid_kill(hls_pid, libc::SIGKILL);
            if let Some(mut ac) = alsa_child.take() {
                let _ = ac.kill().await;
            }
            return Err(e).context("MPD add+play for video companion (device audio)");
        }
    }

    let inner = VideoSessionInner {
        generation,
        hls_ffmpeg_pid: hls_pid,
        alsa_ffmpeg_pid: alsa_pid,
        audio_via_mpd: audio_mpd,
        volumio_uri: volumio_uri.clone(),
        title,
        artist,
        album,
        duration_secs: probe.duration_secs,
        track_type,
        albumart,
        local_path: source.clone(),
        alsa_device,
        has_audio: probe.has_audio,
        anchor_elapsed_ms: 0,
        anchor_wall: Instant::now(),
        playing: true,
        paused: false,
    };

    {
        let mut g = state.video_session.inner.lock().await;
        *g = Some(inner);
    }

    state.set_video_playback_active(true);
    state.notify_push_state();
    state.notify_push_queue();

    tracing::info!(
        "{} video session started ({:?}) gen={} hls_pid={} alsa_pid={:?} audio_via_mpd={} intent={:?} uri={}",
        crate::log_tags::EVO_PLAY,
        source,
        generation,
        hls_pid,
        alsa_pid,
        audio_mpd,
        intent,
        volumio_uri
    );

    let st_h = std::sync::Arc::clone(state);
    let gen_w = generation;
    tokio::spawn(async move {
        let status = hls_child.wait().await;
        tracing::info!(
            "{} ffmpeg HLS leg exited: {:?}",
            crate::log_tags::EVO_PLAY,
            status
        );
        cleanup_when_ffmpeg_leg_exits(st_h, gen_w, hls_pid).await;
    });

    if let (Some(mut ac), Some(ap)) = (alsa_child, alsa_pid) {
        let st_a = std::sync::Arc::clone(state);
        let gen_a = generation;
        tokio::spawn(async move {
            let status = ac.wait().await;
            tracing::info!(
                "{} ffmpeg ALSA leg exited: {:?}",
                crate::log_tags::EVO_PLAY,
                status
            );
            cleanup_when_ffmpeg_leg_exits(st_a, gen_a, ap).await;
        });
    }

    Ok(())
}

async fn restart_ffmpeg_at(
    state: &crate::api::AppState,
    pos_secs: f64,
) -> anyhow::Result<()> {
    let snapshot = {
        let mut g = state.video_session.inner.lock().await;
        let Some(inner) = g.as_mut() else {
            return Ok(());
        };
        let pos_ms = (pos_secs.max(0.0) * 1000.0) as u64;
        let capped_ms = inner
            .duration_secs
            .map(|d| pos_ms.min(d.saturating_mul(1000)))
            .unwrap_or(pos_ms);
        let generation = inner.generation;
        let source = inner.local_path.clone();
        let has_audio = inner.has_audio;
        let alsa_dev = inner.alsa_device.clone();
        let audio_via_mpd = inner.audio_via_mpd;
        let _ = pid_kill(inner.hls_ffmpeg_pid, libc::SIGKILL);
        if let Some(a) = inner.alsa_ffmpeg_pid {
            let _ = pid_kill(a, libc::SIGKILL);
        }
        (generation, source, has_audio, alsa_dev, capped_ms, audio_via_mpd)
    };

    let (generation, source, has_audio, alsa_dev, capped_ms, audio_via_mpd) = snapshot;
    let seek_secs = (capped_ms as f64) / 1000.0;

    let live_dir = prepare_hls_directory().await?;
    scrub_hls_dir(&live_dir).await;

    let playlist_path = live_dir.join("index.m3u8");
    let seg_pat = live_dir.join("seg_%03d.ts");
    let segment_pattern = seg_pat
        .to_str()
        .ok_or_else(|| anyhow!("invalid utf-8 in HLS path"))?
        .to_string();

    let use_ffmpeg_alsa = has_audio && !audio_via_mpd;
    let mut alsa_child = if use_ffmpeg_alsa {
        Some(spawn_ffmpeg_alsa_only(&source, seek_secs, &alsa_dev).await?)
    } else {
        None
    };
    let mut hls_child = match spawn_ffmpeg_hls_only(&source, seek_secs, &playlist_path, &segment_pattern).await {
        Ok(c) => c,
        Err(e) => {
            if let Some(mut ac) = alsa_child.take() {
                let _ = ac.kill().await;
            }
            return Err(e.context("spawn ffmpeg HLS leg after seek"));
        }
    };

    let hls_pid = match hls_child.id() {
        Some(p) => p,
        None => {
            if let Some(mut ac) = alsa_child.take() {
                let _ = ac.kill().await;
            }
            anyhow::bail!("ffmpeg HLS leg had no pid");
        }
    };
    let alsa_pid = alsa_child.as_mut().and_then(|c| c.id());

    if audio_via_mpd && has_audio {
        if let Err(e) =
            wait_for_hls_manifest_ready(&playlist_path, Duration::from_secs(30)).await
        {
            let _ = pid_kill(hls_pid, libc::SIGKILL);
            if let Some(a) = alsa_pid {
                let _ = pid_kill(a, libc::SIGKILL);
            }
            {
                let mut g = state.video_session.inner.lock().await;
                if let Some(inner) = g.as_ref() {
                    if inner.generation == generation {
                        *g = None;
                    }
                }
            }
            let cfg = crate::mpd::MpdConfig {
                host: state.config.mpd_host.clone(),
                port: state.config.mpd_port,
            };
            let _ = crate::mpd::stop_clear_queue_connected(&cfg).await;
            state.clear_video_playback_active();
            state.notify_push_state();
            state.notify_push_queue();
            return Err(e).context("HLS playlist not ready after seek (audio_via_mpd)");
        }
    }

    {
        let mut g = state.video_session.inner.lock().await;
        let Some(inner) = g.as_mut() else {
            let _ = pid_kill(hls_pid, libc::SIGKILL);
            if let Some(a) = alsa_pid {
                let _ = pid_kill(a, libc::SIGKILL);
            }
            return Ok(());
        };
        if inner.generation != generation {
            let _ = pid_kill(hls_pid, libc::SIGKILL);
            if let Some(a) = alsa_pid {
                let _ = pid_kill(a, libc::SIGKILL);
            }
            return Ok(());
        }
        inner.hls_ffmpeg_pid = hls_pid;
        inner.alsa_ffmpeg_pid = alsa_pid;
        inner.sync_anchor_from_now(capped_ms);
        inner.paused = false;
        inner.playing = true;
    }

    if has_audio && audio_via_mpd {
        let cfg = crate::mpd::MpdConfig {
            host: state.config.mpd_host.clone(),
            port: state.config.mpd_port,
        };
        let sec = (capped_ms / 1000).min(i64::MAX as u64) as i64;
        let _ = crate::mpd::run_command_connected(&cfg, "seek", None, Some(sec), None, None).await;
    }

    let st_h = std::sync::Arc::clone(state);
    let gen_w = generation;
    tokio::spawn(async move {
        let status = hls_child.wait().await;
        tracing::debug!(
            "{} ffmpeg HLS leg (after seek) exited: {:?}",
            crate::log_tags::EVO_PLAY,
            status
        );
        cleanup_when_ffmpeg_leg_exits(st_h, gen_w, hls_pid).await;
    });

    if let (Some(mut ac), Some(ap)) = (alsa_child, alsa_pid) {
        let st_a = std::sync::Arc::clone(state);
        let gen_a = generation;
        tokio::spawn(async move {
            let status = ac.wait().await;
            tracing::debug!(
                "{} ffmpeg ALSA leg (after seek) exited: {:?}",
                crate::log_tags::EVO_PLAY,
                status
            );
            cleanup_when_ffmpeg_leg_exits(st_a, gen_a, ap).await;
        });
    }

    Ok(())
}

pub async fn stop_session(state: &crate::api::AppState) {
    let (pids, stop_mpd) = {
        let mut g = state.video_session.inner.lock().await;
        let prev = g.as_ref();
        if prev.is_none() && !state.video_playback_is_active() {
            return;
        }
        let stop_mpd = prev.is_some_and(|v| v.audio_via_mpd);
        let pids = prev.map(|v| (v.hls_ffmpeg_pid, v.alsa_ffmpeg_pid));
        *g = None;
        (pids, stop_mpd)
    };
    if let Some((hls, alsa)) = pids {
        let _ = pid_kill(hls, libc::SIGKILL);
        if let Some(a) = alsa {
            let _ = pid_kill(a, libc::SIGKILL);
        }
    }
    if stop_mpd {
        let cfg = crate::mpd::MpdConfig {
            host: state.config.mpd_host.clone(),
            port: state.config.mpd_port,
        };
        let _ = crate::mpd::stop_clear_queue_connected(&cfg).await;
    }
    state.clear_video_playback_active();
    state.notify_push_state();
    state.notify_push_queue();
}

/// Build full `pushState` / REST body while a encode session owns playback.
pub async fn volumio_state_for_video_session(
    state: &crate::api::AppState,
    master_volume_from_alsa: Option<u8>,
) -> Option<crate::mpd::VolumioState> {
    let (
        audio_via_mpd,
        paused_fallback,
        snapshot_elapsed,
        duration_secs,
        title,
        artist,
        album,
        volumio_uri,
        track_type,
        albumart,
    ) = {
        let inner = state.video_session.inner.lock().await;
        let s = inner.as_ref()?;
        (
            s.audio_via_mpd,
            s.paused,
            s.snapshot_elapsed_ms(),
            s.duration_secs,
            s.title.clone(),
            s.artist.clone(),
            s.album.clone(),
            s.volumio_uri.clone(),
            s.track_type.clone(),
            s.albumart.clone(),
        )
    };

    let cfg = crate::mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };

    let (status_str, seek_ms) = if audio_via_mpd {
        let elapsed = crate::mpd::status_elapsed_ms_connected(&cfg).await.ok().flatten();
        let st = crate::mpd::status_volumio_style_string_connected(&cfg)
            .await
            .ok()
            .flatten();
        let seek_ms = elapsed.unwrap_or(snapshot_elapsed);
        let seek_ms = duration_secs
            .map(|d| seek_ms.min(d.saturating_mul(1000)))
            .unwrap_or(seek_ms);
        let mut status_str = st.unwrap_or_else(|| {
            if paused_fallback {
                "pause".to_string()
            } else {
                "play".to_string()
            }
        });
        if status_str == "stop" {
            status_str = "pause".to_string();
        }
        (status_str, seek_ms)
    } else {
        let status_str = if paused_fallback {
            "pause".to_string()
        } else {
            "play".to_string()
        };
        (status_str, snapshot_elapsed)
    };

    let volume = master_volume_from_alsa;

    Some(crate::mpd::VolumioState {
        status: Some(status_str),
        position: Some(0),
        seek: Some(seek_ms),
        duration: duration_secs,
        volume,
        mute: false,
        disable_volume_control: false,
        repeat: Some(false),
        random: Some(false),
        repeat_single: Some(false),
        consume: Some(false),
        title,
        artist,
        album,
        uri: Some(volumio_uri),
        track_type,
        service: Some("mpd".to_string()),
        albumart,
        samplerate: None,
        bitdepth: None,
        bitrate: None,
        updatedb: false,
        video_stream_url: Some("/hls/live/index.m3u8".to_string()),
    })
}

pub async fn video_queue_items(state: &crate::api::AppState) -> Option<Vec<crate::mpd::QueueItem>> {
    let inner = state.video_session.inner.lock().await;
    let s = inner.as_ref()?;
    let name = s.title.clone();
    let albumart = s.albumart.clone().unwrap_or_default();
    let track_type = s.track_type.clone().unwrap_or_else(|| {
        let path = s.volumio_uri.rsplit('/').next().unwrap_or("");
        path.rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default()
    });
    Some(vec![crate::mpd::QueueItem {
        position: 0,
        name,
        title: s.title.clone(),
        artist: s.artist.clone(),
        album: s.album.clone(),
        uri: Some(s.volumio_uri.clone()),
        duration: s.duration_secs,
        service: "mpd".to_string(),
        albumart,
        track_type,
    }])
}

pub async fn transport_dispatch(
    state: &crate::api::AppState,
    cmd: &str,
    position: Option<i64>,
) -> anyhow::Result<()> {
    match cmd {
        "stop" | "clearQueue" | "next" | "prev" => {
            stop_session(state).await;
        }
        "pause" => {
            let cfg = crate::mpd::MpdConfig {
                host: state.config.mpd_host.clone(),
                port: state.config.mpd_port,
            };
            let (hls, alsa, audio_via_mpd) = {
                let mut g = state.video_session.inner.lock().await;
                let Some(inner) = g.as_mut() else {
                    return Ok(());
                };
                inner.pause_timeline();
                (
                    inner.hls_ffmpeg_pid,
                    inner.alsa_ffmpeg_pid,
                    inner.audio_via_mpd,
                )
            };
            if audio_via_mpd {
                let _ =
                    crate::mpd::run_command_connected(&cfg, "pause", None, None, None, None).await;
            }
            let _ = pid_kill(hls, libc::SIGSTOP);
            if let Some(a) = alsa {
                let _ = pid_kill(a, libc::SIGSTOP);
            }
            state.notify_push_state();
        }
        "play" => {
            let cfg = crate::mpd::MpdConfig {
                host: state.config.mpd_host.clone(),
                port: state.config.mpd_port,
            };
            let (hls, alsa, audio_via_mpd) = {
                let mut g = state.video_session.inner.lock().await;
                let Some(inner) = g.as_mut() else {
                    return Ok(());
                };
                inner.resume_timeline();
                (
                    inner.hls_ffmpeg_pid,
                    inner.alsa_ffmpeg_pid,
                    inner.audio_via_mpd,
                )
            };
            if audio_via_mpd {
                let _ =
                    crate::mpd::run_command_connected(&cfg, "play", None, None, None, None).await;
            }
            let _ = pid_kill(hls, libc::SIGCONT);
            if let Some(a) = alsa {
                let _ = pid_kill(a, libc::SIGCONT);
            }
            state.notify_push_state();
        }
        "toggle" => {
            let cfg = crate::mpd::MpdConfig {
                host: state.config.mpd_host.clone(),
                port: state.config.mpd_port,
            };
            let (hls, alsa, audio_via_mpd, cont) = {
                let mut g = state.video_session.inner.lock().await;
                let Some(inner) = g.as_mut() else {
                    return Ok(());
                };
                inner.toggle_timeline();
                let cont = inner.paused;
                (
                    inner.hls_ffmpeg_pid,
                    inner.alsa_ffmpeg_pid,
                    inner.audio_via_mpd,
                    cont,
                )
            };
            if audio_via_mpd {
                let _ =
                    crate::mpd::run_command_connected(&cfg, "toggle", None, None, None, None).await;
            }
            let sig = if cont {
                libc::SIGSTOP
            } else {
                libc::SIGCONT
            };
            let _ = pid_kill(hls, sig);
            if let Some(a) = alsa {
                let _ = pid_kill(a, sig);
            }
            state.notify_push_state();
        }
        "seek" => {
            if let Some(pos) = position {
                restart_ffmpeg_at(state, pos as f64).await?;
                state.notify_push_state();
            }
        }
        _ => {}
    }
    Ok(())
}

pub async fn transport_skip_relative(
    state: &crate::api::AppState,
    delta_secs: i64,
) -> anyhow::Result<()> {
    let inner = state.video_session.inner.lock().await;
    let Some(s) = inner.as_ref() else {
        return Ok(());
    };
    let cur_ms = s.snapshot_elapsed_ms() as i128;
    let delta_ms = (delta_secs as i128) * 1000;
    let new_ms = (cur_ms + delta_ms).max(0) as u64;
    let new_sec = (new_ms as f64) / 1000.0;
    drop(inner);
    restart_ffmpeg_at(state, new_sec).await?;
    state.notify_push_state();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn music_library_uri_maps_under_music_root() {
        assert_eq!(
            crate::mpd::resolved_queue_uri_to_path(
                Path::new("/srv/media"),
                "music-library/INTERNAL/Videos/a.mp4",
            ),
            PathBuf::from("/srv/media/INTERNAL/Videos/a.mp4")
        );
    }

    #[test]
    fn video_extensions_recognized() {
        assert!(is_video_volumio_uri("music-library/INTERNAL/A/foo.mp4"));
        assert!(is_video_volumio_uri("music-library/USB/x/bar.MKV"));
        assert!(!is_video_volumio_uri("music-library/INTERNAL/a.flac"));
        assert!(!is_video_volumio_uri("music-library/INTERNAL/a.mp3"));
    }
}
