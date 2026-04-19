//! Video companion: classify library URIs and optionally take over playback before MPD.
//!
//! Full decode/display is **not** implemented here — see [VIDEO_COMPANION.md](../../../docs/VIDEO_COMPANION.md).
//! Enable routing hooks with Cargo feature **`video-companion`**.

use std::path::Path;

/// Lowercase extensions treated as video containers for routing (concept doc §4).
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mkv", "webm", "mov", "avi", "m4v"];

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

#[cfg(feature = "video-companion")]
async fn take_over_replace_and_play(
    state: &crate::api::AppState,
    uri: &str,
) -> anyhow::Result<Option<()>> {
    let config = crate::mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    let music_root = &state.config.music_sources.music_root;
    let paths = crate::mpd::resolve_uri_for_queue(&config, music_root, uri).await?;
    if paths.len() != 1 {
        return Ok(None);
    }
    let only = &paths[0];
    if !is_video_volumio_uri(only) {
        return Ok(None);
    }
    tracing::info!(
        "{} video-companion: single video URI {:?} — hook reserved; still using MPD until player lands",
        crate::log_tags::EVO_PLAY,
        only
    );
    Ok(None)
}

/// When feature **`video-companion`** is enabled, inspect the resolved URI; return **`Some(())`**
/// if this request was fully handled without MPD. Stub implementation returns **`None`** (fall through).
#[cfg(feature = "video-companion")]
pub async fn try_take_over_replace_and_play(
    state: &crate::api::AppState,
    uri: &str,
) -> anyhow::Result<Option<()>> {
    take_over_replace_and_play(state, uri).await
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
