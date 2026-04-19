//! Video companion: classify library URIs and optionally take over playback before MPD.
//!
//! Full decode/display is **not** implemented here — see [VIDEO_COMPANION.md](../../../docs/VIDEO_COMPANION.md).
//! Enable routing hooks with Cargo feature **`video-companion`**.
//!
//! Note: **`main.rs`** duplicates this module alongside `lib.rs`; without **`video-companion`**, only
//! classification helpers are compiled and may appear unused in the **binary** crate during `cargo build`.

use std::path::Path;

/// Lowercase extensions treated as video containers for routing (concept doc §4).
#[allow(dead_code)]
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];

/// True if the URI’s path leaf looks like a video file (Volumio `music-library/...` or plain path).
#[allow(dead_code)]
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

#[cfg(feature = "video-companion")]
#[derive(Debug, Clone, Copy)]
pub(crate) enum VideoRouteIntent {
    ReplaceAndPlay,
    AddPlayAppend,
    PlayItemsList,
}

#[cfg(feature = "video-companion")]
async fn try_take_over_single_resolved_uri(
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
    tracing::info!(
        "{} video-companion {:?}: {:?} — hook reserved; still using MPD until player lands",
        crate::log_tags::EVO_PLAY,
        intent,
        only
    );
    Ok(None)
}

/// Clear queue + play: optional takeover before MPD.
#[cfg(feature = "video-companion")]
pub async fn try_take_over_replace_and_play(
    state: &crate::api::AppState,
    uri: &str,
) -> anyhow::Result<Option<()>> {
    try_take_over_single_resolved_uri(state, uri, VideoRouteIntent::ReplaceAndPlay).await
}

/// Append + play: optional takeover before MPD.
#[cfg(feature = "video-companion")]
pub async fn try_take_over_add_play_append(
    state: &crate::api::AppState,
    uri: &str,
) -> anyhow::Result<Option<()>> {
    try_take_over_single_resolved_uri(state, uri, VideoRouteIntent::AddPlayAppend).await
}

/// Multi-row play: only when the list is a single resolved video track (same policy as replace).
#[cfg(feature = "video-companion")]
pub async fn try_take_over_play_items_list(
    state: &crate::api::AppState,
    uris: &[String],
    index: usize,
) -> anyhow::Result<Option<()>> {
    if uris.len() != 1 || index >= uris.len() {
        return Ok(None);
    }
    try_take_over_single_resolved_uri(state, &uris[index], VideoRouteIntent::PlayItemsList).await
}

// --- Transport (when `RouterState.video_playback_active` is true; mpv stub for now)

#[cfg(feature = "video-companion")]
pub async fn transport_dispatch(
    state: &crate::api::AppState,
    cmd: &str,
    position: Option<i64>,
) -> anyhow::Result<()> {
    tracing::debug!(
        target: "volumio_evo::video",
        "transport {} position={:?} (stub)",
        cmd,
        position
    );
    if matches!(cmd, "stop" | "clearQueue") {
        state.clear_video_playback_active();
    }
    Ok(())
}

#[cfg(feature = "video-companion")]
pub async fn transport_skip_relative(
    _state: &crate::api::AppState,
    delta_secs: i64,
) -> anyhow::Result<()> {
    tracing::debug!(
        target: "volumio_evo::video",
        "skip Δ={}s (stub)",
        delta_secs
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_extensions_recognized() {
        assert!(is_video_volumio_uri("music-library/INTERNAL/A/foo.mp4"));
        assert!(is_video_volumio_uri("music-library/USB/x/bar.MKV"));
        assert!(!is_video_volumio_uri("music-library/INTERNAL/a.flac"));
        assert!(!is_video_volumio_uri("music-library/INTERNAL/a.mp3"));
    }
}
