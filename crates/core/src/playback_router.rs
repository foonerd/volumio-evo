//! Playback entry points: optional **video-companion** takeover, then MPD.
//!
//! See [VIDEO_COMPANION.md](../../../docs/VIDEO_COMPANION.md).

use crate::api::AppState;
use crate::mpd;

fn mpd_cfg(state: &AppState) -> mpd::MpdConfig {
    mpd::MpdConfig {
        host: state.config.mpd_host.clone(),
        port: state.config.mpd_port,
    }
}

/// MPD is about to own the queue; clear the “video session” flag (no-op when never set).
fn clear_video_then_mpd(state: &AppState) {
    state.clear_video_playback_active();
}

/// Clear queue, add resolved URIs, play — [`mpd::replace_and_play_resolved`] plus optional video takeover.
pub async fn replace_and_play_uri(state: &AppState, uri: &str) -> anyhow::Result<()> {
    #[cfg(feature = "video-companion")]
    if let Some(()) = crate::video_companion::try_take_over_replace_and_play(state, uri).await? {
        state.set_video_playback_active(true);
        return Ok(());
    }
    clear_video_then_mpd(state);
    mpd::replace_and_play_resolved(&mpd_cfg(state), &state.config.music_sources.music_root, uri).await
}

/// Append URI and jump to it — [`mpd::add_play_append_resolved`] plus optional video takeover.
pub async fn add_play_append_uri(state: &AppState, uri: &str) -> anyhow::Result<()> {
    #[cfg(feature = "video-companion")]
    if let Some(()) = crate::video_companion::try_take_over_add_play_append(state, uri).await? {
        state.set_video_playback_active(true);
        return Ok(());
    }
    clear_video_then_mpd(state);
    mpd::add_play_append_resolved(&mpd_cfg(state), &state.config.music_sources.music_root, uri).await
}

/// Clear queue, add many URIs, play at index — [`mpd::play_items_list_connected`] plus optional video takeover.
pub async fn play_items_list_uri(
    state: &AppState,
    uris: &[String],
    index: usize,
) -> anyhow::Result<()> {
    #[cfg(feature = "video-companion")]
    if let Some(()) =
        crate::video_companion::try_take_over_play_items_list(state, uris, index).await?
    {
        state.set_video_playback_active(true);
        return Ok(());
    }
    clear_video_then_mpd(state);
    mpd::play_items_list_connected(&mpd_cfg(state), uris, index).await
}

/// Same wire contract as [`mpd::run_command_connected`]. When **`video_playback_is_active`** and feature
/// **`video-companion`**, playback-like commands go to the video layer first (stub until mpv).
pub async fn run_command_connected_with_video(
    state: &AppState,
    cmd: &str,
    volume: Option<u8>,
    position: Option<i64>,
    repeat: Option<bool>,
    random: Option<bool>,
) -> anyhow::Result<()> {
    #[cfg(feature = "video-companion")]
    {
        if state.video_playback_is_active() {
            match cmd {
                "seek" | "play" | "pause" | "toggle" | "stop" | "next" | "prev" | "clearQueue" => {
                    crate::video_companion::transport_dispatch(state, cmd, position).await?;
                    return Ok(());
                }
                _ => {}
            }
        }
    }
    mpd::run_command_connected(
        &mpd_cfg(state),
        cmd,
        volume,
        position,
        repeat,
        random,
    )
    .await
}

/// Skip forward / backward within the current track (`skipForward` / `skipBackwards` socket events).
pub async fn skip_within_track_seconds(state: &AppState, delta_secs: i64) -> anyhow::Result<()> {
    #[cfg(feature = "video-companion")]
    if state.video_playback_is_active() {
        return crate::video_companion::transport_skip_relative(state, delta_secs).await;
    }
    let cfg = mpd_cfg(state);
    if delta_secs >= 0 {
        mpd::skip_forward_connected(&cfg, delta_secs as u64).await
    } else {
        mpd::skip_backwards_connected(&cfg, (-delta_secs) as u64).await
    }
}
