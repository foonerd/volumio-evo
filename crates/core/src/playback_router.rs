//! Single entry for **replace and play** after URI resolution: optional video path, then MPD.
//!
//! See [VIDEO_COMPANION.md](../../../docs/VIDEO_COMPANION.md).

use crate::api::AppState;
use crate::mpd;

/// Clear queue, add resolved URIs, play — same as [`mpd::replace_and_play_resolved`] plus optional
/// **video-companion** takeover when feature is enabled.
pub async fn replace_and_play_uri(state: &AppState, uri: &str) -> anyhow::Result<()> {
    #[cfg(feature = "video-companion")]
    if let Some(()) = crate::video_companion::try_take_over_replace_and_play(state, uri).await? {
        return Ok(());
    }

    let config = mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    };
    mpd::replace_and_play_resolved(&config, &state.config.music_sources.music_root, uri).await
}
